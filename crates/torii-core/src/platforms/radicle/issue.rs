//! Radicle — issue client.

use crate::error::{Result, ToriiError};
use crate::platforms::issue::*;
use serde_json::Value;

pub struct RadicleIssueClient;

impl RadicleIssueClient {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl IssueClient for RadicleIssueClient {
    fn list(&self, _o: &str, _r: &str, state: &str) -> Result<Vec<Issue>> {
        // `rad issue list --state open|closed|all`. We translate the
        // platform-agnostic "open" / "closed" / "all" into rad's
        // matching state names.
        let st = match state {
            "open" => "open",
            "closed" => "closed",
            _ => "all",
        };
        let json = crate::radicle::run_rad_json(&["issue", "list", "--state", st])?;
        let arr = json
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "radicle".into(),
                message: "rad issue list: expected array".into(),
            })?;
        Ok(arr
            .iter()
            .filter_map(|v| parse_radicle_issue(v).ok())
            .collect())
    }

    fn create(&self, _o: &str, _r: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let body = opts.body.unwrap_or_default();
        let stdout = crate::radicle::run_rad(&[
            "issue",
            "open",
            "--title",
            &opts.title,
            "--description",
            &body,
        ])?;
        // `rad issue open` prints the new issue id on the last line.
        let id = stdout
            .trim()
            .lines()
            .last()
            .unwrap_or("")
            .trim()
            .to_string();
        Ok(Issue {
            number: 0, // Radicle issues are identified by hash, not number
            title: opts.title,
            body: Some(body),
            state: "open".to_string(),
            author: String::new(),
            url: format!("rad:{}", id),
            labels: vec![],
            assignees: vec![],
            created_at: String::new(),
            comments: 0,
        })
    }

    fn close(&self, _o: &str, _r: &str, number: u64) -> Result<()> {
        // Radicle uses string ids, not numbers — torii's IssueClient
        // signature takes u64 so we can't address a real radicle issue
        // through this method. Surface a clear error pointing at the
        // CLI direct path.
        Err(ToriiError::Unsupported(format!(
            "Radicle issues are identified by hash, not number. `torii issue close {}` \
             cannot be mapped 1:1 — use `rad issue state <id> --closed` directly until \
             torii's IssueClient trait grows a string-id variant.",
            number
        )))
    }

    fn comment(&self, _o: &str, _r: &str, number: u64, _body: &str) -> Result<()> {
        Err(ToriiError::Unsupported(format!(
            "Radicle issues are identified by hash, not number. `torii issue comment {}` \
             can't address a hash-id issue — use `rad issue comment <id>` directly.",
            number
        )))
    }
}

fn parse_radicle_issue(v: &Value) -> Result<Issue> {
    let id = v["id"].as_str().unwrap_or("");
    Ok(Issue {
        number: 0,
        title: v["title"].as_str().unwrap_or("").to_string(),
        body: v["description"].as_str().map(String::from),
        state: v["state"]["status"].as_str().unwrap_or("open").to_string(),
        author: v["author"]["alias"]
            .as_str()
            .or_else(|| v["author"]["id"].as_str())
            .unwrap_or("")
            .to_string(),
        url: format!("rad:{}", id),
        labels: v["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|l| l.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        assignees: v["assignees"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|u| u.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        created_at: v["timestamp"].as_str().unwrap_or("").to_string(),
        comments: v["comments"].as_u64().unwrap_or(0),
    })
}

// ── Bitbucket Cloud (issues — deprecated but still works if enabled) ────────
//
// Bitbucket Cloud's issue tracker is technically deprecated in favour
// of third-party trackers (Jira), but the REST endpoint still works
// for repos that have issues enabled. On repos without issues
// enabled, the API returns 404 — we surface that with a hint.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_radicle_issue_full() {
        // Shape of one element from `rad issue list` JSON output.
        let v = serde_json::json!({
            "id": "deadbeefcafe",
            "title": "Tracker bug",
            "description": "Body text",
            "state": { "status": "closed" },
            "author": { "alias": "alice", "id": "did:key:z6MkAlice" },
            "labels": ["bug", "p1"],
            "assignees": ["did:key:z6MkBob"],
            "timestamp": "2026-01-01T00:00:00Z",
            "comments": 3u64,
        });
        let issue = parse_radicle_issue(&v).unwrap();
        assert_eq!(issue.number, 0); // radicle issues are hash-id'd, not numbered
        assert_eq!(issue.title, "Tracker bug");
        assert_eq!(issue.body.as_deref(), Some("Body text"));
        assert_eq!(issue.state, "closed");
        assert_eq!(issue.author, "alice");
        assert_eq!(issue.url, "rad:deadbeefcafe");
        assert_eq!(issue.labels, vec!["bug".to_string(), "p1".to_string()]);
        assert_eq!(issue.assignees, vec!["did:key:z6MkBob".to_string()]);
        assert_eq!(issue.created_at, "2026-01-01T00:00:00Z");
        assert_eq!(issue.comments, 3);
    }

    #[test]
    fn parse_radicle_issue_author_falls_back_to_did() {
        let v = serde_json::json!({ "author": { "id": "did:key:z6MkExample" } });
        assert_eq!(
            parse_radicle_issue(&v).unwrap().author,
            "did:key:z6MkExample"
        );
    }

    #[test]
    fn parse_radicle_issue_minimal_defaults() {
        let v = serde_json::json!({});
        let issue = parse_radicle_issue(&v).unwrap();
        assert_eq!(issue.number, 0);
        assert_eq!(issue.title, "");
        assert_eq!(issue.body, None);
        assert_eq!(issue.state, "open"); // missing state defaults to open
        assert_eq!(issue.author, "");
        assert_eq!(issue.url, "rad:");
        assert!(issue.labels.is_empty());
        assert!(issue.assignees.is_empty());
        assert_eq!(issue.created_at, "");
        assert_eq!(issue.comments, 0);
    }
}
