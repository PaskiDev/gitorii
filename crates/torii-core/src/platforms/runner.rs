//! CI runners surface — list / show / remove / reset-token / pause / resume.
//!
//! GitLab exposes all six operations on shared/group/project runners.
//! GitHub Actions exposes only list/show/remove on self-hosted runners
//! (gating is done via labels, and self-hosted tokens are rotated
//! manually on the runner host). Other platforms aren't covered here
//! yet — calling `get_runner_client` on them returns `Unsupported`.

use super::github::GitHubRunnerClient;
use super::gitlab::GitLabRunnerClient;
use crate::error::{Result, ToriiError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runner {
    /// Platform-native ID (GitLab numeric; GitHub numeric).
    pub id: String,
    pub description: String,
    /// Normalized: online | offline | paused | active | stale | other
    pub status: String,
    pub paused: bool,
    pub ip_address: String,
    pub os: String,
    pub tags: Vec<String>,
    pub version: String,
    /// GitLab: instance_type | group_type | project_type.
    /// GitHub: "self-hosted".
    pub runner_type: String,
    pub web_url: String,
}

#[allow(dead_code)]
pub trait RunnerClient: Send {
    fn list(&self, owner: &str, repo: &str) -> Result<Vec<Runner>>;
    fn show(&self, owner: &str, repo: &str, id: &str) -> Result<Runner>;
    fn remove(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
    /// Reset the runner's authentication token; returns the new token
    /// the operator must paste into the runner's config.
    fn reset_token(&self, owner: &str, repo: &str, id: &str) -> Result<String>;
    fn pause(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
    fn resume(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
    /// Obtain a short-lived registration token. `torii runner register`
    /// uses it to wrap the platform's CLI (gitlab-runner register,
    /// ./config.sh on GitHub Actions). Returns (token, register_url) —
    /// the URL is the value the CLI wants for its `--url` arg.
    fn registration_token(&self, owner: &str, repo: &str) -> Result<RegistrationToken>;
}

#[derive(Debug, Clone)]
pub struct RegistrationToken {
    pub token: String,
    /// URL the runner CLI expects (e.g. https://gitlab.com or
    /// https://github.com/<owner>/<repo>).
    pub register_url: String,
    /// Expiry hint in seconds, when the platform reports one. GitHub
    /// gives ~1h; GitLab tokens don't expire until you regenerate.
    pub expires_in_seconds: Option<u64>,
}

// ============================================================================
// GitLab
// ============================================================================

pub(crate) fn set_paused(
    c: &GitLabRunnerClient,
    _owner: &str,
    _repo: &str,
    id: &str,
    paused: bool,
) -> Result<()> {
    let url = format!("{}/runners/{}", c.base_url, id);
    let req = c
        .client()
        .put(&url)
        .header("Authorization", c.auth())
        .json(&serde_json::json!({ "paused": paused }));
    crate::http::send_empty(req, &format!("GitLab set runner.paused={}", paused))
}

pub fn get_runner_client(platform: &str) -> Result<Box<dyn RunnerClient>> {
    match platform {
        "gitlab" => Ok(Box::new(GitLabRunnerClient::new()?)),
        "github" => Ok(Box::new(GitHubRunnerClient::new()?)),
        other => Err(ToriiError::Unsupported(format!(
            "Runners surface not implemented for `{}` yet. \
             Supported: github, gitlab.",
            other
        ))),
    }
}
