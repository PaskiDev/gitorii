//! Azure DevOps — pr client.

use crate::error::{Result, ToriiError};
use crate::platforms::pr::*;
use reqwest::blocking::Client;

pub struct AzurePrClient {
    token: String,
}

impl AzurePrClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("azure", ".").value
            .ok_or_else(|| ToriiError::Auth { provider: "azure".into(), message: "Azure DevOps PAT not found. Create one at https://dev.azure.com/{org}/_usersSettings/tokens \
                 with scopes `Code (read/write)`, `Build (read/execute)`, `Work Items (read/write)`, \
                 `Release (read/write)` and run: torii auth set azure YOUR_PAT".to_string() })?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }

    /// Azure PATs go in the password slot with an empty username.
    /// Equivalent to `Authorization: Basic <base64(":PAT")>`.
    fn auth(&self) -> String {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(format!(":{}", self.token));
        format!("Basic {}", b64)
    }
}

/// Split the packed `org/project` owner back into its parts. Returns
/// a clear error if the owner doesn't contain a `/` — that means the
/// URL parser saw something unexpected.
pub(crate) fn split_azure_owner(owner: &str) -> Result<(String, String)> {
    let mut parts = owner.splitn(2, '/');
    let org =
        parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "azure".into(),
                message: format!("Azure: cannot parse organisation from owner '{}'", owner),
            })?;
    let project = parts.next().filter(|s| !s.is_empty()).ok_or_else(|| {
        ToriiError::InvalidConfig(format!(
            "Azure: cannot parse project from owner '{}' — \
                 expected 'org/project' (URL parser should populate both)",
            owner
        ))
    })?;
    Ok((org.to_string(), project.to_string()))
}

impl PrClient for AzurePrClient {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        let (org, project) = split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullrequests?api-version=7.0",
            org, project, repo
        );
        // Azure expects fully-qualified ref names.
        let body = serde_json::json!({
            "title":         opts.title,
            "description":   opts.body.unwrap_or_default(),
            "sourceRefName": format!("refs/heads/{}", opts.head),
            "targetRefName": format!("refs/heads/{}", opts.base),
            "isDraft":       opts.draft,
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        let json = crate::http::send_json(req, "Azure create PR")?;
        parse_azure_pr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let (org, project) = split_azure_owner(owner)?;
        let az_state = match state {
            "open" => "active",
            "closed" => "abandoned",
            "merged" => "completed",
            _ => "active",
        };
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullrequests\
             ?searchCriteria.status={}&$top=50&api-version=7.0",
            org, project, repo, az_state
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, &format!("Azure (url: {})", url))?;
        let arr = json["value"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "azure".into(),
                message: format!("Azure returned no `value` array. Body: {}", json),
            })?;
        arr.iter().map(parse_azure_pr).collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let (org, project) = split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullrequests/{}?api-version=7.0",
            org, project, repo, number
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, &format!("Azure PR #{}", number))?;
        parse_azure_pr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let (org, project) = split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullrequests/{}?api-version=7.0",
            org, project, repo, number
        );
        // Azure merges by transitioning status → "completed" with a
        // completionOptions block. mergeStrategy: noFastForward (≈ merge
        // commit) / squash / rebase / rebaseMerge.
        let strategy = match method {
            MergeMethod::Merge => "noFastForward",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        };
        let body = serde_json::json!({
            "status": "completed",
            "completionOptions": { "mergeStrategy": strategy }
        });
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Azure merge PR")
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let (org, project) = split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullrequests/{}?api-version=7.0",
            org, project, repo, number
        );
        let body = serde_json::json!({ "status": "abandoned" });
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Azure abandon PR")
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let (org, project) = split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullrequests/{}?api-version=7.0",
            org, project, repo, number
        );
        let mut body = serde_json::Map::new();
        if let Some(t) = opts.title {
            body.insert("title".into(), serde_json::Value::String(t));
        }
        if let Some(b) = opts.body {
            body.insert("description".into(), serde_json::Value::String(b));
        }
        if let Some(base) = opts.base {
            body.insert(
                "targetRefName".into(),
                serde_json::Value::String(format!("refs/heads/{}", base)),
            );
        }
        if body.is_empty() {
            return Ok(());
        }
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "Azure update PR")
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        // Azure deletes a ref by POSTing the refUpdates list with the
        // old object id and an all-zeros new object id. This needs the
        // current SHA of the ref, which means an extra round-trip.
        let (org, project) = split_azure_owner(owner)?;
        let lookup_url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/refs?filter=heads/{}&api-version=7.0",
            org, project, repo, branch
        );
        let lookup_req = self
            .client()
            .get(&lookup_url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let lookup_json = crate::http::send_json(lookup_req, "Azure lookup ref")?;
        let old_oid = lookup_json["value"][0]["objectId"]
            .as_str()
            .ok_or_else(|| {
                ToriiError::BranchNotFound(format!(
                    "Azure: branch '{}' not found on remote",
                    branch
                ))
            })?;

        let update_url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/refs?api-version=7.0",
            org, project, repo
        );
        let body = serde_json::json!([{
            "name":        format!("refs/heads/{}", branch),
            "oldObjectId": old_oid,
            "newObjectId": "0000000000000000000000000000000000000000",
        }]);
        let req = self
            .client()
            .post(&update_url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Azure delete branch")
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_azure_pr(json: &serde_json::Value) -> Result<PullRequest> {
    // Azure surfaces ref names as `refs/heads/foo` — strip the prefix
    // so the value matches how every other client reports it.
    fn strip_ref(s: &str) -> String {
        s.trim_start_matches("refs/heads/").to_string()
    }
    Ok(PullRequest {
        number: json["pullRequestId"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["description"].as_str().map(String::from),
        state: match json["status"].as_str().unwrap_or("") {
            "active" => "open".to_string(),
            "abandoned" => "closed".to_string(),
            "completed" => "merged".to_string(),
            other => other.to_string(),
        },
        head: strip_ref(json["sourceRefName"].as_str().unwrap_or("")),
        base: strip_ref(json["targetRefName"].as_str().unwrap_or("")),
        author: json["createdBy"]["displayName"]
            .as_str()
            .or_else(|| json["createdBy"]["uniqueName"].as_str())
            .unwrap_or("")
            .to_string(),
        url: json["url"].as_str().unwrap_or("").to_string(),
        draft: json["isDraft"].as_bool().unwrap_or(false),
        mergeable: json["mergeStatus"].as_str().map(|s| s == "succeeded"),
        created_at: json["creationDate"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Factory
// ============================================================================

/// Extract `(org, project, repo)` from any of the three Azure DevOps
/// URL shapes. Returns `None` if the URL doesn't match a known shape.
pub(crate) fn parse_azure_url(url: &str) -> Option<(String, String, String)> {
    // SSH: git@ssh.dev.azure.com:v3/<org>/<project>/<repo>
    if let Some(rest) = url.strip_prefix("git@ssh.dev.azure.com:") {
        let rest = rest.trim_start_matches("v3/").trim_end_matches(".git");
        let mut parts = rest.splitn(3, '/');
        let org = parts.next()?.to_string();
        let project = parts.next()?.to_string();
        let repo = parts.next()?.to_string();
        return Some((org, project, repo));
    }
    // HTTPS legacy: https://<org>.visualstudio.com/<project>/_git/<repo>
    if let Some(after_scheme) = url.split("://").nth(1) {
        if let Some(host_end) = after_scheme.find('/') {
            let host = &after_scheme[..host_end];
            let path = &after_scheme[host_end + 1..].trim_end_matches(".git");
            if let Some(org) = host.strip_suffix(".visualstudio.com") {
                // path = "<project>/_git/<repo>"
                let mut parts = path.splitn(3, '/');
                let project = parts.next()?.to_string();
                let _git_marker = parts.next()?;
                let repo = parts.next()?.to_string();
                return Some((org.to_string(), project, repo));
            }
            // HTTPS modern: dev.azure.com/<org>/<project>/_git/<repo>
            // (host might also include "<org>@dev.azure.com" for legacy
            // basic-auth-in-URL — strip the userinfo.)
            let host = host.split('@').next_back().unwrap_or(host);
            if host == "dev.azure.com" {
                let mut parts = path.splitn(4, '/');
                let org = parts.next()?.to_string();
                let project = parts.next()?.to_string();
                let _git_marker = parts.next()?;
                let repo = parts.next()?.to_string();
                return Some((org, project, repo));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── split_azure_owner ─────────────────────────────────────────────

    #[test]
    fn split_azure_owner_org_project_ok() {
        let (org, project) = split_azure_owner("myorg/myproject").unwrap();
        assert_eq!(org, "myorg");
        assert_eq!(project, "myproject");
    }

    #[test]
    fn split_azure_owner_missing_project_is_err() {
        assert!(split_azure_owner("soloorg").is_err());
    }

    #[test]
    fn split_azure_owner_empty_project_is_err() {
        assert!(split_azure_owner("org/").is_err());
    }

    #[test]
    fn split_azure_owner_empty_org_is_err() {
        assert!(split_azure_owner("/project").is_err());
    }

    #[test]
    fn split_azure_owner_splits_only_on_first_slash() {
        // splitn(2, '/') — anything after the first slash belongs to
        // the project part verbatim.
        let (org, project) = split_azure_owner("org/team/project").unwrap();
        assert_eq!(org, "org");
        assert_eq!(project, "team/project");
    }

    // ── parse_azure_pr ────────────────────────────────────────────────

    #[test]
    fn parse_azure_pr_full() {
        let json = serde_json::json!({
            "pullRequestId": 42u64,
            "title": "Add feature",
            "description": "Long description",
            "status": "active",
            "sourceRefName": "refs/heads/feature/x",
            "targetRefName": "refs/heads/main",
            "createdBy": { "displayName": "Jane Doe", "uniqueName": "jane@example.com" },
            "url": "https://dev.azure.com/org/proj/_apis/git/repositories/repo/pullRequests/42",
            "isDraft": true,
            "mergeStatus": "succeeded",
            "creationDate": "2026-01-02T03:04:05Z",
        });
        let pr = parse_azure_pr(&json).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "Add feature");
        assert_eq!(pr.body.as_deref(), Some("Long description"));
        assert_eq!(pr.state, "open");
        assert_eq!(pr.head, "feature/x");
        assert_eq!(pr.base, "main");
        assert_eq!(pr.author, "Jane Doe");
        assert!(pr.draft);
        assert_eq!(pr.mergeable, Some(true));
        assert_eq!(pr.created_at, "2026-01-02T03:04:05Z");
    }

    #[test]
    fn parse_azure_pr_state_mapping() {
        for (az, ours) in [
            ("active", "open"),
            ("abandoned", "closed"),
            ("completed", "merged"),
            ("notSet", "notSet"), // unknown statuses pass through raw
        ] {
            let json = serde_json::json!({ "status": az });
            assert_eq!(parse_azure_pr(&json).unwrap().state, ours);
        }
    }

    #[test]
    fn parse_azure_pr_minimal_defaults() {
        let json = serde_json::json!({});
        let pr = parse_azure_pr(&json).unwrap();
        assert_eq!(pr.number, 0);
        assert_eq!(pr.title, "");
        assert_eq!(pr.body, None);
        assert_eq!(pr.head, "");
        assert_eq!(pr.author, "");
        assert!(!pr.draft);
        assert_eq!(pr.mergeable, None);
    }

    #[test]
    fn parse_azure_pr_author_falls_back_to_unique_name() {
        let json = serde_json::json!({
            "createdBy": { "uniqueName": "jane@example.com" }
        });
        assert_eq!(parse_azure_pr(&json).unwrap().author, "jane@example.com");
    }

    #[test]
    fn parse_azure_pr_merge_status_conflicts_is_not_mergeable() {
        let json = serde_json::json!({ "mergeStatus": "conflicts" });
        assert_eq!(parse_azure_pr(&json).unwrap().mergeable, Some(false));
    }

    // ── parse_azure_url ───────────────────────────────────────────────

    #[test]
    fn parse_azure_url_ssh() {
        assert_eq!(
            parse_azure_url("git@ssh.dev.azure.com:v3/org/project/repo"),
            Some(("org".into(), "project".into(), "repo".into()))
        );
    }

    #[test]
    fn parse_azure_url_ssh_strips_git_suffix() {
        assert_eq!(
            parse_azure_url("git@ssh.dev.azure.com:v3/org/project/repo.git"),
            Some(("org".into(), "project".into(), "repo".into()))
        );
    }

    #[test]
    fn parse_azure_url_https_modern() {
        assert_eq!(
            parse_azure_url("https://dev.azure.com/org/project/_git/repo"),
            Some(("org".into(), "project".into(), "repo".into()))
        );
    }

    #[test]
    fn parse_azure_url_https_modern_with_userinfo() {
        assert_eq!(
            parse_azure_url("https://org@dev.azure.com/org/project/_git/repo"),
            Some(("org".into(), "project".into(), "repo".into()))
        );
    }

    #[test]
    fn parse_azure_url_https_legacy_visualstudio() {
        assert_eq!(
            parse_azure_url("https://org.visualstudio.com/project/_git/repo"),
            Some(("org".into(), "project".into(), "repo".into()))
        );
    }

    #[test]
    fn parse_azure_url_non_azure_returns_none() {
        assert_eq!(parse_azure_url("https://github.com/owner/repo.git"), None);
        assert_eq!(parse_azure_url("git@github.com:owner/repo.git"), None);
        assert_eq!(parse_azure_url("not a url"), None);
    }

    #[test]
    fn parse_azure_url_incomplete_path_returns_none() {
        // Missing the repo segment after `_git`.
        assert_eq!(
            parse_azure_url("https://dev.azure.com/org/project/_git"),
            None
        );
        assert_eq!(
            parse_azure_url("git@ssh.dev.azure.com:v3/org/project"),
            None
        );
    }
}
