//! GitHub — runner client.

use crate::error::{Result, ToriiError};
use crate::platforms::runner::*;
use reqwest::blocking::Client;

pub struct GitHubRunnerClient {
    token: String,
    base_url: String,
}

impl GitHubRunnerClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("github", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "github".into(),
                message: "GitHub token not found. Run: torii auth oauth github".to_string(),
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
    fn accept(&self) -> &'static str {
        "application/vnd.github+json"
    }
}

impl RunnerClient for GitHubRunnerClient {
    fn list(&self, owner: &str, repo: &str) -> Result<Vec<Runner>> {
        let url = format!(
            "{}/repos/{}/{}/actions/runners?per_page=100",
            self.base_url, owner, repo
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", self.accept());
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        let arr = json["runners"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "github".into(),
                message: format!("GitHub returned no `runners` array: {}", json),
            })?;
        arr.iter().map(parse_github_runner).collect()
    }

    fn show(&self, owner: &str, repo: &str, id: &str) -> Result<Runner> {
        let url = format!(
            "{}/repos/{}/{}/actions/runners/{}",
            self.base_url, owner, repo, id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", self.accept());
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        parse_github_runner(&json)
    }

    fn remove(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/actions/runners/{}",
            self.base_url, owner, repo, id
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", self.accept());
        crate::http::send_empty(req, "GitHub delete runner")
    }

    fn reset_token(&self, _owner: &str, _repo: &str, _id: &str) -> Result<String> {
        Err(ToriiError::Unsupported(
            "GitHub Actions doesn't expose a per-runner token reset. \
             Re-register the runner: stop the agent, fetch a fresh \
             registration token from `Settings → Actions → Runners`, \
             and run `./config.sh remove` then `./config.sh` again."
                .to_string(),
        ))
    }

    fn pause(&self, _owner: &str, _repo: &str, _id: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "GitHub Actions has no pause/resume on self-hosted runners. \
             Use a workflow `runs-on:` label that the runner doesn't \
             advertise, or stop the agent on the host."
                .to_string(),
        ))
    }
    fn resume(&self, _owner: &str, _repo: &str, _id: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "GitHub Actions has no pause/resume on self-hosted runners.".to_string(),
        ))
    }

    fn registration_token(&self, owner: &str, repo: &str) -> Result<RegistrationToken> {
        // GitHub Actions: `POST /repos/:owner/:repo/actions/runners/registration-token`
        // returns a token valid for ~1h. The token is single-use per
        // registration but you can request new ones freely.
        let url = format!(
            "{}/repos/{}/{}/actions/runners/registration-token",
            self.base_url, owner, repo
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", self.accept());
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        let token = json["token"]
            .as_str()
            .ok_or_else(|| ToriiError::Auth {
                provider: "github".into(),
                message: format!(
                    "GitHub registration-token response missing `token`: {}",
                    json
                ),
            })?
            .to_string();
        // `expires_at` is RFC3339; we don't parse it here, we just
        // mark "an hour" because that's the documented default.
        Ok(RegistrationToken {
            token,
            register_url: format!("https://github.com/{}/{}", owner, repo),
            expires_in_seconds: Some(3600),
        })
    }
}

fn parse_github_runner(v: &serde_json::Value) -> Result<Runner> {
    let id = v["id"]
        .as_u64()
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: format!("GitHub runner has no `id`: {}", v),
        })?
        .to_string();
    let busy = v["busy"].as_bool().unwrap_or(false);
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let status = if raw_status == "online" && busy {
        "active".to_string()
    } else {
        raw_status
    };

    let tags = v["labels"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(Runner {
        id,
        description: v["name"].as_str().unwrap_or("").to_string(),
        status,
        paused: false,
        ip_address: String::new(),
        os: v["os"].as_str().unwrap_or("").to_string(),
        tags,
        version: String::new(),
        runner_type: "self-hosted".to_string(),
        web_url: String::new(),
    })
}

// ============================================================================
// Factory
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn client_for(server: &MockServer) -> GitHubRunnerClient {
        GitHubRunnerClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn parse_github_runner_maps_fields_and_busy_online_to_active() {
        let json = serde_json::json!({
            "id": 8u64,
            "name": "runner-01",
            "os": "linux",
            "status": "online",
            "busy": true,
            "labels": [{ "name": "self-hosted" }, { "name": "x64" }],
        });
        let r = parse_github_runner(&json).unwrap();
        assert_eq!(r.id, "8");
        assert_eq!(r.description, "runner-01");
        assert_eq!(r.os, "linux");
        assert_eq!(r.status, "active");
        assert_eq!(r.tags, vec!["self-hosted".to_string(), "x64".to_string()]);
        assert_eq!(r.runner_type, "self-hosted");
        assert!(!r.paused);
    }

    #[test]
    fn parse_github_runner_idle_online_and_missing_optionals() {
        let json = serde_json::json!({ "id": 9u64, "status": "online" });
        let r = parse_github_runner(&json).unwrap();
        // not busy → stays "online", no "active" promotion
        assert_eq!(r.status, "online");
        assert_eq!(r.description, "");
        assert_eq!(r.os, "");
        assert!(r.tags.is_empty());
    }

    #[test]
    fn parse_github_runner_missing_id_is_malformed() {
        let err = parse_github_runner(&serde_json::json!({ "name": "x" })).unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { .. }),
            "expected MalformedResponse, got: {err:?}"
        );
    }

    #[test]
    fn list_parses_runners_array() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/octo/demo/actions/runners")
                .query_param("per_page", "100")
                .header("Authorization", "token test-token");
            then.status(200).json_body(serde_json::json!({
                "runners": [
                    { "id": 1u64, "name": "a", "os": "linux", "status": "online", "busy": false },
                    { "id": 2u64, "name": "b", "os": "macos", "status": "offline", "busy": false },
                ]
            }));
        });
        let runners = client_for(&server).list("octo", "demo").unwrap();
        m.assert();
        assert_eq!(runners.len(), 2);
        assert_eq!(runners[0].id, "1");
        assert_eq!(runners[0].status, "online");
        assert_eq!(runners[1].description, "b");
        assert_eq!(runners[1].status, "offline");
    }

    #[test]
    fn remove_deletes_runner_with_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(DELETE)
                .path("/repos/octo/demo/actions/runners/12")
                .header("Authorization", "token test-token");
            then.status(204);
        });
        client_for(&server).remove("octo", "demo", "12").unwrap();
        m.assert();
    }

    #[test]
    fn registration_token_posts_and_maps_response() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/repos/octo/demo/actions/runners/registration-token")
                .header("Authorization", "token test-token");
            then.status(201).json_body(serde_json::json!({
                "token": "AAAREG123",
                "expires_at": "2026-06-05T13:00:00Z",
            }));
        });
        let reg = client_for(&server)
            .registration_token("octo", "demo")
            .unwrap();
        m.assert();
        assert_eq!(reg.token, "AAAREG123");
        assert_eq!(reg.register_url, "https://github.com/octo/demo");
        assert_eq!(reg.expires_in_seconds, Some(3600));
    }

    #[test]
    fn show_maps_404_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/repos/octo/demo/actions/runners/77");
            then.status(404)
                .json_body(serde_json::json!({ "message": "Not Found" }));
        });
        let err = client_for(&server).show("octo", "demo", "77").unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 404, .. }),
            "expected PlatformApi 404, got: {err:?}"
        );
    }
}
