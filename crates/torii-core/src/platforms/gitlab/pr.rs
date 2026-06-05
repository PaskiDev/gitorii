//! GitLab — pr client.

use crate::error::{Result, ToriiError};
use crate::platforms::pr::*;
use reqwest::blocking::Client;

pub struct GitLabPrClient {
    token: String,
    base_url: String,
}

impl GitLabPrClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("gitlab", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "gitlab".into(),
                message: "GitLab token not found. Run: torii auth set gitlab YOUR_TOKEN"
                    .to_string(),
            })?;
        let base_url =
            std::env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com/api/v4".to_string());
        Ok(Self { token, base_url })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }

    fn project_path(owner: &str, repo: &str) -> String {
        crate::url::encode(&format!("{}/{}", owner, repo))
    }
}

impl PrClient for GitLabPrClient {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        let url = format!(
            "{}/projects/{}/merge_requests",
            self.base_url,
            Self::project_path(owner, repo)
        );
        let body = serde_json::json!({
            "title":         opts.title,
            "description":   opts.body.unwrap_or_default(),
            "source_branch": opts.head,
            "target_branch": opts.base,
            "draft":         opts.draft,
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body);
        let json = crate::http::send_json(req, "GitLab create MR")?;
        parse_gitlab_mr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let gl_state = match state {
            "open" => "opened",
            "closed" => "closed",
            "merged" => "merged",
            other => other,
        };
        let url = format!(
            "{}/projects/{}/merge_requests?state={}&per_page=50",
            self.base_url,
            Self::project_path(owner, repo),
            gl_state
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_gitlab_mr)
            .collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.base_url,
            Self::project_path(owner, repo),
            number
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab MR !{}", number))?;
        parse_gitlab_mr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/merge",
            self.base_url,
            Self::project_path(owner, repo),
            number
        );
        let squash = matches!(method, MergeMethod::Squash);
        let body = serde_json::json!({ "squash": squash });
        let req = self
            .client()
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body);
        crate::http::send_empty(req, "GitLab merge MR")
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.base_url,
            Self::project_path(owner, repo),
            number
        );
        let body = serde_json::json!({ "state_event": "close" });
        let req = self
            .client()
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body);
        crate::http::send_empty(req, "GitLab close MR")
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.base_url,
            Self::project_path(owner, repo),
            number
        );
        let mut body = serde_json::Map::new();
        if let Some(t) = opts.title {
            body.insert("title".into(), t.into());
        }
        if let Some(b) = opts.body {
            body.insert("description".into(), b.into());
        }
        if let Some(b) = opts.base {
            body.insert("target_branch".into(), b.into());
        }
        let req = self
            .client()
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "GitLab update MR")
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/repository/branches/{}",
            self.base_url,
            Self::project_path(owner, repo),
            crate::url::encode(branch)
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab delete branch")
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_gitlab_mr(json: &serde_json::Value) -> Result<PullRequest> {
    Ok(PullRequest {
        number: json["iid"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["description"].as_str().map(|s| s.to_string()),
        state: json["state"].as_str().unwrap_or("").to_string(),
        head: json["source_branch"].as_str().unwrap_or("").to_string(),
        base: json["target_branch"].as_str().unwrap_or("").to_string(),
        author: json["author"]["username"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        url: json["web_url"].as_str().unwrap_or("").to_string(),
        draft: json["draft"].as_bool().unwrap_or(false),
        mergeable: json["merge_status"].as_str().map(|s| s == "can_be_merged"),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Gitea / Codeberg / Forgejo
// ============================================================================
//
// Gitea's pulls API is GitHub-shaped at `/api/v1/...`. Same `number`
// identifier, same `head`/`base`/`draft` fields, same `merge_method`
// values for merge — auth header is `token <token>` like GitHub.
// `mergeable` is exposed as a boolean rather than GitHub's null/true
// while-computing dance, so we surface it directly.

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    // ── parser ───────────────────────────────────────────────────────────

    #[test]
    fn parse_gitlab_mr_full() {
        let json = serde_json::json!({
            "iid": 42u64,
            "title": "Add feature",
            "description": "Implements the thing.",
            "state": "opened",
            "source_branch": "feature/x",
            "target_branch": "main",
            "author": { "username": "paski" },
            "web_url": "https://gitlab.com/acme/widget/-/merge_requests/42",
            "draft": true,
            "merge_status": "can_be_merged",
            "created_at": "2026-06-01T12:00:00Z"
        });
        let pr = parse_gitlab_mr(&json).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "Add feature");
        assert_eq!(pr.body.as_deref(), Some("Implements the thing."));
        assert_eq!(pr.state, "opened");
        assert_eq!(pr.head, "feature/x");
        assert_eq!(pr.base, "main");
        assert_eq!(pr.author, "paski");
        assert_eq!(pr.url, "https://gitlab.com/acme/widget/-/merge_requests/42");
        assert!(pr.draft);
        assert_eq!(pr.mergeable, Some(true));
        assert_eq!(pr.created_at, "2026-06-01T12:00:00Z");
    }

    #[test]
    fn parse_gitlab_mr_missing_optionals_defaults() {
        let json = serde_json::json!({
            "iid": 7u64,
            "title": "t",
            "state": "merged",
            "source_branch": "b",
            "target_branch": "main"
        });
        let pr = parse_gitlab_mr(&json).unwrap();
        assert_eq!(pr.number, 7);
        assert_eq!(pr.body, None);
        assert_eq!(pr.author, "");
        assert!(!pr.draft);
        assert_eq!(pr.mergeable, None);
        assert_eq!(pr.created_at, "");
    }

    #[test]
    fn parse_gitlab_mr_cannot_be_merged_maps_to_false() {
        let json = serde_json::json!({ "iid": 1u64, "merge_status": "cannot_be_merged" });
        assert_eq!(parse_gitlab_mr(&json).unwrap().mergeable, Some(false));
    }

    // ── client (httpmock) ────────────────────────────────────────────────

    fn client(server: &MockServer) -> GitLabPrClient {
        GitLabPrClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn list_translates_open_state_and_parses_mrs() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/merge_requests")
                .query_param("state", "opened")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([{
                "iid": 5u64, "title": "MR five", "state": "opened",
                "source_branch": "f", "target_branch": "main",
                "author": { "username": "paski" },
                "web_url": "https://x", "created_at": ""
            }]));
        });
        let prs = client(&server).list("acme", "widget", "open").unwrap();
        m.assert();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 5);
        assert_eq!(prs[0].title, "MR five");
    }

    #[test]
    fn close_sends_put_state_event_with_bearer_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(PUT)
                .path("/projects/acme%2Fwidget/merge_requests/7")
                .header("Authorization", "Bearer test-token")
                .json_body(serde_json::json!({ "state_event": "close" }));
            then.status(200);
        });
        client(&server).close("acme", "widget", 7).unwrap();
        m.assert();
    }

    #[test]
    fn get_non_2xx_maps_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/merge_requests/404");
            then.status(404)
                .json_body(serde_json::json!({ "message": "404 Not found" }));
        });
        let err = client(&server).get("acme", "widget", 404).unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
