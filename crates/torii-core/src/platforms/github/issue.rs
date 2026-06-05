//! GitHub — issue client.

use crate::error::{Result, ToriiError};
use crate::platforms::issue::*;
use reqwest::blocking::Client;

pub struct GitHubIssueClient {
    token: String,
    base_url: String,
}

impl GitHubIssueClient {
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

impl IssueClient for GitHubIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        let url = format!(
            "{}/repos/{}/{}/issues?state={}&per_page=50",
            self.base_url, owner, repo, state
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        // filter out PRs (GitHub issues API returns PRs too)
        Ok(crate::http::extract_array(&json, &url)?
            .iter()
            .filter(|v| v["pull_request"].is_null())
            .filter_map(|v| parse_github_issue(v).ok())
            .collect())
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!("{}/repos/{}/{}/issues", self.base_url, owner, repo);
        let body = serde_json::json!({
            "title": opts.title,
            "body":  opts.body.unwrap_or_default(),
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        let json = crate::http::send_json(req, "GitHub create issue")?;
        parse_github_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            self.base_url, owner, repo, number
        );
        let body = serde_json::json!({ "state": "closed" });
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        crate::http::send_empty(req, "GitHub close issue")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            self.base_url, owner, repo, number
        );
        let payload = serde_json::json!({ "body": body });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&payload);
        crate::http::send_empty(req, "GitHub comment issue")
    }
}

fn parse_github_issue(json: &serde_json::Value) -> Result<Issue> {
    Ok(Issue {
        number: json["number"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["body"].as_str().map(|s| s.to_string()),
        state: json["state"].as_str().unwrap_or("").to_string(),
        author: json["user"]["login"].as_str().unwrap_or("").to_string(),
        url: json["html_url"].as_str().unwrap_or("").to_string(),
        labels: json["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        assignees: json["assignees"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|u| u["login"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
        comments: json["comments"].as_u64().unwrap_or(0),
    })
}

// ── GitLab ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn client_for(server: &MockServer) -> GitHubIssueClient {
        GitHubIssueClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn parse_github_issue_maps_all_fields() {
        let json = serde_json::json!({
            "number": 12u64,
            "title": "Bug report",
            "body": "It breaks",
            "state": "open",
            "user": { "login": "bob" },
            "html_url": "https://github.com/o/r/issues/12",
            "labels": [{ "name": "bug" }, { "name": "p1" }],
            "assignees": [{ "login": "alice" }],
            "created_at": "2026-02-03T00:00:00Z",
            "comments": 4u64,
        });
        let issue = parse_github_issue(&json).unwrap();
        assert_eq!(issue.number, 12);
        assert_eq!(issue.title, "Bug report");
        assert_eq!(issue.body.as_deref(), Some("It breaks"));
        assert_eq!(issue.state, "open");
        assert_eq!(issue.author, "bob");
        assert_eq!(issue.url, "https://github.com/o/r/issues/12");
        assert_eq!(issue.labels, vec!["bug".to_string(), "p1".to_string()]);
        assert_eq!(issue.assignees, vec!["alice".to_string()]);
        assert_eq!(issue.created_at, "2026-02-03T00:00:00Z");
        assert_eq!(issue.comments, 4);
    }

    #[test]
    fn parse_github_issue_defaults_when_fields_missing() {
        let issue = parse_github_issue(&serde_json::json!({})).unwrap();
        assert_eq!(issue.number, 0);
        assert_eq!(issue.title, "");
        assert_eq!(issue.body, None);
        assert!(issue.labels.is_empty());
        assert!(issue.assignees.is_empty());
        assert_eq!(issue.comments, 0);
    }

    #[test]
    fn list_filters_out_pull_requests() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/octo/demo/issues")
                .query_param("state", "open")
                .query_param("per_page", "50")
                .header("Authorization", "token test-token");
            then.status(200).json_body(serde_json::json!([
                {
                    "number": 1, "title": "Real issue", "state": "open",
                    "user": { "login": "alice" }, "html_url": "https://x/1",
                    "created_at": "", "comments": 0,
                },
                {
                    "number": 2, "title": "Actually a PR", "state": "open",
                    "user": { "login": "bob" }, "html_url": "https://x/2",
                    "created_at": "", "comments": 0,
                    "pull_request": { "url": "https://api/pulls/2" },
                },
            ]));
        });
        let issues = client_for(&server).list("octo", "demo", "open").unwrap();
        m.assert();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 1);
        assert_eq!(issues[0].title, "Real issue");
    }

    #[test]
    fn close_patches_issue_with_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(PATCH)
                .path("/repos/octo/demo/issues/3")
                .header("Authorization", "token test-token")
                .json_body(serde_json::json!({ "state": "closed" }));
            then.status(200);
        });
        client_for(&server).close("octo", "demo", 3).unwrap();
        m.assert();
    }

    #[test]
    fn create_maps_500_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/repos/octo/demo/issues");
            then.status(500)
                .json_body(serde_json::json!({ "message": "boom" }));
        });
        let opts = CreateIssueOptions {
            title: "t".into(),
            body: None,
        };
        let err = client_for(&server)
            .create("octo", "demo", opts)
            .unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 500, .. }),
            "expected PlatformApi 500, got: {err:?}"
        );
    }
}
