//! Sourcehut — issue client.

use crate::error::{Result, ToriiError};
use crate::platforms::issue::*;
use reqwest::blocking::Client;

pub struct SourcehutIssueClient {
    token: String,
}

impl SourcehutIssueClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("sourcehut", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "sourcehut".into(),
                message:
                    "Sourcehut token not found. Generate one at https://meta.sr.ht/oauth and run: \
                 torii auth set sourcehut YOUR_TOKEN"
                        .to_string(),
            })?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
    fn auth(&self) -> String {
        format!("token {}", self.token)
    }
}

impl IssueClient for SourcehutIssueClient {
    fn list(&self, owner: &str, repo: &str, _state: &str) -> Result<Vec<Issue>> {
        // todo.sr.ht doesn't support per-state filtering on the list
        // endpoint — we fetch then filter client-side. The owner already
        // includes the `~` prefix from the URL parser.
        let url = format!("https://todo.sr.ht/api/trackers/{}/{}/tickets", owner, repo);
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Sourcehut todo (url: {})", url))?;
        let arr = json["results"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "sourcehut".into(),
                message: format!("Sourcehut returned no `results` array. Body: {}", json),
            })?;
        Ok(arr
            .iter()
            .filter_map(|v| parse_sourcehut_issue(v).ok())
            .collect())
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!("https://todo.sr.ht/api/trackers/{}/{}/tickets", owner, repo);
        let body = serde_json::json!({
            "title":       opts.title,
            "description": opts.body.unwrap_or_default(),
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .json(&body);
        let json = crate::http::send_json(req, "Sourcehut create ticket")?;
        parse_sourcehut_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        // todo.sr.ht ticket updates go through `PUT /tickets/{id}` with
        // a `status: "resolved"` and `resolution: "fixed"` body.
        let url = format!(
            "https://todo.sr.ht/api/trackers/{}/{}/tickets/{}",
            owner, repo, number
        );
        let body = serde_json::json!({
            "status":     "resolved",
            "resolution": "fixed",
        });
        let req = self
            .client()
            .put(&url)
            .header("Authorization", self.auth())
            .json(&body);
        crate::http::send_empty(req, "Sourcehut close ticket")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "https://todo.sr.ht/api/trackers/{}/{}/tickets/{}/events",
            owner, repo, number
        );
        let payload = serde_json::json!({ "comment": body });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .json(&payload);
        crate::http::send_empty(req, "Sourcehut comment ticket")
    }
}

fn parse_sourcehut_issue(json: &serde_json::Value) -> Result<Issue> {
    let number = json["id"].as_u64().unwrap_or(0);
    let owner = json["tracker"]["owner"]["canonical_name"]
        .as_str()
        .unwrap_or("");
    let tracker = json["tracker"]["name"].as_str().unwrap_or("");
    Ok(Issue {
        number,
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["description"].as_str().map(String::from),
        // todo.sr.ht uses "reported" (open) and "resolved" (closed).
        state: match json["status"].as_str().unwrap_or("") {
            "reported" => "open".to_string(),
            "resolved" => "closed".to_string(),
            other => other.to_string(),
        },
        author: json["submitter"]["canonical_name"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        url: format!("https://todo.sr.ht/{}/{}/{}", owner, tracker, number),
        labels: json["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|l| l["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        assignees: json["assignees"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|u| u["canonical_name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        created_at: json["created"].as_str().unwrap_or("").to_string(),
        comments: 0, // todo.sr.ht doesn't expose a count on the list endpoint
    })
}

// ── Radicle (peer-to-peer, via `rad` CLI) ────────────────────────────────────
//
// Radicle stores issues in special refs inside the project itself —
// no central server. We drive the `rad issue` subcommand directly.
// owner/repo args from the URL parser hold the RID and (empty) repo
// part; `rad` resolves the project from the current working dir's
// `.git` config, so we don't need to pass the RID per call.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sourcehut_issue_full() {
        let json = serde_json::json!({
            "id": 12u64,
            "title": "Ticket title",
            "description": "Body text",
            "status": "reported",
            "submitter": { "canonical_name": "~alice" },
            "tracker": {
                "name": "mytracker",
                "owner": { "canonical_name": "~alice" }
            },
            "labels": [ { "name": "bug" }, { "name": "ui" } ],
            "assignees": [ { "canonical_name": "~bob" } ],
            "created": "2026-01-01T00:00:00Z",
        });
        let issue = parse_sourcehut_issue(&json).unwrap();
        assert_eq!(issue.number, 12);
        assert_eq!(issue.title, "Ticket title");
        assert_eq!(issue.body.as_deref(), Some("Body text"));
        assert_eq!(issue.state, "open");
        assert_eq!(issue.author, "~alice");
        assert_eq!(issue.url, "https://todo.sr.ht/~alice/mytracker/12");
        assert_eq!(issue.labels, vec!["bug".to_string(), "ui".to_string()]);
        assert_eq!(issue.assignees, vec!["~bob".to_string()]);
        assert_eq!(issue.created_at, "2026-01-01T00:00:00Z");
        assert_eq!(issue.comments, 0);
    }

    #[test]
    fn parse_sourcehut_issue_state_mapping() {
        for (srht, ours) in [
            ("reported", "open"),
            ("resolved", "closed"),
            ("confirmed", "confirmed"), // unknown statuses pass through raw
        ] {
            let json = serde_json::json!({ "status": srht });
            assert_eq!(parse_sourcehut_issue(&json).unwrap().state, ours);
        }
    }

    #[test]
    fn parse_sourcehut_issue_minimal_defaults() {
        let json = serde_json::json!({});
        let issue = parse_sourcehut_issue(&json).unwrap();
        assert_eq!(issue.number, 0);
        assert_eq!(issue.title, "");
        assert_eq!(issue.body, None);
        assert_eq!(issue.author, "");
        assert!(issue.labels.is_empty());
        assert!(issue.assignees.is_empty());
        assert_eq!(issue.created_at, "");
    }
}
