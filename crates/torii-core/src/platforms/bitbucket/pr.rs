//! Bitbucket Cloud — pr client.

use crate::error::{Result, ToriiError};
use crate::platforms::pr::*;
use reqwest::blocking::Client;

pub struct BitbucketPrClient {
    token: String,
}

impl BitbucketPrClient {
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

    /// Bitbucket accepts either `Basic base64(user:apppwd)` for app
    /// passwords or `Bearer <oauth>` for OAuth tokens. Heuristic: if the
    /// stored value contains `:`, treat it as `user:pass`; otherwise
    /// pass it through as a bearer token.
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

/// Translate torii's normalised state (`open`/`closed`/`merged`/`all`)
/// into Bitbucket's uppercase enum. `closed` maps to DECLINED because
/// MERGED is a distinct state on Bitbucket.
fn bitbucket_state(state: &str) -> &'static str {
    match state {
        "open" => "OPEN",
        "closed" => "DECLINED",
        "merged" => "MERGED",
        _ => "OPEN",
    }
}

impl PrClient for BitbucketPrClient {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pullrequests",
            owner, repo
        );
        let body = serde_json::json!({
            "title":       opts.title,
            "description": opts.body.unwrap_or_default(),
            "source":      { "branch": { "name": opts.head } },
            "destination": { "branch": { "name": opts.base } },
            "draft":       opts.draft,
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        let json = crate::http::send_json(req, "Bitbucket create PR")?;
        parse_bitbucket_pr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pullrequests?state={}&pagelen=50",
            owner,
            repo,
            bitbucket_state(state)
        );
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
                message: format!("Bitbucket returned no `values` array. Body: {}", json),
            })?;
        arr.iter().map(parse_bitbucket_pr).collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pullrequests/{}",
            owner, repo, number
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, &format!("Bitbucket PR #{}", number))?;
        parse_bitbucket_pr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pullrequests/{}/merge",
            owner, repo, number
        );
        let strategy = match method {
            MergeMethod::Merge => "merge_commit",
            MergeMethod::Squash => "squash",
            // Bitbucket's `fast_forward` is the closest analog to git rebase
            // for a PR merge — it preserves linear history.
            MergeMethod::Rebase => "fast_forward",
        };
        let body = serde_json::json!({ "merge_strategy": strategy });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Bitbucket merge PR")
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        // Bitbucket closes PRs by "declining" them.
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pullrequests/{}/decline",
            owner, repo, number
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        crate::http::send_empty(req, "Bitbucket decline PR")
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pullrequests/{}",
            owner, repo, number
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
                "destination".into(),
                serde_json::json!({ "branch": { "name": base } }),
            );
        }
        if body.is_empty() {
            return Ok(());
        }
        let req = self
            .client()
            .put(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "Bitbucket update PR")
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/refs/branches/{}",
            owner, repo, branch
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        crate::http::send_empty(req, "Bitbucket delete branch")
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_bitbucket_pr(json: &serde_json::Value) -> Result<PullRequest> {
    Ok(PullRequest {
        number: json["id"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["description"].as_str().map(String::from),
        // Normalise back to lowercase to match the rest of torii.
        state: match json["state"].as_str().unwrap_or("") {
            "OPEN" => "open".to_string(),
            "MERGED" => "merged".to_string(),
            "DECLINED" => "closed".to_string(),
            other => other.to_lowercase(),
        },
        head: json["source"]["branch"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        base: json["destination"]["branch"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        author: json["author"]["display_name"]
            .as_str()
            .or_else(|| json["author"]["username"].as_str())
            .unwrap_or("")
            .to_string(),
        url: json["links"]["html"]["href"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        draft: json["draft"].as_bool().unwrap_or(false),
        mergeable: None, // Bitbucket doesn't surface a mergeable flag on the list endpoint.
        created_at: json["created_on"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Azure DevOps Repos
// ============================================================================
//
// Azure DevOps uses a 3-level path (`org/project/repo`). The URL
// parser packs `org/project` into the `owner` slot and the repo name
// into `repo`; [`split_azure_owner`] unpacks them.
//
// Auth: a Personal Access Token (PAT) sent as Basic auth with an
// empty username — i.e. `Authorization: Basic base64(":PAT")`.
//
// Every call needs an `api-version` query parameter; we use `7.0` as
// the GA baseline. Newer endpoints may require `7.1-preview`; we
// stick to 7.0 for the surface we expose.

// The client's URLs are hardcoded to api.bitbucket.org, so only the
// parsing layer is testable without touching production code.
#[cfg(test)]
mod tests {
    use super::*;

    fn pr_json(id: u64, state: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "title": "Add login flow",
            "description": "implements OAuth",
            "state": state,
            "source": { "branch": { "name": "feature/login" } },
            "destination": { "branch": { "name": "main" } },
            "author": { "display_name": "Alice Doe", "username": "alice" },
            "links": { "html": { "href": "https://bitbucket.org/w/r/pull-requests/3" } },
            "draft": true,
            "created_on": "2026-04-05T06:07:08.123456+00:00",
        })
    }

    #[test]
    fn parse_bitbucket_pr_extracts_all_fields() {
        let pr = parse_bitbucket_pr(&pr_json(3, "OPEN")).unwrap();
        assert_eq!(pr.number, 3);
        assert_eq!(pr.title, "Add login flow");
        assert_eq!(pr.body.as_deref(), Some("implements OAuth"));
        assert_eq!(pr.state, "open");
        assert_eq!(pr.head, "feature/login");
        assert_eq!(pr.base, "main");
        // display_name wins over username when both are present.
        assert_eq!(pr.author, "Alice Doe");
        assert_eq!(pr.url, "https://bitbucket.org/w/r/pull-requests/3");
        assert!(pr.draft);
        assert_eq!(pr.mergeable, None);
        assert_eq!(pr.created_at, "2026-04-05T06:07:08.123456+00:00");
    }

    #[test]
    fn parse_bitbucket_pr_normalizes_states_to_lowercase() {
        assert_eq!(
            parse_bitbucket_pr(&pr_json(1, "OPEN")).unwrap().state,
            "open"
        );
        assert_eq!(
            parse_bitbucket_pr(&pr_json(1, "MERGED")).unwrap().state,
            "merged"
        );
        assert_eq!(
            parse_bitbucket_pr(&pr_json(1, "DECLINED")).unwrap().state,
            "closed"
        );
        // Unknown states pass through lowercased.
        assert_eq!(
            parse_bitbucket_pr(&pr_json(1, "SUPERSEDED")).unwrap().state,
            "superseded"
        );
    }

    #[test]
    fn parse_bitbucket_pr_defaults_when_optionals_missing() {
        let json = serde_json::json!({
            "id": 9,
            "title": "t",
            "state": "OPEN",
            "author": { "username": "bob" },
        });
        let pr = parse_bitbucket_pr(&json).unwrap();
        assert_eq!(pr.body, None);
        // Falls back to username when display_name is absent.
        assert_eq!(pr.author, "bob");
        assert!(!pr.draft);
        assert_eq!(pr.head, "");
        assert_eq!(pr.base, "");
        assert_eq!(pr.url, "");
        assert_eq!(pr.created_at, "");
    }

    #[test]
    fn parses_prs_out_of_paginated_values_envelope() {
        // `list` reads Bitbucket's `{"values": [...]}` page shape.
        let page = serde_json::json!({
            "pagelen": 50,
            "size": 2,
            "values": [pr_json(1, "OPEN"), pr_json(2, "MERGED")],
        });
        let prs: Vec<PullRequest> = page["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| parse_bitbucket_pr(v).unwrap())
            .collect();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].number, 1);
        assert_eq!(prs[1].state, "merged");
    }

    #[test]
    fn bitbucket_state_maps_normalized_states_to_uppercase_enum() {
        assert_eq!(bitbucket_state("open"), "OPEN");
        assert_eq!(bitbucket_state("closed"), "DECLINED");
        assert_eq!(bitbucket_state("merged"), "MERGED");
        assert_eq!(bitbucket_state("anything-else"), "OPEN");
    }
}
