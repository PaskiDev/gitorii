use serde::{Deserialize, Serialize};
use reqwest::blocking::Client;
use chrono::{DateTime, Utc, Duration};
use crate::error::{Result, ToriiError};

// ============================================================================
// Shared types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: String,
    /// Normalized status: success | failed | running | canceled | pending | other
    pub status: String,
    /// Platform-native status string for display (GitLab uses one set,
    /// GitHub Actions splits status/conclusion — we squash that into a
    /// single label here).
    pub raw_status: String,
    pub branch: String,
    pub sha: String,
    pub web_url: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct ListFilters {
    /// Normalized status filter. Translated to platform-specific
    /// query parameter inside each client.
    pub status: Option<String>,
    /// Page size, clamped to [1, 100] per platform API limits.
    pub per_page: usize,
}

#[allow(dead_code)]
pub trait PipelineClient: Send {
    fn list(&self, owner: &str, repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>>;
    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
}

// ============================================================================
// GitHub Actions (workflow runs)
// ============================================================================

pub struct GitHubPipelineClient { token: String }

impl GitHubPipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("github", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "GitHub token not found. Run: torii auth set github YOUR_TOKEN".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        Client::builder().user_agent("gitorii-cli").build().unwrap()
    }

    fn auth_header(&self) -> String { format!("token {}", self.token) }
}

impl PipelineClient for GitHubPipelineClient {
    fn list(&self, owner: &str, repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>> {
        // GitHub splits run state across two parameters:
        //   status=queued|in_progress|completed
        //   ...and once completed, conclusion=success|failure|cancelled|...
        // The API also accepts conclusion-style values directly on the
        // `status` parameter as of 2022 (success, failure, etc.) — they
        // map onto status=completed&conclusion=<value> internally. We
        // exploit that to keep the request to a single param.
        let mut url = format!(
            "https://api.github.com/repos/{}/{}/actions/runs?per_page={}",
            owner, repo, filters.per_page.clamp(1, 100)
        );
        if let Some(ref s) = filters.status {
            let gh = match s.as_str() {
                "success"  => "success",
                "failed"   => "failure",
                "running"  => "in_progress",
                "canceled" => "cancelled",
                "pending"  => "queued",
                other      => other,
            };
            url.push_str(&format!("&status={}", gh));
        }
        let resp = self.client()
            .get(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str().unwrap_or("(no message)");
            return Err(ToriiError::InvalidConfig(format!(
                "GitHub API {}: {} (url: {})", status, msg, url
            )));
        }
        let arr = json["workflow_runs"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitHub returned no workflow_runs array. Body: {}", json
            )))?;
        arr.iter().map(parse_github_run).collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runs/{}/cancel",
            owner, repo, id
        );
        post_no_body(&self.client(), &url, &self.auth_header(), "cancel")
    }

    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runs/{}/rerun",
            owner, repo, id
        );
        post_no_body(&self.client(), &url, &self.auth_header(), "retry")
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runs/{}",
            owner, repo, id
        );
        let resp = self.client().delete(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(ToriiError::InvalidConfig(format!(
                "GitHub API {} delete failed: {}", s, body
            )));
        }
        Ok(())
    }
}

fn post_no_body(client: &Client, url: &str, auth: &str, op: &str) -> Result<()> {
    let resp = client.post(url)
        .header("Authorization", auth)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(ToriiError::InvalidConfig(format!(
            "GitHub API {} {} failed: {}", s, op, body
        )));
    }
    Ok(())
}

fn parse_github_run(v: &serde_json::Value) -> Result<Pipeline> {
    let id = v["id"].as_u64().map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::InvalidConfig("GitHub run missing id".into()))?;
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let conclusion = v["conclusion"].as_str().unwrap_or("");
    let label = if raw_status == "completed" && !conclusion.is_empty() {
        conclusion.to_string()
    } else {
        raw_status.clone()
    };
    let status = match raw_status.as_str() {
        "queued"      => "pending".to_string(),
        "in_progress" => "running".to_string(),
        "completed"   => match conclusion {
            "success"   => "success",
            "failure" | "timed_out" => "failed",
            "cancelled" => "canceled",
            _           => "other",
        }.to_string(),
        _             => "other".to_string(),
    };
    Ok(Pipeline {
        id,
        status,
        raw_status: label,
        branch: v["head_branch"].as_str().unwrap_or("").to_string(),
        sha: v["head_sha"].as_str().unwrap_or("").to_string(),
        web_url: v["html_url"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        updated_at: v["updated_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// GitLab Pipelines
// ============================================================================

pub struct GitLabPipelineClient {
    token: String,
    base_url: String,
}

impl GitLabPipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("gitlab", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "GitLab token not found. Run: torii auth set gitlab YOUR_TOKEN".to_string()
            ))?;
        let base_url = std::env::var("GITLAB_URL")
            .unwrap_or_else(|_| "https://gitlab.com/api/v4".to_string());
        Ok(Self { token, base_url })
    }

    fn client(&self) -> Client {
        Client::builder().user_agent("gitorii-cli").build().unwrap()
    }

    fn project_path(owner: &str, repo: &str) -> String {
        crate::url::encode(&format!("{}/{}", owner, repo))
    }
}

impl PipelineClient for GitLabPipelineClient {
    fn list(&self, owner: &str, repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>> {
        let mut url = format!(
            "{}/projects/{}/pipelines?per_page={}",
            self.base_url, Self::project_path(owner, repo),
            filters.per_page.clamp(1, 100)
        );
        if let Some(ref s) = filters.status {
            let gl = match s.as_str() {
                "success"  => "success",
                "failed"   => "failed",
                "running"  => "running",
                "canceled" => "canceled",
                "pending"  => "pending",
                other      => other,
            };
            url.push_str(&format!("&status={}", gl));
        }
        let resp = self.client().get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str()
                .or_else(|| json["error"].as_str())
                .unwrap_or("(no message)");
            return Err(ToriiError::InvalidConfig(format!(
                "GitLab API {}: {} (url: {})", status, msg, url
            )));
        }
        let arr = json.as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitLab returned non-array body for {}. Body: {}", url, json
            )))?;
        arr.iter().map(parse_gitlab_pipeline).collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}/cancel",
            self.base_url, Self::project_path(owner, repo), id
        );
        let resp = self.client().post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        gitlab_check_ok(resp, "cancel")
    }

    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}/retry",
            self.base_url, Self::project_path(owner, repo), id
        );
        let resp = self.client().post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        gitlab_check_ok(resp, "retry")
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}",
            self.base_url, Self::project_path(owner, repo), id
        );
        let resp = self.client().delete(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        gitlab_check_ok(resp, "delete")
    }
}

fn gitlab_check_ok(resp: reqwest::blocking::Response, op: &str) -> Result<()> {
    let status = resp.status();
    if status.is_success() { return Ok(()); }
    let body = resp.text().unwrap_or_default();
    Err(ToriiError::InvalidConfig(format!(
        "GitLab API {} {} failed: {}", status, op, body
    )))
}

fn parse_gitlab_pipeline(v: &serde_json::Value) -> Result<Pipeline> {
    let id = v["id"].as_u64().map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::InvalidConfig("GitLab pipeline missing id".into()))?;
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let status = match raw_status.as_str() {
        "success"                                          => "success",
        "failed"                                           => "failed",
        "running" | "preparing" | "waiting_for_resource"   => "running",
        "canceled" | "cancelled"                           => "canceled",
        "pending" | "scheduled" | "created" | "manual"     => "pending",
        _                                                  => "other",
    }.to_string();
    Ok(Pipeline {
        id,
        status,
        raw_status,
        branch: v["ref"].as_str().unwrap_or("").to_string(),
        sha: v["sha"].as_str().unwrap_or("").to_string(),
        web_url: v["web_url"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        updated_at: v["updated_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Factory + helpers
// ============================================================================

pub fn get_pipeline_client(platform: &str) -> Result<Box<dyn PipelineClient>> {
    match platform.to_lowercase().as_str() {
        "github" => Ok(Box::new(GitHubPipelineClient::new()?)),
        "gitlab" => Ok(Box::new(GitLabPipelineClient::new()?)),
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported: github, gitlab", other)
        )),
    }
}

/// Keep only pipelines created more than `days` ago. Pipelines whose
/// `created_at` is empty or unparseable are kept (we don't act on
/// state we can't reason about — the user can still inspect via
/// `pipeline list`).
pub fn filter_older_than(pipelines: Vec<Pipeline>, days: i64) -> Vec<Pipeline> {
    let cutoff = Utc::now() - Duration::days(days);
    pipelines.into_iter().filter(|p| {
        match DateTime::parse_from_rfc3339(&p.created_at) {
            Ok(dt) => dt.with_timezone(&Utc) < cutoff,
            Err(_) => true,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(id: &str, status: &str, created_at: &str) -> Pipeline {
        Pipeline {
            id: id.into(),
            status: status.into(),
            raw_status: status.into(),
            branch: "main".into(),
            sha: "deadbeef".into(),
            web_url: String::new(),
            created_at: created_at.into(),
            updated_at: created_at.into(),
        }
    }

    #[test]
    fn parse_github_completed_failure_normalizes_to_failed() {
        let json = serde_json::json!({
            "id": 12345u64,
            "status": "completed",
            "conclusion": "failure",
            "head_branch": "main",
            "head_sha": "abc",
            "html_url": "https://x",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
        });
        let p = parse_github_run(&json).unwrap();
        assert_eq!(p.id, "12345");
        assert_eq!(p.status, "failed");
        assert_eq!(p.raw_status, "failure");
    }

    #[test]
    fn parse_github_in_progress_normalizes_to_running() {
        let json = serde_json::json!({
            "id": 1u64, "status": "in_progress", "conclusion": serde_json::Value::Null,
            "head_branch": "main", "head_sha": "a", "html_url": "",
            "created_at": "", "updated_at": "",
        });
        assert_eq!(parse_github_run(&json).unwrap().status, "running");
    }

    #[test]
    fn parse_gitlab_canceled_normalizes() {
        let json = serde_json::json!({
            "id": 42u64, "status": "canceled", "ref": "main", "sha": "a",
            "web_url": "https://x", "created_at": "", "updated_at": "",
        });
        let p = parse_gitlab_pipeline(&json).unwrap();
        assert_eq!(p.status, "canceled");
        assert_eq!(p.raw_status, "canceled");
    }

    #[test]
    fn filter_older_than_keeps_recent_drops_old() {
        let now = Utc::now();
        let recent = (now - Duration::days(1)).to_rfc3339();
        let ancient = (now - Duration::days(30)).to_rfc3339();
        let list = vec![
            mk("recent",  "failed", &recent),
            mk("ancient", "failed", &ancient),
        ];
        let kept = filter_older_than(list, 7);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "ancient");
    }

    #[test]
    fn filter_older_than_keeps_unparseable_timestamps() {
        // Conservative: if we can't tell when it was created, we
        // don't delete it. Keep it so the user can see it.
        let kept = filter_older_than(vec![mk("x", "failed", "not a date")], 7);
        assert_eq!(kept.len(), 1);
    }
}
