//! Gitea / Codeberg / Forgejo — issue client.

use crate::error::Result;
use crate::platforms::issue::*;
use reqwest::blocking::Client;

pub struct GiteaIssueClient {
    token: String,
    base_url: String,
}

impl GiteaIssueClient {
    pub fn new() -> Result<Self> {
        Self::new_with_host(crate::pr::gitea_base_url())
    }

    pub fn new_with_host(base_url: &str) -> Result<Self> {
        let token = crate::pr::resolve_gitea_token()?;
        Ok(Self {
            token,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
    fn auth(&self) -> String {
        format!("token {}", self.token)
    }
}

impl IssueClient for GiteaIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        // Gitea uses `type=issues` to exclude PRs from the listing.
        let url = format!(
            "{}/api/v1/repos/{}/{}/issues?state={}&type=issues&limit=50",
            self.base_url, owner, repo, state
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Gitea (url: {})", url))?;
        Ok(crate::http::extract_array(&json, &url)?
            .iter()
            .filter_map(|v| parse_gitea_issue(v).ok())
            .collect())
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!("{}/api/v1/repos/{}/{}/issues", self.base_url, owner, repo);
        let body = serde_json::json!({
            "title": opts.title,
            "body":  opts.body.unwrap_or_default(),
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .json(&body);
        let json = crate::http::send_json(req, "Gitea create issue")?;
        parse_gitea_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/issues/{}",
            self.base_url, owner, repo, number
        );
        let body = serde_json::json!({ "state": "closed" });
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .json(&body);
        crate::http::send_empty(req, "Gitea close issue")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/issues/{}/comments",
            self.base_url, owner, repo, number
        );
        let payload = serde_json::json!({ "body": body });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .json(&payload);
        crate::http::send_empty(req, "Gitea comment issue")
    }
}

fn parse_gitea_issue(json: &serde_json::Value) -> Result<Issue> {
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

// ── Sourcehut (todo.sr.ht) ───────────────────────────────────────────────────
//
// Sourcehut's bug tracker lives on a separate subdomain from the git
// host. The convention is `~user/<tracker-name>` where tracker name is
// usually the same as the repo (but not enforced) — projects sometimes
// have multiple trackers (e.g. `-bugs`, `-features`). We assume
// `tracker_name == repo_name`; if the user uses a different naming
// scheme they can pass `--remote` to a remote whose URL points at the
// correct tracker (in 0.8.0 with platforms.toml this will be
// configurable per-host).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ToriiError;
    use httpmock::prelude::*;

    fn client(server: &MockServer) -> GiteaIssueClient {
        GiteaIssueClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    fn issue_json(number: u64) -> serde_json::Value {
        serde_json::json!({
            "number": number,
            "title": "Crash on startup",
            "body": "stack trace attached",
            "state": "open",
            "user": { "login": "bob" },
            "html_url": "https://codeberg.org/o/r/issues/12",
            "labels": [{ "name": "bug" }, { "name": "p1" }],
            "assignees": [{ "login": "alice" }, { "login": "carol" }],
            "created_at": "2026-02-03T04:05:06Z",
            "comments": 4,
        })
    }

    #[test]
    fn parse_gitea_issue_extracts_all_fields() {
        let i = parse_gitea_issue(&issue_json(12)).unwrap();
        assert_eq!(i.number, 12);
        assert_eq!(i.title, "Crash on startup");
        assert_eq!(i.body.as_deref(), Some("stack trace attached"));
        assert_eq!(i.state, "open");
        assert_eq!(i.author, "bob");
        assert_eq!(i.url, "https://codeberg.org/o/r/issues/12");
        assert_eq!(i.labels, vec!["bug".to_string(), "p1".to_string()]);
        assert_eq!(i.assignees, vec!["alice".to_string(), "carol".to_string()]);
        assert_eq!(i.created_at, "2026-02-03T04:05:06Z");
        assert_eq!(i.comments, 4);
    }

    #[test]
    fn parse_gitea_issue_defaults_when_optionals_missing() {
        let i = parse_gitea_issue(&serde_json::json!({ "number": 5, "title": "t" })).unwrap();
        assert_eq!(i.number, 5);
        assert_eq!(i.body, None);
        assert!(i.labels.is_empty());
        assert!(i.assignees.is_empty());
        assert_eq!(i.author, "");
        assert_eq!(i.comments, 0);
    }

    #[test]
    fn list_parses_issues_from_mocked_endpoint() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/repos/owner/repo/issues")
                .query_param("state", "open")
                .query_param("type", "issues")
                .query_param("limit", "50")
                .header("Authorization", "token test-token");
            then.status(200)
                .json_body(serde_json::json!([issue_json(1), issue_json(2)]));
        });
        let issues = client(&server).list("owner", "repo", "open").unwrap();
        mock.assert();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 1);
        assert_eq!(issues[1].number, 2);
    }

    #[test]
    fn comment_posts_body_with_token_auth() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v1/repos/owner/repo/issues/8/comments")
                .header("Authorization", "token test-token")
                .json_body(serde_json::json!({ "body": "looks good" }));
            then.status(201);
        });
        client(&server)
            .comment("owner", "repo", 8, "looks good")
            .unwrap();
        mock.assert();
    }

    #[test]
    fn list_maps_non_2xx_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/api/v1/repos/owner/repo/issues");
            then.status(403)
                .json_body(serde_json::json!({ "message": "forbidden" }));
        });
        let err = client(&server).list("owner", "repo", "open").unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 403, .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
