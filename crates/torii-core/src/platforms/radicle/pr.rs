//! Radicle — pr client.

use crate::error::{Result, ToriiError};
use crate::platforms::pr::*;

pub struct RadiclePrClient;

impl RadiclePrClient {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl PrClient for RadiclePrClient {
    fn create(&self, _o: &str, _r: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        // `rad patch open` creates a patch from the current branch
        // against the project's default branch. We pass title +
        // description; head/base are picked up from the current
        // checkout.
        let body = opts.body.unwrap_or_default();
        let stdout = crate::radicle::run_rad(&[
            "patch",
            "open",
            "--message",
            &opts.title,
            "--message",
            &body,
        ])?;
        let id = stdout
            .trim()
            .lines()
            .last()
            .unwrap_or("")
            .trim()
            .to_string();
        Ok(PullRequest {
            number: 0,
            title: opts.title,
            body: Some(body),
            state: "open".to_string(),
            head: opts.head,
            base: opts.base,
            author: String::new(),
            url: format!("rad:{}", id),
            draft: opts.draft,
            mergeable: None,
            created_at: String::new(),
        })
    }

    fn list(&self, _o: &str, _r: &str, state: &str) -> Result<Vec<PullRequest>> {
        let st = match state {
            "open" => "open",
            "closed" => "archived",
            "merged" => "merged",
            _ => "all",
        };
        let json = crate::radicle::run_rad_json(&["patch", "list", "--state", st])?;
        let arr = json
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "radicle".into(),
                message: "rad patch list: expected array".into(),
            })?;
        Ok(arr
            .iter()
            .filter_map(|v| parse_radicle_patch(v).ok())
            .collect())
    }

    fn get(&self, _o: &str, _r: &str, _number: u64) -> Result<PullRequest> {
        Err(ToriiError::Unsupported(
            "Radicle patches are identified by hash, not number. Use \
             `rad patch show <id>` directly until torii's PrClient trait \
             grows a string-id variant."
                .to_string(),
        ))
    }

    fn merge(&self, _o: &str, _r: &str, _number: u64, _method: MergeMethod) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Radicle patches merge through `rad patch merge <id>` directly. \
             The CLI's numeric merge surface doesn't apply."
                .to_string(),
        ))
    }

    fn close(&self, _o: &str, _r: &str, _number: u64) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Radicle uses `rad patch archive <id>` (by hash) to close a patch.".to_string(),
        ))
    }

    fn update(&self, _o: &str, _r: &str, _number: u64, _opts: UpdatePrOptions) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Radicle patches are updated by pushing a new revision \
             (`git push rad HEAD:refs/patches/<id>`). Use the CLI directly."
                .to_string(),
        ))
    }

    fn delete_branch(&self, _o: &str, _r: &str, _b: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Radicle patches don't have branches in the github sense; revisions live in COB refs."
                .to_string(),
        ))
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_radicle_patch(v: &serde_json::Value) -> Result<PullRequest> {
    let id = v["id"].as_str().unwrap_or("");
    Ok(PullRequest {
        number: 0,
        title: v["title"].as_str().unwrap_or("").to_string(),
        body: v["description"].as_str().map(String::from),
        state: v["state"]["status"].as_str().unwrap_or("open").to_string(),
        head: v["head"].as_str().unwrap_or("").to_string(),
        base: v["base"].as_str().unwrap_or("").to_string(),
        author: v["author"]["alias"]
            .as_str()
            .or_else(|| v["author"]["id"].as_str())
            .unwrap_or("")
            .to_string(),
        url: format!("rad:{}", id),
        draft: v["draft"].as_bool().unwrap_or(false),
        mergeable: None,
        created_at: v["timestamp"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Bitbucket Cloud
// ============================================================================
//
// Bitbucket Cloud's REST v2 at `api.bitbucket.org/2.0`. Auth is Basic
// with `user:app_password`; if the user stores the token without the
// `:` we treat it as a Bearer (OAuth) token instead. The terminology:
//   workspace ≈ owner   (the org / user slug)
//   repo_slug ≈ repo    (the project slug)
//   state strings are UPPERCASE: OPEN / MERGED / DECLINED / SUPERSEDED
// Pages come wrapped in `{ values: [...], pagelen, next }` — we read
// just the first page (50 entries) like the other clients do.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_radicle_patch_full() {
        // Shape of one element from `rad patch list` JSON output.
        let v = serde_json::json!({
            "id": "abc123def456",
            "title": "Fix sync",
            "description": "Patch body",
            "state": { "status": "merged" },
            "head": "deadbeef",
            "base": "cafebabe",
            "author": { "alias": "alice", "id": "did:key:z6MkAlice" },
            "draft": true,
            "timestamp": "2026-01-01T00:00:00Z",
        });
        let pr = parse_radicle_patch(&v).unwrap();
        assert_eq!(pr.number, 0); // radicle patches are hash-id'd, not numbered
        assert_eq!(pr.title, "Fix sync");
        assert_eq!(pr.body.as_deref(), Some("Patch body"));
        assert_eq!(pr.state, "merged");
        assert_eq!(pr.head, "deadbeef");
        assert_eq!(pr.base, "cafebabe");
        assert_eq!(pr.author, "alice");
        assert_eq!(pr.url, "rad:abc123def456");
        assert!(pr.draft);
        assert_eq!(pr.mergeable, None);
        assert_eq!(pr.created_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn parse_radicle_patch_author_falls_back_to_did() {
        // Peers without an alias only expose their DID.
        let v = serde_json::json!({ "author": { "id": "did:key:z6MkExample" } });
        assert_eq!(
            parse_radicle_patch(&v).unwrap().author,
            "did:key:z6MkExample"
        );
    }

    #[test]
    fn parse_radicle_patch_minimal_defaults() {
        let v = serde_json::json!({});
        let pr = parse_radicle_patch(&v).unwrap();
        assert_eq!(pr.title, "");
        assert_eq!(pr.body, None);
        assert_eq!(pr.state, "open"); // missing state defaults to open
        assert_eq!(pr.head, "");
        assert_eq!(pr.base, "");
        assert_eq!(pr.author, "");
        assert_eq!(pr.url, "rad:");
        assert!(!pr.draft);
        assert_eq!(pr.created_at, "");
    }
}
