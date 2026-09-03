//! Remote repository browsing — read a repo's file tree and file contents
//! via the platform's HTTP API, WITHOUT cloning.
//!
//! Unlike the other platform surfaces (issue/pr/…), the access token is
//! passed IN by the caller. Consumers like svitrio are multi-tenant and hold
//! a per-repo token themselves, so this must not read from the local
//! `torii auth` store. `api_base` is the platform API root
//! (e.g. `https://api.github.com`), so self-hosted instances work too.

use crate::error::{Result, ToriiError};

/// One entry in a repository tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeEntry {
    pub path: String,
    /// "blob" (file) or "tree" (directory).
    pub kind: String,
    pub size: u64,
}

fn unsupported(platform: &str) -> ToriiError {
    ToriiError::Other(anyhow::anyhow!(
        "remote contents not yet supported for platform {platform}"
    ))
}

/// List a repo's full (recursive) file tree at `git_ref` via the platform API.
pub fn list_tree(
    platform: &str,
    api_base: &str,
    owner: &str,
    repo: &str,
    git_ref: &str,
    token: &str,
) -> Result<Vec<TreeEntry>> {
    match platform {
        "github" => github_list_tree(api_base, owner, repo, git_ref, token),
        "gitlab" => gitlab_list_tree(api_base, owner, repo, git_ref, token),
        other => Err(unsupported(other)),
    }
}

/// Read one file's raw bytes at `git_ref` via the platform API.
pub fn read_file(
    platform: &str,
    api_base: &str,
    owner: &str,
    repo: &str,
    git_ref: &str,
    path: &str,
    token: &str,
) -> Result<Vec<u8>> {
    match platform {
        "github" => github_read_file(api_base, owner, repo, git_ref, path, token),
        "gitlab" => gitlab_read_file(api_base, owner, repo, git_ref, path, token),
        other => Err(unsupported(other)),
    }
}

// --- GitHub ----------------------------------------------------------------

fn github_auth(token: &str) -> String {
    format!("token {token}")
}

fn github_list_tree(
    api_base: &str,
    owner: &str,
    repo: &str,
    git_ref: &str,
    token: &str,
) -> Result<Vec<TreeEntry>> {
    let url = format!("{api_base}/repos/{owner}/{repo}/git/trees/{git_ref}?recursive=1");
    let req = crate::http::make_client()
        .get(&url)
        .header("Authorization", github_auth(token))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "torii");
    let json = crate::http::send_json(req, &format!("GitHub (url: {url})"))?;
    parse_github_tree(&json)
}

fn parse_github_tree(json: &serde_json::Value) -> Result<Vec<TreeEntry>> {
    let arr = json
        .get("tree")
        .and_then(|t| t.as_array())
        .ok_or_else(|| ToriiError::Other(anyhow::anyhow!("unexpected GitHub tree response")))?;
    let mut out = Vec::with_capacity(arr.len());
    for e in arr {
        let path = e.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() {
            continue;
        }
        out.push(TreeEntry {
            path: path.to_string(),
            kind: e
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            size: e.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
        });
    }
    Ok(out)
}

fn github_read_file(
    api_base: &str,
    owner: &str,
    repo: &str,
    git_ref: &str,
    path: &str,
    token: &str,
) -> Result<Vec<u8>> {
    let url = format!("{api_base}/repos/{owner}/{repo}/contents/{path}?ref={git_ref}");
    let req = crate::http::make_client()
        .get(&url)
        .header("Authorization", github_auth(token))
        // Raw media type returns the file bytes directly (no base64 wrapper).
        .header("Accept", "application/vnd.github.raw")
        .header("User-Agent", "torii");
    crate::http::send_bytes(req, &format!("GitHub (url: {url})"))
}

// --- GitLab ----------------------------------------------------------------

/// Percent-encode a path segment (project id or file path) for GitLab, which
/// wants `owner/repo` and file paths URL-encoded into the URL path.
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn gitlab_project(owner: &str, repo: &str) -> String {
    pct_encode(&format!("{owner}/{repo}"))
}

fn parse_gitlab_tree(json: &serde_json::Value) -> Vec<TreeEntry> {
    json.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let path = e.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    if path.is_empty() {
                        return None;
                    }
                    Some(TreeEntry {
                        path: path.to_string(),
                        kind: e
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        size: 0, // GitLab's tree listing carries no size
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn gitlab_list_tree(
    api_base: &str,
    owner: &str,
    repo: &str,
    git_ref: &str,
    token: &str,
) -> Result<Vec<TreeEntry>> {
    let proj = gitlab_project(owner, repo);
    // GitLab doesn't accept "HEAD"; omit ref to use the default branch.
    let ref_q = if git_ref.is_empty() || git_ref == "HEAD" {
        String::new()
    } else {
        format!("&ref={git_ref}")
    };
    let mut out = Vec::new();
    // GitLab paginates (max 100/page); loop until a short/empty page.
    for page in 1..=1000u32 {
        let url = format!(
            "{api_base}/projects/{proj}/repository/tree?recursive=true&per_page=100&page={page}{ref_q}"
        );
        let req = crate::http::make_client()
            .get(&url)
            .header("PRIVATE-TOKEN", token)
            .header("User-Agent", "torii");
        let json = crate::http::send_json(req, &format!("GitLab (url: {url})"))?;
        let n = json.as_array().map(|a| a.len()).unwrap_or(0);
        out.extend(parse_gitlab_tree(&json));
        if n < 100 {
            break;
        }
    }
    Ok(out)
}

fn gitlab_read_file(
    api_base: &str,
    owner: &str,
    repo: &str,
    git_ref: &str,
    path: &str,
    token: &str,
) -> Result<Vec<u8>> {
    let proj = gitlab_project(owner, repo);
    let enc_path = pct_encode(path);
    // GitLab's raw endpoint requires a ref; the caller passes the branch.
    let git_ref = if git_ref.is_empty() { "HEAD" } else { git_ref };
    let url = format!("{api_base}/projects/{proj}/repository/files/{enc_path}/raw?ref={git_ref}");
    let req = crate::http::make_client()
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .header("User-Agent", "torii");
    crate::http::send_bytes(req, &format!("GitLab (url: {url})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_github_recursive_tree() {
        let json = serde_json::json!({
            "sha": "abc",
            "tree": [
                { "path": "src", "type": "tree", "mode": "040000" },
                { "path": "src/en.json", "type": "blob", "size": 128, "mode": "100644" },
                { "path": "README.md", "type": "blob", "size": 42, "mode": "100644" }
            ],
            "truncated": false
        });
        let entries = parse_github_tree(&json).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[1],
            TreeEntry {
                path: "src/en.json".into(),
                kind: "blob".into(),
                size: 128
            }
        );
        // A tree entry with no size defaults to 0.
        assert_eq!(entries[0].kind, "tree");
        assert_eq!(entries[0].size, 0);
    }

    #[test]
    fn unsupported_platform_errors() {
        assert!(list_tree("azure", "https://x", "o", "r", "main", "t").is_err());
        assert!(read_file("sourcehut", "https://x", "o", "r", "main", "p", "t").is_err());
    }

    #[test]
    fn gitlab_project_is_url_encoded() {
        assert_eq!(gitlab_project("syrakon", "svitrio"), "syrakon%2Fsvitrio");
        // A subgroup path keeps its slashes encoded too.
        assert_eq!(pct_encode("a/b c"), "a%2Fb%20c");
    }

    #[test]
    fn parses_a_gitlab_tree_page() {
        let json = serde_json::json!([
            { "id": "1", "name": "src", "type": "tree", "path": "src", "mode": "040000" },
            { "id": "2", "name": "en.json", "type": "blob", "path": "src/en.json", "mode": "100644" }
        ]);
        let entries = parse_gitlab_tree(&json);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[1],
            TreeEntry {
                path: "src/en.json".into(),
                kind: "blob".into(),
                size: 0
            }
        );
    }
}
