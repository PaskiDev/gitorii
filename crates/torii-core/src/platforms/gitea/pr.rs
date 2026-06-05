//! Gitea / Codeberg / Forgejo — pr client.

use crate::error::{Result, ToriiError};
use crate::platforms::pr::*;
use reqwest::blocking::Client;

pub struct GiteaPrClient {
    token: String,
    base_url: String,
}

impl GiteaPrClient {
    pub fn new() -> Result<Self> {
        Self::new_with_host(gitea_base_url())
    }

    pub fn new_with_host(base_url: &str) -> Result<Self> {
        let token = resolve_gitea_token()?;
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

impl PrClient for GiteaPrClient {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        let url = format!("{}/api/v1/repos/{}/{}/pulls", self.base_url, owner, repo);
        let mut title = opts.title.clone();
        // Gitea has no draft flag — convention is `WIP:` prefix.
        if opts.draft && !title.to_lowercase().starts_with("wip:") {
            title = format!("WIP: {}", title);
        }
        let body = serde_json::json!({
            "title": title,
            "body":  opts.body.unwrap_or_default(),
            "head":  opts.head,
            "base":  opts.base,
        });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .json(&body);
        let json = crate::http::send_json(req, "Gitea create PR")?;
        parse_gitea_pr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/pulls?state={}&limit=50",
            self.base_url, owner, repo, state
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Gitea (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_gitea_pr)
            .collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Gitea PR #{}", number))?;
        parse_gitea_pr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/pulls/{}/merge",
            self.base_url, owner, repo, number
        );
        let do_param = match method {
            MergeMethod::Merge => "merge",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        };
        let body = serde_json::json!({ "Do": do_param });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .json(&body);
        crate::http::send_empty(req, "Gitea merge PR")
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );
        let body = serde_json::json!({ "state": "closed" });
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .json(&body);
        crate::http::send_empty(req, "Gitea close PR")
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, number
        );
        let mut body = serde_json::Map::new();
        if let Some(t) = opts.title {
            body.insert("title".into(), serde_json::Value::String(t));
        }
        if let Some(b) = opts.body {
            body.insert("body".into(), serde_json::Value::String(b));
        }
        if let Some(base) = opts.base {
            body.insert("base".into(), serde_json::Value::String(base));
        }
        if body.is_empty() {
            return Ok(());
        }
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "Gitea update PR")
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/branches/{}",
            self.base_url, owner, repo, branch
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth());
        crate::http::send_empty(req, "Gitea delete branch")
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_gitea_pr(json: &serde_json::Value) -> Result<PullRequest> {
    Ok(PullRequest {
        number: json["number"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["body"].as_str().map(|s| s.to_string()),
        state: json["state"].as_str().unwrap_or("").to_string(),
        head: json["head"]["ref"].as_str().unwrap_or("").to_string(),
        base: json["base"]["ref"].as_str().unwrap_or("").to_string(),
        author: json["user"]["login"].as_str().unwrap_or("").to_string(),
        url: json["html_url"].as_str().unwrap_or("").to_string(),
        // Gitea convention: WIP: prefix marks drafts (no native flag pre-1.19).
        draft: json["title"]
            .as_str()
            .map(|s| {
                let l = s.to_lowercase();
                l.starts_with("wip:") || l.starts_with("[wip]") || l.starts_with("draft:")
            })
            .unwrap_or(false),
        mergeable: json["mergeable"].as_bool(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Sourcehut (paradigm mismatch — patches go through mailing lists)
// ============================================================================
//
// Sourcehut's contribution model is **email-based patches sent to
// `~user/repo-devel@lists.sr.ht`**, not a server-side merge-request
// object. There is no REST endpoint to "create a PR" the way GitHub
// or GitLab expose one — the patch lives on the mailing list, the
// maintainer applies it locally with `torii patch apply`, then pushes.
//
// We expose a stub client that returns a clear error explaining the
// workflow, so the four CLI commands (`torii pr list/create/view/...`)
// fail with guidance instead of silently working on a wrong endpoint.
// `torii patch export <range>` + mailing the resulting `.patch` files
// is the supported flow.

/// Map a "gitea" platform value to its base URL. Today this is always
/// `https://codeberg.org`; in 0.8.0 with `platforms.toml` support, the
/// caller will be able to resolve self-hosted instances per-remote.
///
/// Centralised here so adding a per-host map later only touches one site.
pub fn gitea_base_url() -> &'static str {
    "https://codeberg.org"
}

/// Resolve the Gitea / Codeberg / Forgejo token. The auth subsystem
/// accepts all three names as distinct providers (because users like
/// to call them by their brand), but the API surface is the same — so
/// we try all three in order and return the first one set.
///
/// Used by every Gitea* client (release / issue / pr / pipeline) so
/// `torii auth set codeberg YOUR_TOKEN` works without forcing the user
/// to learn that "the canonical provider is gitea".
pub fn resolve_gitea_token() -> Result<String> {
    for provider in ["gitea", "codeberg", "forgejo"] {
        if let Some(t) = crate::auth::resolve_token(provider, ".").value {
            return Ok(t);
        }
    }
    Err(ToriiError::Auth {
        provider: "gitea".into(),
        message:
            "Gitea / Codeberg / Forgejo token not found. Run: torii auth set codeberg YOUR_TOKEN"
                .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn client(server: &MockServer) -> GiteaPrClient {
        GiteaPrClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    fn pr_json(number: u64, title: &str) -> serde_json::Value {
        serde_json::json!({
            "number": number,
            "title": title,
            "body": "the body",
            "state": "open",
            "head": { "ref": "feature" },
            "base": { "ref": "main" },
            "user": { "login": "alice" },
            "html_url": "https://codeberg.org/o/r/pulls/1",
            "mergeable": true,
            "created_at": "2026-01-02T03:04:05Z",
        })
    }

    #[test]
    fn parse_gitea_pr_extracts_all_fields() {
        let pr = parse_gitea_pr(&pr_json(7, "Add feature")).unwrap();
        assert_eq!(pr.number, 7);
        assert_eq!(pr.title, "Add feature");
        assert_eq!(pr.body.as_deref(), Some("the body"));
        assert_eq!(pr.state, "open");
        assert_eq!(pr.head, "feature");
        assert_eq!(pr.base, "main");
        assert_eq!(pr.author, "alice");
        assert_eq!(pr.url, "https://codeberg.org/o/r/pulls/1");
        assert!(!pr.draft);
        assert_eq!(pr.mergeable, Some(true));
        assert_eq!(pr.created_at, "2026-01-02T03:04:05Z");
    }

    #[test]
    fn parse_gitea_pr_detects_drafts_from_title_conventions() {
        for t in ["WIP: thing", "wip: thing", "[WIP] thing", "Draft: thing"] {
            let pr = parse_gitea_pr(&pr_json(1, t)).unwrap();
            assert!(pr.draft, "title {t:?} should be detected as draft");
        }
        // "WIP" elsewhere in the title is not a draft marker.
        assert!(
            !parse_gitea_pr(&pr_json(1, "ship the WIP tracker"))
                .unwrap()
                .draft
        );
    }

    #[test]
    fn parse_gitea_pr_defaults_when_optionals_missing() {
        let pr = parse_gitea_pr(&serde_json::json!({ "number": 3 })).unwrap();
        assert_eq!(pr.number, 3);
        assert_eq!(pr.body, None);
        assert_eq!(pr.mergeable, None);
        assert_eq!(pr.title, "");
        assert_eq!(pr.head, "");
        assert_eq!(pr.base, "");
        assert_eq!(pr.author, "");
        assert!(!pr.draft);
    }

    #[test]
    fn list_parses_prs_from_mocked_endpoint() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/repos/owner/repo/pulls")
                .query_param("state", "open")
                .query_param("limit", "50")
                .header("Authorization", "token test-token");
            then.status(200).json_body(serde_json::json!([
                pr_json(1, "First"),
                pr_json(2, "WIP: Second")
            ]));
        });
        let prs = client(&server).list("owner", "repo", "open").unwrap();
        mock.assert();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].number, 1);
        assert_eq!(prs[0].title, "First");
        assert!(prs[1].draft);
    }

    #[test]
    fn create_prefixes_wip_for_draft_and_sends_token_auth() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v1/repos/owner/repo/pulls")
                .header("Authorization", "token test-token")
                .json_body(serde_json::json!({
                    "title": "WIP: Feature",
                    "body": "",
                    "head": "feature",
                    "base": "main",
                }));
            then.status(201).json_body(pr_json(9, "WIP: Feature"));
        });
        let pr = client(&server)
            .create(
                "owner",
                "repo",
                CreatePrOptions {
                    title: "Feature".into(),
                    body: None,
                    head: "feature".into(),
                    base: "main".into(),
                    draft: true,
                },
            )
            .unwrap();
        mock.assert();
        assert_eq!(pr.number, 9);
        assert!(pr.draft);
    }

    #[test]
    fn update_with_no_fields_is_a_noop_without_network() {
        // Port 1 has no listener — if update() sent a request this
        // would fail with a Network error instead of Ok(()).
        let c = GiteaPrClient {
            token: "test-token".into(),
            base_url: "http://127.0.0.1:1".into(),
        };
        let opts = UpdatePrOptions {
            title: None,
            body: None,
            base: None,
        };
        assert!(c.update("owner", "repo", 1, opts).is_ok());
    }

    #[test]
    fn get_maps_non_2xx_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/api/v1/repos/owner/repo/pulls/4");
            then.status(500)
                .json_body(serde_json::json!({ "message": "boom" }));
        });
        let err = client(&server).get("owner", "repo", 4).unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 500, .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
