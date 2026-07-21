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
            kind: e.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
            TreeEntry { path: "src/en.json".into(), kind: "blob".into(), size: 128 }
        );
        // A tree entry with no size defaults to 0.
        assert_eq!(entries[0].kind, "tree");
        assert_eq!(entries[0].size, 0);
    }

    #[test]
    fn unsupported_platform_errors() {
        assert!(list_tree("gitlab", "https://x", "o", "r", "main", "t").is_err());
        assert!(read_file("bitbucket", "https://x", "o", "r", "main", "p", "t").is_err());
    }
}
