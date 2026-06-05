//! GitLab — issue client.

use crate::error::{Result, ToriiError};
use crate::platforms::issue::*;
use reqwest::blocking::Client;

pub struct GitLabIssueClient {
    token: String,
    base_url: String,
}

impl GitLabIssueClient {
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

impl IssueClient for GitLabIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        let gl_state = match state {
            "open" => "opened",
            "closed" => "closed",
            other => other,
        };
        let url = format!(
            "{}/projects/{}/issues?state={}&per_page=50",
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
            .map(parse_gitlab_issue)
            .collect()
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!(
            "{}/projects/{}/issues",
            self.base_url,
            Self::project_path(owner, repo)
        );
        let body = serde_json::json!({
            "title":       opts.title,
            "description": opts.body.unwrap_or_default(),
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body);
        let json = crate::http::send_json(req, "GitLab create issue")?;
        parse_gitlab_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/projects/{}/issues/{}",
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
        crate::http::send_empty(req, "GitLab close issue")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/issues/{}/notes",
            self.base_url,
            Self::project_path(owner, repo),
            number
        );
        let payload = serde_json::json!({ "body": body });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&payload);
        crate::http::send_empty(req, "GitLab comment issue")
    }
}

fn parse_gitlab_issue(json: &serde_json::Value) -> Result<Issue> {
    Ok(Issue {
        number: json["iid"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["description"].as_str().map(|s| s.to_string()),
        state: json["state"].as_str().unwrap_or("").to_string(),
        author: json["author"]["username"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        url: json["web_url"].as_str().unwrap_or("").to_string(),
        labels: json["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|l| l.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        assignees: json["assignees"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|u| u["username"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
        comments: json["user_notes_count"].as_u64().unwrap_or(0),
    })
}

// ── Gitea / Codeberg / Forgejo ────────────────────────────────────────────────
//
// Gitea's issues API mirrors GitHub's at `/api/v1/...` — same field
// names, same `number` identifier (per-repo), same auth header.

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    // ── parser ───────────────────────────────────────────────────────────

    #[test]
    fn parse_gitlab_issue_full() {
        let json = serde_json::json!({
            "iid": 12u64,
            "title": "Crash on startup",
            "description": "Steps to reproduce…",
            "state": "opened",
            "author": { "username": "paski" },
            "web_url": "https://gitlab.com/acme/widget/-/issues/12",
            "labels": ["bug", "p1"],
            "assignees": [{ "username": "alice" }, { "username": "bob" }],
            "created_at": "2026-06-02T08:30:00Z",
            "user_notes_count": 3u64
        });
        let issue = parse_gitlab_issue(&json).unwrap();
        assert_eq!(issue.number, 12);
        assert_eq!(issue.title, "Crash on startup");
        assert_eq!(issue.body.as_deref(), Some("Steps to reproduce…"));
        assert_eq!(issue.state, "opened");
        assert_eq!(issue.author, "paski");
        assert_eq!(issue.url, "https://gitlab.com/acme/widget/-/issues/12");
        assert_eq!(issue.labels, vec!["bug", "p1"]);
        assert_eq!(issue.assignees, vec!["alice", "bob"]);
        assert_eq!(issue.created_at, "2026-06-02T08:30:00Z");
        assert_eq!(issue.comments, 3);
    }

    #[test]
    fn parse_gitlab_issue_missing_optionals_defaults() {
        let json = serde_json::json!({ "iid": 3u64, "title": "bare", "state": "closed" });
        let issue = parse_gitlab_issue(&json).unwrap();
        assert_eq!(issue.number, 3);
        assert_eq!(issue.body, None);
        assert_eq!(issue.author, "");
        assert!(issue.labels.is_empty());
        assert!(issue.assignees.is_empty());
        assert_eq!(issue.comments, 0);
    }

    // ── client (httpmock) ────────────────────────────────────────────────

    fn client(server: &MockServer) -> GitLabIssueClient {
        GitLabIssueClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn list_translates_open_state_and_parses_issues() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/issues")
                .query_param("state", "opened")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([{
                "iid": 9u64, "title": "Issue nine", "state": "opened",
                "author": { "username": "paski" }, "web_url": "https://x",
                "labels": ["bug"], "assignees": [], "created_at": "",
                "user_notes_count": 1u64
            }]));
        });
        let issues = client(&server).list("acme", "widget", "open").unwrap();
        m.assert();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 9);
        assert_eq!(issues[0].labels, vec!["bug"]);
    }

    #[test]
    fn comment_posts_note_with_bearer_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/projects/acme%2Fwidget/issues/5/notes")
                .header("Authorization", "Bearer test-token")
                .json_body(serde_json::json!({ "body": "lgtm" }));
            then.status(201);
        });
        client(&server)
            .comment("acme", "widget", 5, "lgtm")
            .unwrap();
        m.assert();
    }

    #[test]
    fn create_non_2xx_maps_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/projects/acme%2Fwidget/issues");
            then.status(500)
                .json_body(serde_json::json!({ "message": "boom" }));
        });
        let opts = CreateIssueOptions {
            title: "x".into(),
            body: None,
        };
        let err = client(&server).create("acme", "widget", opts).unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
