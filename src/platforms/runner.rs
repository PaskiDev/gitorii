//! CI runners surface — list / show / remove / reset-token / pause / resume.
//!
//! GitLab exposes all six operations on shared/group/project runners.
//! GitHub Actions exposes only list/show/remove on self-hosted runners
//! (gating is done via labels, and self-hosted tokens are rotated
//! manually on the runner host). Other platforms aren't covered here
//! yet — calling `get_runner_client` on them returns `Unsupported`.

use serde::{Deserialize, Serialize};
use reqwest::blocking::Client;
use crate::error::{Result, ToriiError};

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
}

// ============================================================================
// GitLab
// ============================================================================

pub struct GitLabRunnerClient {
    token: String,
    base_url: String,
}

impl GitLabRunnerClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("gitlab", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "GitLab token not found. Run: torii auth oauth gitlab".to_string()
            ))?;
        Ok(Self { token, base_url: "https://gitlab.com/api/v4".to_string() })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth(&self) -> String { format!("Bearer {}", self.token) }

    fn project_path(owner: &str, repo: &str) -> String {
        crate::url::encode(&format!("{}/{}", owner, repo))
    }
}

impl RunnerClient for GitLabRunnerClient {
    fn list(&self, owner: &str, repo: &str) -> Result<Vec<Runner>> {
        let url = format!(
            "{}/projects/{}/runners?per_page=100",
            self.base_url, Self::project_path(owner, repo)
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_gitlab_runner).collect()
    }

    fn show(&self, _owner: &str, _repo: &str, id: &str) -> Result<Runner> {
        let url = format!("{}/runners/{}", self.base_url, id);
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        parse_gitlab_runner(&json)
    }

    fn remove(&self, _owner: &str, _repo: &str, id: &str) -> Result<()> {
        let url = format!("{}/runners/{}", self.base_url, id);
        let req = self.client().delete(&url).header("Authorization", self.auth());
        crate::http::send_empty(req, "GitLab delete runner")
    }

    fn reset_token(&self, _owner: &str, _repo: &str, id: &str) -> Result<String> {
        let url = format!("{}/runners/{}/reset_authentication_token", self.base_url, id);
        let req = self.client().post(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, "GitLab reset runner token")?;
        Ok(json["token"].as_str()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitLab returned no `token` field in reset response: {}", json
            )))?
            .to_string())
    }

    fn pause(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        set_paused(self, owner, repo, id, true)
    }
    fn resume(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        set_paused(self, owner, repo, id, false)
    }
}

fn set_paused(c: &GitLabRunnerClient, _owner: &str, _repo: &str, id: &str, paused: bool) -> Result<()> {
    let url = format!("{}/runners/{}", c.base_url, id);
    let req = c.client().put(&url)
        .header("Authorization", c.auth())
        .json(&serde_json::json!({ "paused": paused }));
    crate::http::send_empty(req, &format!("GitLab set runner.paused={}", paused))
}

fn parse_gitlab_runner(v: &serde_json::Value) -> Result<Runner> {
    let id = v["id"].as_u64()
        .ok_or_else(|| ToriiError::InvalidConfig(format!(
            "GitLab runner has no `id`: {}", v
        )))?
        .to_string();
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let paused = v["paused"].as_bool().unwrap_or(false);
    let status = if paused { "paused".to_string() } else { raw_status };

    let tags = v["tag_list"].as_array()
        .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
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

pub struct GitHubRunnerClient { token: String }

impl GitHubRunnerClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("github", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "GitHub token not found. Run: torii auth oauth github".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth(&self) -> String { format!("token {}", self.token) }
    fn accept(&self) -> &'static str { "application/vnd.github+json" }
}

impl RunnerClient for GitHubRunnerClient {
    fn list(&self, owner: &str, repo: &str) -> Result<Vec<Runner>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runners?per_page=100",
            owner, repo
        );
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", self.accept());
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        let arr = json["runners"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitHub returned no `runners` array: {}", json
            )))?;
        arr.iter().map(parse_github_runner).collect()
    }

    fn show(&self, owner: &str, repo: &str, id: &str) -> Result<Runner> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runners/{}",
            owner, repo, id
        );
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", self.accept());
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        parse_github_runner(&json)
    }

    fn remove(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runners/{}",
            owner, repo, id
        );
        let req = self.client().delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", self.accept());
        crate::http::send_empty(req, "GitHub delete runner")
    }

    fn reset_token(&self, _owner: &str, _repo: &str, _id: &str) -> Result<String> {
        Err(ToriiError::InvalidConfig(
            "GitHub Actions doesn't expose a per-runner token reset. \
             Re-register the runner: stop the agent, fetch a fresh \
             registration token from `Settings → Actions → Runners`, \
             and run `./config.sh remove` then `./config.sh` again.".to_string()
        ))
    }

    fn pause(&self, _owner: &str, _repo: &str, _id: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "GitHub Actions has no pause/resume on self-hosted runners. \
             Use a workflow `runs-on:` label that the runner doesn't \
             advertise, or stop the agent on the host.".to_string()
        ))
    }
    fn resume(&self, _owner: &str, _repo: &str, _id: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "GitHub Actions has no pause/resume on self-hosted runners.".to_string()
        ))
    }
}

fn parse_github_runner(v: &serde_json::Value) -> Result<Runner> {
    let id = v["id"].as_u64()
        .ok_or_else(|| ToriiError::InvalidConfig(format!(
            "GitHub runner has no `id`: {}", v
        )))?
        .to_string();
    let busy = v["busy"].as_bool().unwrap_or(false);
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let status = if raw_status == "online" && busy { "active".to_string() }
                 else { raw_status };

    let tags = v["labels"].as_array()
        .map(|a| a.iter().filter_map(|t| t["name"].as_str().map(String::from)).collect())
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

pub fn get_runner_client(platform: &str) -> Result<Box<dyn RunnerClient>> {
    match platform {
        "gitlab" => Ok(Box::new(GitLabRunnerClient::new()?)),
        "github" => Ok(Box::new(GitHubRunnerClient::new()?)),
        other => Err(ToriiError::InvalidConfig(format!(
            "Runners surface not implemented for `{}` yet. \
             Supported: github, gitlab.", other
        ))),
    }
}
