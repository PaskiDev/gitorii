use super::azure::AzurePipelineClient;
use super::bitbucket::BitbucketPipelineClient;
use super::gitea::GiteaPipelineClient;
use super::github::GitHubPipelineClient;
use super::gitlab::GitLabPipelineClient;
use super::radicle::RadiclePipelineClient;
use super::sourcehut::SourcehutPipelineClient;
use crate::error::{Result, ToriiError};
use chrono::{DateTime, Duration, Utc};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    /// Pipeline / workflow-run this job belongs to.
    pub pipeline_id: String,
    pub name: String,
    /// Normalized status: success | failed | running | canceled | pending | other
    pub status: String,
    pub raw_status: String,
    pub stage: String,
    pub web_url: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub duration_seconds: Option<f64>,
}

#[allow(dead_code)]
pub trait PipelineClient: Send {
    // --- pipeline ops ---
    fn list(&self, owner: &str, repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>>;
    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()>;

    // --- job ops (0.7.10) ---
    fn list_jobs(
        &self,
        owner: &str,
        repo: &str,
        pipeline_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<Job>>;
    /// Fetch the raw log/trace for a single job. Returned as a String;
    /// caller decides whether to print it all or `--tail N`.
    fn job_log(&self, owner: &str, repo: &str, job_id: &str) -> Result<String>;
    fn job_retry(&self, owner: &str, repo: &str, job_id: &str) -> Result<()>;
    fn job_cancel(&self, owner: &str, repo: &str, job_id: &str) -> Result<()>;
    /// Download the job's artifacts archive to `output_path`. GitLab
    /// supports this per-job; GitHub Actions only offers artifacts at
    /// the workflow-run level, so the GitHub impl returns an error
    /// pointing the user at the per-run download flow.
    fn job_artifacts_download(
        &self,
        owner: &str,
        repo: &str,
        job_id: &str,
        output_path: &std::path::Path,
    ) -> Result<()>;
    /// Erase a job's log + artifacts but keep its metadata visible in
    /// the UI (GitLab-specific operation; GitHub returns unsupported).
    fn job_erase(&self, owner: &str, repo: &str, job_id: &str) -> Result<()>;
}

// ============================================================================
// GitHub Actions (workflow runs)
// ============================================================================

/// Helper used by GitHub Actions and Gitea Actions clients for cancel /
/// retry / job_retry — POSTs to an action URL with no body and translates
/// the response to a clear error. Used by GitHub clients that need to
/// send the `Accept: vnd.github+json` header.
pub(crate) fn post_no_body(client: &Client, url: &str, auth: &str, op: &str) -> Result<()> {
    let req = client
        .post(url)
        .header("Authorization", auth)
        .header("Accept", "application/vnd.github+json");
    crate::http::send_empty(req, &format!("GitHub {}", op))
}

pub fn get_pipeline_client(platform: &str) -> Result<Box<dyn PipelineClient>> {
    get_pipeline_client_with_base_url(platform, None)
}

/// 0.8.0 — same as `get_pipeline_client` but lets the caller override
/// the API base URL from a `platforms.toml` entry. Today only GitLab
/// honours the override end-to-end; the rest of the kinds still
/// build against their builtin defaults. v0.8.1 will extend the
/// override to Gitea / GitHub Enterprise / Bitbucket Data Center.
pub fn get_pipeline_client_with_base_url(
    platform: &str,
    base_url: Option<&str>,
) -> Result<Box<dyn PipelineClient>> {
    match platform.to_lowercase().as_str() {
        "github"    => Ok(Box::new(GitHubPipelineClient::new()?)),
        "gitlab"    => Ok(Box::new(GitLabPipelineClient::new_with_base_url(base_url)?)),
        "gitea"     => Ok(Box::new(GiteaPipelineClient::new()?)),
        "sourcehut" => Ok(Box::new(SourcehutPipelineClient::new()?)),
        "radicle"   => Ok(Box::new(RadiclePipelineClient::new()?)),
        "bitbucket" => Ok(Box::new(BitbucketPipelineClient::new()?)),
        "azure"     => Ok(Box::new(AzurePipelineClient::new()?)),
        other => Err(ToriiError::Unsupported(format!("Unsupported platform: {}. Supported: github, gitlab, gitea, sourcehut, radicle, bitbucket, azure", other))),
    }
}

/// Keep only pipelines created more than `days` ago. Pipelines whose
/// `created_at` is empty or unparseable are kept (we don't act on
/// state we can't reason about — the user can still inspect via
/// `pipeline list`).
pub fn filter_older_than(pipelines: Vec<Pipeline>, days: i64) -> Vec<Pipeline> {
    let cutoff = Utc::now() - Duration::days(days);
    pipelines
        .into_iter()
        .filter(|p| match DateTime::parse_from_rfc3339(&p.created_at) {
            Ok(dt) => dt.with_timezone(&Utc) < cutoff,
            Err(_) => true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::github::pipeline::parse_github_run;
    use crate::platforms::gitlab::pipeline::parse_gitlab_pipeline;

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
            mk("recent", "failed", &recent),
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
