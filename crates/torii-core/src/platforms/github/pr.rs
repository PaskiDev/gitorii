//! GitHub — pr client.

use crate::error::{Result, ToriiError};
use crate::platforms::pr::*;
use reqwest::blocking::Client;

pub struct GitHubPrClient {
    token: String,
    base_url: String,
}

impl GitHubPrClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("github", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "github".into(),
                message: "GitHub token not found. Run: torii auth set github YOUR_TOKEN"
                    .to_string(),
            })?;
        Ok(Self {
            token,
            base_url: "https://api.github.com".to_string(),
        })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }

    fn auth(&self) -> String {
        format!("token {}", self.token)
    }
}

impl PrClient for GitHubPrClient {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        let url = format!("{}/repos/{}/{}/pulls", self.base_url, owner, repo);
        let body = serde_json::json!({
            "title": opts.title,
            "body":  opts.body.unwrap_or_default(),
            "head":  opts.head,
            "base":  opts.base,
            "draft": opts.draft,
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        let json = crate::http::send_json(req, "GitHub create PR")?;
        parse_github_pr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let url = format!(
            "{}/repos/{}/{}/pulls?state={}&per_page=50",
            self.base_url, owner, repo, state
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_github_pr)
            .collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json");
        let json = crate::http::send_json(req, &format!("GitHub PR #{}", number))?;
        parse_github_pr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/merge",
            self.base_url, owner, repo, number
        );
        let body = serde_json::json!({ "merge_method": method.to_string() });
        let req = self
            .client()
            .put(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        crate::http::send_empty(req, "GitHub merge PR")
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );
        let body = serde_json::json!({ "state": "closed" });
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        crate::http::send_empty(req, "GitHub close PR")
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );
        let mut body = serde_json::Map::new();
        if let Some(t) = opts.title {
            body.insert("title".into(), t.into());
        }
        if let Some(b) = opts.body {
            body.insert("body".into(), b.into());
        }
        if let Some(b) = opts.base {
            body.insert("base".into(), b.into());
        }
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "GitHub update PR")
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/git/refs/heads/{}",
            self.base_url, owner, repo, branch
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json");
        crate::http::send_empty(req, "GitHub delete branch")
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_github_pr(json: &serde_json::Value) -> Result<PullRequest> {
    Ok(PullRequest {
        number: json["number"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["body"].as_str().map(|s| s.to_string()),
        state: json["state"].as_str().unwrap_or("").to_string(),
        head: json["head"]["ref"].as_str().unwrap_or("").to_string(),
        base: json["base"]["ref"].as_str().unwrap_or("").to_string(),
        author: json["user"]["login"].as_str().unwrap_or("").to_string(),
        url: json["html_url"].as_str().unwrap_or("").to_string(),
        draft: json["draft"].as_bool().unwrap_or(false),
        mergeable: json["mergeable"].as_bool(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// GitLab (Merge Requests)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn client_for(server: &MockServer) -> GitHubPrClient {
        GitHubPrClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn parse_github_pr_maps_all_fields() {
        let json = serde_json::json!({
            "number": 42u64,
            "title": "Add feature",
            "body": "Long description",
            "state": "open",
            "head": { "ref": "feature/x" },
            "base": { "ref": "main" },
            "user": { "login": "octocat" },
            "html_url": "https://github.com/o/r/pull/42",
            "draft": true,
            "mergeable": false,
            "created_at": "2026-01-02T03:04:05Z",
        });
        let pr = parse_github_pr(&json).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "Add feature");
        assert_eq!(pr.body.as_deref(), Some("Long description"));
        assert_eq!(pr.state, "open");
        assert_eq!(pr.head, "feature/x");
        assert_eq!(pr.base, "main");
        assert_eq!(pr.author, "octocat");
        assert_eq!(pr.url, "https://github.com/o/r/pull/42");
        assert!(pr.draft);
        assert_eq!(pr.mergeable, Some(false));
        assert_eq!(pr.created_at, "2026-01-02T03:04:05Z");
    }

    #[test]
    fn parse_github_pr_defaults_when_fields_missing() {
        let pr = parse_github_pr(&serde_json::json!({})).unwrap();
        assert_eq!(pr.number, 0);
        assert_eq!(pr.title, "");
        assert_eq!(pr.body, None);
        assert_eq!(pr.head, "");
        assert_eq!(pr.base, "");
        assert!(!pr.draft);
        assert_eq!(pr.mergeable, None);
    }

    #[test]
    fn list_parses_pull_requests_from_api() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/octo/demo/pulls")
                .query_param("state", "open")
                .query_param("per_page", "50")
                .header("Authorization", "token test-token");
            then.status(200).json_body(serde_json::json!([{
                "number": 7,
                "title": "First",
                "state": "open",
                "head": { "ref": "topic" },
                "base": { "ref": "main" },
                "user": { "login": "alice" },
                "html_url": "https://x/pull/7",
                "draft": false,
                "created_at": "2026-01-01T00:00:00Z",
            }]));
        });
        let prs = client_for(&server).list("octo", "demo", "open").unwrap();
        m.assert();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 7);
        assert_eq!(prs[0].head, "topic");
        assert_eq!(prs[0].author, "alice");
    }

    #[test]
    fn merge_puts_to_merge_endpoint_with_auth_and_method() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(PUT)
                .path("/repos/octo/demo/pulls/5/merge")
                .header("Authorization", "token test-token")
                .json_body(serde_json::json!({ "merge_method": "squash" }));
            then.status(200)
                .json_body(serde_json::json!({ "merged": true }));
        });
        client_for(&server)
            .merge("octo", "demo", 5, MergeMethod::Squash)
            .unwrap();
        m.assert();
    }

    #[test]
    fn get_maps_404_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/repos/octo/demo/pulls/99");
            then.status(404)
                .json_body(serde_json::json!({ "message": "Not Found" }));
        });
        let err = client_for(&server).get("octo", "demo", 99).unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 404, .. }),
            "expected PlatformApi 404, got: {err:?}"
        );
    }
}
