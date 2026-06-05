//! Bitbucket Cloud — issue client.

use crate::error::{Result, ToriiError};
use crate::platforms::issue::*;
use reqwest::blocking::Client;

pub struct BitbucketIssueClient {
    token: String,
}

impl BitbucketIssueClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("bitbucket", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "bitbucket".into(),
                message: "Bitbucket token not found. Create an app password at \
                 https://bitbucket.org/account/settings/app-passwords/ \
                 and run: torii auth set bitbucket USERNAME:APP_PASSWORD"
                    .to_string(),
            })?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
    fn auth(&self) -> String {
        if self.token.contains(':') {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&self.token);
            format!("Basic {}", b64)
        } else {
            format!("Bearer {}", self.token)
        }
    }
}

impl IssueClient for BitbucketIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        // Bitbucket issue states: new/open/resolved/on hold/invalid/
        // duplicate/wontfix/closed. We collapse "open" to the two
        // active ones and "closed" to the terminal set. Q-syntax is
        // Bitbucket-specific (`state="new" OR state="open"`).
        let q = match state {
            "open"   => r#"state="new" OR state="open""#.to_string(),
            "closed" => r#"state="resolved" OR state="closed" OR state="invalid" OR state="duplicate" OR state="wontfix""#.to_string(),
            _        => String::new(),
        };
        let mut url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/issues?pagelen=50",
            owner, repo
        );
        if !q.is_empty() {
            url.push_str(&format!("&q={}", crate::url::encode(&q)));
        }
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, &format!("Bitbucket (url: {})", url))?;
        let arr = json["values"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "bitbucket".into(),
                message: format!(
                    "Bitbucket returned no `values` array — does the repo have issues enabled? \
                 Body: {}",
                    json
                ),
            })?;
        arr.iter().map(parse_bitbucket_issue).collect()
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/issues",
            owner, repo
        );
        let body = serde_json::json!({
            "title":   opts.title,
            "content": {
                "raw":    opts.body.unwrap_or_default(),
                "markup": "markdown",
            },
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        let json = crate::http::send_json(req, "Bitbucket create issue")?;
        parse_bitbucket_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/issues/{}",
            owner, repo, number
        );
        let body = serde_json::json!({ "state": "resolved" });
        let req = self
            .client()
            .put(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Bitbucket close issue")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/issues/{}/comments",
            owner, repo, number
        );
        let payload = serde_json::json!({
            "content": { "raw": body, "markup": "markdown" },
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&payload);
        crate::http::send_empty(req, "Bitbucket comment issue")
    }
}

fn parse_bitbucket_issue(json: &serde_json::Value) -> Result<Issue> {
    Ok(Issue {
        number: json["id"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["content"]["raw"].as_str().map(String::from),
        state: match json["state"].as_str().unwrap_or("") {
            "new" | "open" => "open".to_string(),
            "resolved" | "closed" | "invalid" | "duplicate" | "wontfix" => "closed".to_string(),
            other => other.to_string(),
        },
        author: json["reporter"]["display_name"]
            .as_str()
            .or_else(|| json["reporter"]["username"].as_str())
            .unwrap_or("")
            .to_string(),
        url: json["links"]["html"]["href"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        labels: json["kind"]
            .as_str()
            .map(|k| vec![k.to_string()])
            .unwrap_or_default(),
        assignees: json["assignee"]["display_name"]
            .as_str()
            .or_else(|| json["assignee"]["username"].as_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
        created_at: json["created_on"].as_str().unwrap_or("").to_string(),
        comments: 0,
    })
}

// ── Azure DevOps (Work Items via WIQL) ──────────────────────────────────────
//
// Azure's "issues" are Work Items. The list endpoint takes a WIQL
// (Work Item Query Language, SQL-like) query that returns IDs, then a
// second call fetches the full records by id. We bundle that into a
// single torii `list` call.
//
// Work item types depend on the project's process template (Agile /
// Scrum / Basic / CMMI). The Basic process uses `Issue`; Agile uses
// `User Story` and `Bug`. We default to `Issue` for create — projects
// on a non-Basic process will need to extend the create flow later.

// The client's URLs are hardcoded to api.bitbucket.org, so only the
// parsing layer is testable without touching production code.
#[cfg(test)]
mod tests {
    use super::*;

    fn issue_json(id: u64, state: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "title": "Crash on save",
            "content": { "raw": "steps to reproduce", "markup": "markdown" },
            "state": state,
            "kind": "bug",
            "reporter": { "display_name": "Bob Smith", "username": "bob" },
            "assignee": { "display_name": "Alice Doe", "username": "alice" },
            "links": { "html": { "href": "https://bitbucket.org/w/r/issues/14" } },
            "created_on": "2026-05-06T07:08:09.000000+00:00",
        })
    }

    #[test]
    fn parse_bitbucket_issue_extracts_all_fields() {
        let i = parse_bitbucket_issue(&issue_json(14, "new")).unwrap();
        assert_eq!(i.number, 14);
        assert_eq!(i.title, "Crash on save");
        assert_eq!(i.body.as_deref(), Some("steps to reproduce"));
        assert_eq!(i.state, "open");
        assert_eq!(i.author, "Bob Smith");
        assert_eq!(i.url, "https://bitbucket.org/w/r/issues/14");
        // `kind` becomes the single label.
        assert_eq!(i.labels, vec!["bug".to_string()]);
        assert_eq!(i.assignees, vec!["Alice Doe".to_string()]);
        assert_eq!(i.created_at, "2026-05-06T07:08:09.000000+00:00");
        assert_eq!(i.comments, 0);
    }

    #[test]
    fn parse_bitbucket_issue_collapses_states() {
        for s in ["new", "open"] {
            assert_eq!(
                parse_bitbucket_issue(&issue_json(1, s)).unwrap().state,
                "open"
            );
        }
        for s in ["resolved", "closed", "invalid", "duplicate", "wontfix"] {
            assert_eq!(
                parse_bitbucket_issue(&issue_json(1, s)).unwrap().state,
                "closed"
            );
        }
        // States outside both sets pass through verbatim.
        assert_eq!(
            parse_bitbucket_issue(&issue_json(1, "on hold"))
                .unwrap()
                .state,
            "on hold"
        );
    }

    #[test]
    fn parse_bitbucket_issue_defaults_when_optionals_missing() {
        let json = serde_json::json!({
            "id": 2,
            "title": "t",
            "state": "new",
            "reporter": { "username": "bob" },
        });
        let i = parse_bitbucket_issue(&json).unwrap();
        assert_eq!(i.body, None);
        // Falls back to username when display_name is absent.
        assert_eq!(i.author, "bob");
        assert!(i.labels.is_empty());
        assert!(i.assignees.is_empty());
        assert_eq!(i.url, "");
        assert_eq!(i.created_at, "");
    }

    #[test]
    fn parses_issues_out_of_paginated_values_envelope() {
        // `list` reads Bitbucket's `{"values": [...]}` page shape.
        let page = serde_json::json!({
            "pagelen": 50,
            "size": 2,
            "values": [issue_json(1, "new"), issue_json(2, "wontfix")],
        });
        let issues: Vec<Issue> = page["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| parse_bitbucket_issue(v).unwrap())
            .collect();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].state, "open");
        assert_eq!(issues[1].state, "closed");
    }
}
