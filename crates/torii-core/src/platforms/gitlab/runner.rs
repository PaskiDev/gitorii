//! GitLab — runner client.

use crate::error::{Result, ToriiError};
use crate::platforms::runner::*;
use reqwest::blocking::Client;

pub struct GitLabRunnerClient {
    token: String,
    pub(crate) base_url: String,
}

impl GitLabRunnerClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("gitlab", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "gitlab".into(),
                message: "GitLab token not found. Run: torii auth oauth gitlab".to_string(),
            })?;
        Ok(Self {
            token,
            base_url: "https://gitlab.com/api/v4".to_string(),
        })
    }

    pub(crate) fn client(&self) -> Client {
        crate::http::make_client()
    }
    pub(crate) fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    fn project_path(owner: &str, repo: &str) -> String {
        crate::url::encode(&format!("{}/{}", owner, repo))
    }
}

impl RunnerClient for GitLabRunnerClient {
    fn list(&self, owner: &str, repo: &str) -> Result<Vec<Runner>> {
        let url = format!(
            "{}/projects/{}/runners?per_page=100",
            self.base_url,
            Self::project_path(owner, repo)
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_gitlab_runner)
            .collect()
    }

    fn show(&self, _owner: &str, _repo: &str, id: &str) -> Result<Runner> {
        let url = format!("{}/runners/{}", self.base_url, id);
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        parse_gitlab_runner(&json)
    }

    fn remove(&self, _owner: &str, _repo: &str, id: &str) -> Result<()> {
        let url = format!("{}/runners/{}", self.base_url, id);
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth());
        crate::http::send_empty(req, "GitLab delete runner")
    }

    fn reset_token(&self, _owner: &str, _repo: &str, id: &str) -> Result<String> {
        let url = format!(
            "{}/runners/{}/reset_authentication_token",
            self.base_url, id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth());
        let json = crate::http::send_json(req, "GitLab reset runner token")?;
        Ok(json["token"]
            .as_str()
            .ok_or_else(|| ToriiError::Auth {
                provider: "gitlab".into(),
                message: format!(
                    "GitLab returned no `token` field in reset response: {}",
                    json
                ),
            })?
            .to_string())
    }

    fn pause(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        set_paused(self, owner, repo, id, true)
    }
    fn resume(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        set_paused(self, owner, repo, id, false)
    }

    fn registration_token(&self, owner: &str, repo: &str) -> Result<RegistrationToken> {
        // GitLab returns the project's `runners_token` as part of the
        // project payload. Requires Maintainer+ on the project. The
        // token doesn't expire on its own (only when explicitly reset
        // from the project settings).
        let url = format!(
            "{}/projects/{}",
            self.base_url,
            Self::project_path(owner, repo)
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        let token = json["runners_token"]
            .as_str()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "gitlab".into(),
                message: format!(
                    "GitLab project response missing `runners_token`. \
                 The token API needs Maintainer+ on the project. Body: {}",
                    json
                ),
            })?
            .to_string();
        Ok(RegistrationToken {
            token,
            register_url: "https://gitlab.com".to_string(),
            expires_in_seconds: None,
        })
    }
}

fn parse_gitlab_runner(v: &serde_json::Value) -> Result<Runner> {
    let id = v["id"]
        .as_u64()
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "gitlab".into(),
            message: format!("GitLab runner has no `id`: {}", v),
        })?
        .to_string();
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let paused = v["paused"].as_bool().unwrap_or(false);
    let status = if paused {
        "paused".to_string()
    } else {
        raw_status
    };

    let tags = v["tag_list"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(Runner {
        id,
        description: v["description"].as_str().unwrap_or("").to_string(),
        status,
        paused,
        ip_address: v["ip_address"].as_str().unwrap_or("").to_string(),
        os: v["platform"].as_str().unwrap_or("").to_string(),
        tags,
        version: v["version"].as_str().unwrap_or("").to_string(),
        runner_type: v["runner_type"].as_str().unwrap_or("").to_string(),
        web_url: v["web_url"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// GitHub Actions (self-hosted)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    // ── parser ───────────────────────────────────────────────────────────

    #[test]
    fn parse_gitlab_runner_full() {
        let json = serde_json::json!({
            "id": 77u64,
            "description": "shared-runner-1",
            "status": "online",
            "paused": false,
            "ip_address": "10.0.0.5",
            "platform": "linux",
            "tag_list": ["docker", "amd64"],
            "version": "17.1.0",
            "runner_type": "project_type",
            "web_url": "https://gitlab.com/acme/widget/-/runners/77"
        });
        let r = parse_gitlab_runner(&json).unwrap();
        assert_eq!(r.id, "77");
        assert_eq!(r.description, "shared-runner-1");
        assert_eq!(r.status, "online");
        assert!(!r.paused);
        assert_eq!(r.ip_address, "10.0.0.5");
        assert_eq!(r.os, "linux");
        assert_eq!(r.tags, vec!["docker", "amd64"]);
        assert_eq!(r.version, "17.1.0");
        assert_eq!(r.runner_type, "project_type");
        assert_eq!(r.web_url, "https://gitlab.com/acme/widget/-/runners/77");
    }

    #[test]
    fn parse_gitlab_runner_paused_overrides_status() {
        let json = serde_json::json!({ "id": 1u64, "status": "online", "paused": true });
        let r = parse_gitlab_runner(&json).unwrap();
        assert_eq!(r.status, "paused");
        assert!(r.paused);
        assert!(r.tags.is_empty());
    }

    #[test]
    fn parse_gitlab_runner_missing_id_is_malformed_response() {
        let err = parse_gitlab_runner(&serde_json::json!({ "status": "online" })).unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { .. }),
            "expected MalformedResponse, got: {err:?}"
        );
    }

    // ── client (httpmock) ────────────────────────────────────────────────

    fn client(server: &MockServer) -> GitLabRunnerClient {
        GitLabRunnerClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn list_parses_runners() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/runners")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([
                { "id": 77u64, "description": "r1", "status": "online", "paused": false },
                { "id": 78u64, "description": "r2", "status": "offline", "paused": true }
            ]));
        });
        let runners = client(&server).list("acme", "widget").unwrap();
        m.assert();
        assert_eq!(runners.len(), 2);
        assert_eq!(runners[0].id, "77");
        assert_eq!(runners[1].status, "paused");
    }

    #[test]
    fn pause_puts_paused_true_with_bearer_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(PUT)
                .path("/runners/77")
                .header("Authorization", "Bearer test-token")
                .json_body(serde_json::json!({ "paused": true }));
            then.status(200);
        });
        client(&server).pause("acme", "widget", "77").unwrap();
        m.assert();
    }

    #[test]
    fn reset_token_returns_new_token() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/runners/77/reset_authentication_token")
                .header("Authorization", "Bearer test-token");
            then.status(201)
                .json_body(serde_json::json!({ "token": "glrt-new-token" }));
        });
        let token = client(&server).reset_token("acme", "widget", "77").unwrap();
        assert_eq!(token, "glrt-new-token");
    }

    #[test]
    fn registration_token_missing_field_is_malformed_response() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/projects/acme%2Fwidget");
            then.status(200)
                .json_body(serde_json::json!({ "id": 1u64 }));
        });
        let err = client(&server)
            .registration_token("acme", "widget")
            .unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { .. }),
            "expected MalformedResponse, got: {err:?}"
        );
    }

    #[test]
    fn show_non_2xx_maps_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/runners/404");
            then.status(404)
                .json_body(serde_json::json!({ "message": "404 Not Found" }));
        });
        let err = client(&server).show("acme", "widget", "404").unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
