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
    fn list_jobs(&self, owner: &str, repo: &str, pipeline_id: &str, status_filter: Option<&str>) -> Result<Vec<Job>>;
    /// Fetch the raw log/trace for a single job. Returned as a String;
    /// caller decides whether to print it all or `--tail N`.
    fn job_log(&self, owner: &str, repo: &str, job_id: &str) -> Result<String>;
    fn job_retry(&self, owner: &str, repo: &str, job_id: &str) -> Result<()>;
    fn job_cancel(&self, owner: &str, repo: &str, job_id: &str) -> Result<()>;
    /// Download the job's artifacts archive to `output_path`. GitLab
    /// supports this per-job; GitHub Actions only offers artifacts at
    /// the workflow-run level, so the GitHub impl returns an error
    /// pointing the user at the per-run download flow.
    fn job_artifacts_download(&self, owner: &str, repo: &str, job_id: &str, output_path: &std::path::Path) -> Result<()>;
    /// Erase a job's log + artifacts but keep its metadata visible in
    /// the UI (GitLab-specific operation; GitHub returns unsupported).
    fn job_erase(&self, owner: &str, repo: &str, job_id: &str) -> Result<()>;
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
        crate::http::make_client()
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
        let req = self.client().get(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
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
        let req = self.client().delete(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json");
        crate::http::send_empty(req, "GitHub delete run")
    }

    // ---- job ops on GitHub Actions ----

    fn list_jobs(&self, owner: &str, repo: &str, pipeline_id: &str, status_filter: Option<&str>) -> Result<Vec<Job>> {
        // GitHub Actions: "jobs in a workflow run". The `filter` query
        // param accepts `latest` | `all`; per-status filtering happens
        // client-side after the fetch.
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/runs/{}/jobs?per_page=100",
            owner, repo, pipeline_id
        );
        let req = self.client().get(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        let arr = json["jobs"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitHub returned no `jobs` array. Body: {}", json
            )))?;
        let jobs: Vec<Job> = arr.iter().filter_map(|v| parse_github_job(v, pipeline_id).ok()).collect();
        // Apply status filter client-side
        if let Some(s) = status_filter {
            Ok(jobs.into_iter().filter(|j| j.status == s).collect())
        } else {
            Ok(jobs)
        }
    }

    fn job_log(&self, owner: &str, repo: &str, job_id: &str) -> Result<String> {
        // GitHub returns a 302 redirect to a signed log URL. reqwest
        // follows redirects by default. We can't use send_json here —
        // the body is plain text, not JSON.
        let url = format!(
            "https://api.github.com/repos/{}/{}/actions/jobs/{}/logs",
            owner, repo, job_id
        );
        let req = self.client().get(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json");
        crate::http::send_text(req, "GitHub job log")
    }

    fn job_retry(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        // GitHub Actions has no per-job retry — only `/runs/:run_id/rerun`
        // and `/runs/:run_id/rerun-failed-jobs`. Both operate at the run
        // level. Point the user at `torii pipeline retry <run-id>` so
        // the CLI surface stays honest.
        Err(ToriiError::InvalidConfig(
            "GitHub Actions doesn't support per-job retry. Use `torii pipeline retry <run-id>` to re-run failed jobs in a workflow run.".to_string()
        ))
    }

    fn job_cancel(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "GitHub Actions doesn't support per-job cancel. Use `torii pipeline cancel <run-id>` to stop a workflow run.".to_string()
        ))
    }

    fn job_artifacts_download(&self, _owner: &str, _repo: &str, _job_id: &str, _output_path: &std::path::Path) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "GitHub Actions artifacts are scoped to the workflow run, not the job. List artifacts with `torii pipeline list` then use the GitHub UI / API directly until torii adds per-run artifact download.".to_string()
        ))
    }

    fn job_erase(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        // GitLab-only operation; GitHub Actions doesn't expose log-erase.
        Err(ToriiError::InvalidConfig(
            "GitHub Actions doesn't support per-job log erase. Logs are retained for the run lifetime; use `torii pipeline delete <run-id>` to discard the run entirely.".to_string()
        ))
    }
}

fn parse_github_job(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let id = v["id"].as_u64().map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::InvalidConfig("GitHub job missing id".into()))?;
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
            "success"               => "success",
            "failure" | "timed_out" => "failed",
            "cancelled"             => "canceled",
            _                       => "other",
        }.to_string(),
        _             => "other".to_string(),
    };
    // GitHub job duration = finished_at - started_at if both set.
    let started_at = v["started_at"].as_str();
    let finished_at = v["completed_at"].as_str();
    let duration = match (started_at, finished_at) {
        (Some(s), Some(f)) => {
            use chrono::DateTime;
            match (DateTime::parse_from_rfc3339(s), DateTime::parse_from_rfc3339(f)) {
                (Ok(s), Ok(f)) => Some((f - s).num_milliseconds() as f64 / 1000.0),
                _ => None,
            }
        }
        _ => None,
    };
    Ok(Job {
        id,
        pipeline_id: pipeline_id.to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        status,
        raw_status: label,
        stage: String::new(), // GitHub Actions has no "stage" concept
        web_url: v["html_url"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        finished_at: finished_at.map(String::from),
        duration_seconds: duration,
    })
}

/// Helper used by GitHub Actions and Gitea Actions clients for cancel /
/// retry / job_retry — POSTs to an action URL with no body and translates
/// the response to a clear error. Used by GitHub clients that need to
/// send the `Accept: vnd.github+json` header.
fn post_no_body(client: &Client, url: &str, auth: &str, op: &str) -> Result<()> {
    let req = client.post(url)
        .header("Authorization", auth)
        .header("Accept", "application/vnd.github+json");
    crate::http::send_empty(req, &format!("GitHub {}", op))
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
        crate::http::make_client()
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
        let req = self.client().get(&url).header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_gitlab_pipeline).collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}/cancel",
            self.base_url, Self::project_path(owner, repo), id
        );
        let req = self.client().post(&url).header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab cancel pipeline")
    }

    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}/retry",
            self.base_url, Self::project_path(owner, repo), id
        );
        let req = self.client().post(&url).header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab retry pipeline")
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}",
            self.base_url, Self::project_path(owner, repo), id
        );
        let req = self.client().delete(&url).header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab delete pipeline")
    }

    // ---- job ops on GitLab Pipelines ----

    fn list_jobs(&self, owner: &str, repo: &str, pipeline_id: &str, status_filter: Option<&str>) -> Result<Vec<Job>> {
        // GitLab supports `?scope[]=failed&scope[]=success` for server-side
        // filtering, but a single client-side filter is simpler and
        // doesn't risk an empty array because of a typo in the scope name.
        let url = format!(
            "{}/projects/{}/pipelines/{}/jobs?per_page=100",
            self.base_url, Self::project_path(owner, repo), pipeline_id
        );
        let req = self.client().get(&url).header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        let arr = crate::http::extract_array(&json, &url)?;
        let jobs: Vec<Job> = arr.iter().filter_map(|v| parse_gitlab_job(v, pipeline_id).ok()).collect();
        if let Some(s) = status_filter {
            Ok(jobs.into_iter().filter(|j| j.status == s).collect())
        } else {
            Ok(jobs)
        }
    }

    fn job_log(&self, owner: &str, repo: &str, job_id: &str) -> Result<String> {
        // `/jobs/:id/trace` returns the raw text log directly (no JSON
        // wrapping), so we use `.text()` instead of `.json()`.
        let url = format!(
            "{}/projects/{}/jobs/{}/trace",
            self.base_url, Self::project_path(owner, repo), job_id
        );
        let req = self.client().get(&url).header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_text(req, "GitLab job trace")
    }

    fn job_retry(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/jobs/{}/retry",
            self.base_url, Self::project_path(owner, repo), job_id
        );
        let req = self.client().post(&url).header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab job retry")
    }

    fn job_cancel(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/jobs/{}/cancel",
            self.base_url, Self::project_path(owner, repo), job_id
        );
        let req = self.client().post(&url).header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab job cancel")
    }

    fn job_artifacts_download(&self, owner: &str, repo: &str, job_id: &str, output_path: &std::path::Path) -> Result<()> {
        let url = format!(
            "{}/projects/{}/jobs/{}/artifacts",
            self.base_url, Self::project_path(owner, repo), job_id
        );
        let req = self.client().get(&url).header("Authorization", format!("Bearer {}", self.token));
        let bytes = crate::http::send_bytes(req, "GitLab artifacts")?;
        std::fs::write(output_path, &bytes)
            .map_err(|e| ToriiError::InvalidConfig(format!("Failed to write artifacts to {}: {}", output_path.display(), e)))?;
        Ok(())
    }

    fn job_erase(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/jobs/{}/erase",
            self.base_url, Self::project_path(owner, repo), job_id
        );
        let req = self.client().post(&url).header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab job erase")
    }
}

fn parse_gitlab_job(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let id = v["id"].as_u64().map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::InvalidConfig("GitLab job missing id".into()))?;
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let status = match raw_status.as_str() {
        "success"                                          => "success",
        "failed"                                           => "failed",
        "running" | "preparing" | "waiting_for_resource"   => "running",
        "canceled" | "cancelled"                           => "canceled",
        "pending" | "scheduled" | "created" | "manual"     => "pending",
        "skipped"                                          => "canceled",
        _                                                  => "other",
    }.to_string();
    Ok(Job {
        id,
        pipeline_id: pipeline_id.to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        status,
        raw_status,
        stage: v["stage"].as_str().unwrap_or("").to_string(),
        web_url: v["web_url"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        finished_at: v["finished_at"].as_str().map(String::from),
        duration_seconds: v["duration"].as_f64(),
    })
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

// ============================================================================
// Gitea Actions (runs)
// ============================================================================
//
// Gitea Actions is a GitHub-Actions-compatible runner introduced in
// Gitea 1.19+ / Forgejo (the Codeberg fork). The public REST endpoints
// at `/api/v1/repos/{owner}/{repo}/actions/runs` mirror GitHub's
// shape; status enum follows the same `success/failure/in_progress`
// convention. Older Gitea instances may 404 on these endpoints — we
// surface the platform error rather than guessing.

pub struct GiteaPipelineClient {
    token: String,
    base_url: String,
}

impl GiteaPipelineClient {
    pub fn new() -> Result<Self> {
        Self::new_with_host(crate::pr::gitea_base_url())
    }

    pub fn new_with_host(base_url: &str) -> Result<Self> {
        let token = crate::pr::resolve_gitea_token()?;
        Ok(Self { token, base_url: base_url.trim_end_matches('/').to_string() })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth_header(&self) -> String { format!("token {}", self.token) }
}

impl PipelineClient for GiteaPipelineClient {
    fn list(&self, owner: &str, repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>> {
        let mut url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs?limit={}",
            self.base_url, owner, repo, filters.per_page.clamp(1, 50)
        );
        if let Some(ref s) = filters.status {
            // Gitea Actions matches GitHub's vocabulary.
            let g = match s.as_str() {
                "success"  => "success",
                "failed"   => "failure",
                "running"  => "in_progress",
                "canceled" => "cancelled",
                "pending"  => "queued",
                other      => other,
            };
            url.push_str(&format!("&status={}", g));
        }
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Gitea (url: {}) — Actions API requires Gitea >=1.19", url))?;
        let arr = json["workflow_runs"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Gitea returned no workflow_runs array. Body: {}", json
            )))?;
        arr.iter().map(parse_gitea_run).collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs/{}/cancel",
            self.base_url, owner, repo, id
        );
        let req = self.client().post(&url).header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Gitea cancel run")
    }

    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs/{}/rerun",
            self.base_url, owner, repo, id
        );
        let req = self.client().post(&url).header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Gitea retry run")
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs/{}",
            self.base_url, owner, repo, id
        );
        let req = self.client().delete(&url).header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Gitea delete run")
    }

    fn list_jobs(&self, owner: &str, repo: &str, pipeline_id: &str, status_filter: Option<&str>) -> Result<Vec<Job>> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs/{}/jobs",
            self.base_url, owner, repo, pipeline_id
        );
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Gitea (url: {})", url))?;
        let arr = json["jobs"].as_array()
            .or_else(|| json.as_array())
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Gitea returned no jobs array. Body: {}", json
            )))?;
        let mut jobs: Vec<Job> = arr.iter()
            .filter_map(|v| parse_gitea_job(v, pipeline_id).ok())
            .collect();
        if let Some(f) = status_filter {
            jobs.retain(|j| j.status == f);
        }
        Ok(jobs)
    }

    fn job_log(&self, owner: &str, repo: &str, job_id: &str) -> Result<String> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/jobs/{}/logs",
            self.base_url, owner, repo, job_id
        );
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        crate::http::send_text(req, "Gitea job log")
    }

    fn job_retry(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/jobs/{}/rerun",
            self.base_url, owner, repo, job_id
        );
        let req = self.client().post(&url).header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Gitea job retry")
    }

    fn job_cancel(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        // Gitea Actions exposes cancel at the run level, not per-job.
        // Direct callers should cancel the whole run instead.
        Err(ToriiError::InvalidConfig(
            "Gitea Actions cancels at run level — use `torii pipeline cancel <id>`".to_string()
        ))
    }

    fn job_artifacts_download(&self, _owner: &str, _repo: &str, _job_id: &str, _output_path: &std::path::Path) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Gitea Actions: per-job artifact download not exposed by the v1 API. Fetch the run's artifact from the web UI.".to_string()
        ))
    }

    fn job_erase(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        // GitLab-specific concept — Gitea doesn't model "erase trace +
        // artifacts but keep job row". Closest analog is deleting the
        // whole run.
        Err(ToriiError::InvalidConfig(
            "Gitea Actions has no per-job erase. Delete the whole run with `torii pipeline delete <id>`.".to_string()
        ))
    }
}

fn parse_gitea_run(v: &serde_json::Value) -> Result<Pipeline> {
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let conclusion = v["conclusion"].as_str().unwrap_or("");
    // Gitea mirrors GitHub's status/conclusion split for completed runs.
    let normalized = match (raw_status.as_str(), conclusion) {
        ("completed", "success")   => "success",
        ("completed", "failure")   => "failed",
        ("completed", "cancelled") => "canceled",
        ("in_progress", _)         => "running",
        ("queued", _)              => "pending",
        ("waiting", _)             => "pending",
        (other, _)                 => other,
    }.to_string();
    let raw_display = if !conclusion.is_empty() {
        format!("{} ({})", raw_status, conclusion)
    } else {
        raw_status
    };
    Ok(Pipeline {
        id:         v["id"].as_u64().map(|n| n.to_string())
                        .or_else(|| v["id"].as_str().map(String::from))
                        .unwrap_or_default(),
        status:     normalized,
        raw_status: raw_display,
        branch:     v["head_branch"].as_str().unwrap_or("").to_string(),
        sha:        v["head_sha"].as_str().unwrap_or("").to_string(),
        web_url:    v["html_url"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        updated_at: v["updated_at"].as_str().unwrap_or("").to_string(),
    })
}

fn parse_gitea_job(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let conclusion = v["conclusion"].as_str().unwrap_or("");
    let normalized = match (raw_status.as_str(), conclusion) {
        ("completed", "success")   => "success",
        ("completed", "failure")   => "failed",
        ("completed", "cancelled") => "canceled",
        ("in_progress", _)         => "running",
        ("queued", _)              => "pending",
        ("waiting", _)             => "pending",
        (other, _)                 => other,
    }.to_string();
    let raw_display = if !conclusion.is_empty() {
        format!("{} ({})", raw_status, conclusion)
    } else {
        raw_status
    };
    let started  = v["started_at"].as_str().unwrap_or("");
    let finished = v["completed_at"].as_str().unwrap_or("");
    let duration_seconds = if !started.is_empty() && !finished.is_empty() {
        match (DateTime::parse_from_rfc3339(started), DateTime::parse_from_rfc3339(finished)) {
            (Ok(s), Ok(f)) => Some((f - s).num_seconds() as f64),
            _ => None,
        }
    } else { None };
    Ok(Job {
        id:               v["id"].as_u64().map(|n| n.to_string())
                              .or_else(|| v["id"].as_str().map(String::from))
                              .unwrap_or_default(),
        pipeline_id:      pipeline_id.to_string(),
        name:             v["name"].as_str().unwrap_or("").to_string(),
        status:           normalized,
        raw_status:       raw_display,
        stage:            v["workflow_name"].as_str().unwrap_or("").to_string(),
        web_url:          v["html_url"].as_str().unwrap_or("").to_string(),
        created_at:       v["created_at"].as_str().unwrap_or("").to_string(),
        finished_at:      v["completed_at"].as_str().map(String::from),
        duration_seconds,
    })
}

// ============================================================================
// Factory
// ============================================================================

// ============================================================================
// Sourcehut (builds.sr.ht)
// ============================================================================
//
// builds.sr.ht is sourcehut's CI. Its model is flat: a "job" is the
// equivalent of a *pipeline* on other hosts (it's a manifest +
// container + log, not a sub-step). Sourcehut has no per-job
// sub-jobs concept, so `list_jobs(pipeline_id)` returns the run as a
// single entry to keep the surface uniform.
//
// builds.sr.ht REST is scoped to the *authenticated user* — there's no
// "list builds for this repo" endpoint. We list the user's recent jobs
// and surface them all. Filtering by repo requires looking at the
// manifest's `tags` (best-effort).

pub struct SourcehutPipelineClient {
    token: String,
}

impl SourcehutPipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("sourcehut", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "Sourcehut token not found. Generate one at https://meta.sr.ht/oauth and run: \
                 torii auth set sourcehut YOUR_TOKEN".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth(&self) -> String { format!("token {}", self.token) }
}

impl PipelineClient for SourcehutPipelineClient {
    fn list(&self, _owner: &str, _repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>> {
        // builds.sr.ht's listing is user-scoped, not repo-scoped — the
        // owner/repo args are unused. We document this in `--help`.
        let url = format!(
            "https://builds.sr.ht/api/jobs?per_page={}",
            filters.per_page.clamp(1, 50)
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Sourcehut builds (url: {})", url))?;
        let arr = json["results"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Sourcehut returned no `results` array. Body: {}", json
            )))?;
        let normalized_filter = filters.status.as_deref();
        arr.iter()
            .map(parse_sourcehut_build)
            .filter(|p| match (p, normalized_filter) {
                (Ok(pi), Some(s)) => pi.status == s,
                _ => true,
            })
            .collect()
    }

    fn cancel(&self, _owner: &str, _repo: &str, id: &str) -> Result<()> {
        let url = format!("https://builds.sr.ht/api/jobs/{}/cancel", id);
        let req = self.client().post(&url).header("Authorization", self.auth());
        crate::http::send_empty(req, "Sourcehut cancel build")
    }

    fn retry(&self, _owner: &str, _repo: &str, id: &str) -> Result<()> {
        // builds.sr.ht allows resubmitting a job from its manifest. The
        // canonical endpoint is `/api/jobs/{id}/start`, but it only
        // works for jobs that haven't been started yet — for actually
        // failed jobs you have to POST a new job from the same manifest
        // via `/api/jobs`. That's not exposed today; point the user at
        // the web UI.
        Err(ToriiError::InvalidConfig(format!(
            "Sourcehut builds doesn't expose a retry endpoint for finished jobs. \
             Resubmit job #{} from the web UI (https://builds.sr.ht/~user/job/{}) \
             or POST the same manifest again via the API.", id, id
        )))
    }

    fn delete(&self, _owner: &str, _repo: &str, _id: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Sourcehut builds doesn't allow deleting jobs — they're \
             retained per the host's retention policy and aren't user-deletable.".to_string()
        ))
    }

    fn list_jobs(&self, _owner: &str, _repo: &str, pipeline_id: &str, _status_filter: Option<&str>) -> Result<Vec<Job>> {
        // On sourcehut a "job" IS the pipeline. We return the same
        // record reshaped as a single Job so the CLI surface stays
        // uniform with GitLab/GitHub.
        let url = format!("https://builds.sr.ht/api/jobs/{}", pipeline_id);
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Sourcehut build #{}", pipeline_id))?;
        let pipeline = parse_sourcehut_build(&json)?;
        Ok(vec![Job {
            id:               pipeline.id.clone(),
            pipeline_id:      pipeline.id.clone(),
            name:             json["note"].as_str().unwrap_or("(sourcehut job)").to_string(),
            status:           pipeline.status.clone(),
            raw_status:       pipeline.raw_status.clone(),
            stage:            "build".to_string(),
            web_url:          pipeline.web_url.clone(),
            created_at:       pipeline.created_at.clone(),
            finished_at:      None,
            duration_seconds: None,
        }])
    }

    fn job_log(&self, _owner: &str, _repo: &str, job_id: &str) -> Result<String> {
        let url = format!("https://builds.sr.ht/api/jobs/{}/log", job_id);
        let req = self.client().get(&url).header("Authorization", self.auth());
        crate::http::send_text(req, "Sourcehut job log")
    }

    fn job_retry(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        // Same limitation as `retry`.
        self.retry(owner, repo, job_id)
    }

    fn job_cancel(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        self.cancel(owner, repo, job_id)
    }

    fn job_artifacts_download(&self, _owner: &str, _repo: &str, _job_id: &str, _output_path: &std::path::Path) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Sourcehut builds doesn't expose artifacts via the REST API. \
             The job manifest can declare `triggers` that upload to a \
             URL, but there's no per-job artifacts endpoint.".to_string()
        ))
    }

    fn job_erase(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Sourcehut builds has no log-erase operation.".to_string()
        ))
    }
}

fn parse_sourcehut_build(v: &serde_json::Value) -> Result<Pipeline> {
    let id = v["id"].as_u64().map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .unwrap_or_default();
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    // builds.sr.ht statuses: pending, queued, running, success, failed,
    // cancelled, timeout. Normalize:
    let status = match raw_status.as_str() {
        "success"             => "success",
        "failed" | "timeout"  => "failed",
        "running"             => "running",
        "cancelled"           => "canceled",
        "pending" | "queued"  => "pending",
        _                     => "other",
    }.to_string();
    let owner = v["owner"]["canonical_name"].as_str().unwrap_or("");
    Ok(Pipeline {
        id: id.clone(),
        status,
        raw_status,
        // builds.sr.ht jobs aren't anchored to a single repo+branch in
        // the API response — these come from the manifest's `tags` if
        // the user set them.
        branch:     v["tags"].as_array().and_then(|a| a.iter().filter_map(|v| v.as_str()).next().map(String::from)).unwrap_or_default(),
        sha:        String::new(),
        web_url:    format!("https://builds.sr.ht/{}/job/{}", owner, id),
        created_at: v["created"].as_str().unwrap_or("").to_string(),
        updated_at: v["updated"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Factory
// ============================================================================

// ============================================================================
// Radicle (no native CI)
// ============================================================================

pub struct RadiclePipelineClient;

impl RadiclePipelineClient {
    pub fn new() -> Result<Self> { Ok(Self) }
}

fn radicle_ci_unsupported() -> ToriiError {
    ToriiError::InvalidConfig(
        "Radicle has no built-in CI. Mirror the project to a host with CI \
         (GitLab, GitHub, Codeberg, Sourcehut) and use that platform's \
         pipeline surface, or run CI locally / on your own runner.".to_string()
    )
}

impl PipelineClient for RadiclePipelineClient {
    fn list(&self, _o: &str, _r: &str, _f: &ListFilters) -> Result<Vec<Pipeline>> { Err(radicle_ci_unsupported()) }
    fn cancel(&self, _o: &str, _r: &str, _id: &str) -> Result<()> { Err(radicle_ci_unsupported()) }
    fn retry(&self, _o: &str, _r: &str, _id: &str) -> Result<()> { Err(radicle_ci_unsupported()) }
    fn delete(&self, _o: &str, _r: &str, _id: &str) -> Result<()> { Err(radicle_ci_unsupported()) }
    fn list_jobs(&self, _o: &str, _r: &str, _p: &str, _f: Option<&str>) -> Result<Vec<Job>> { Err(radicle_ci_unsupported()) }
    fn job_log(&self, _o: &str, _r: &str, _j: &str) -> Result<String> { Err(radicle_ci_unsupported()) }
    fn job_retry(&self, _o: &str, _r: &str, _j: &str) -> Result<()> { Err(radicle_ci_unsupported()) }
    fn job_cancel(&self, _o: &str, _r: &str, _j: &str) -> Result<()> { Err(radicle_ci_unsupported()) }
    fn job_artifacts_download(&self, _o: &str, _r: &str, _j: &str, _p: &std::path::Path) -> Result<()> { Err(radicle_ci_unsupported()) }
    fn job_erase(&self, _o: &str, _r: &str, _j: &str) -> Result<()> { Err(radicle_ci_unsupported()) }
}

// ============================================================================
// Bitbucket Pipelines (REST v2)
// ============================================================================
//
// Bitbucket runs CI via pipelines + steps. Pipeline ≈ pipeline, step ≈
// job. UUIDs are the canonical identifiers (build_number works too).
// `retry` and `delete` aren't exposed via the public REST API — return
// clear errors.

pub struct BitbucketPipelineClient {
    token: String,
}

impl BitbucketPipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("bitbucket", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "Bitbucket token not found. Create an app password at \
                 https://bitbucket.org/account/settings/app-passwords/ \
                 and run: torii auth set bitbucket USERNAME:APP_PASSWORD".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth_header(&self) -> String {
        if self.token.contains(':') {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&self.token);
            format!("Basic {}", b64)
        } else {
            format!("Bearer {}", self.token)
        }
    }
}

impl PipelineClient for BitbucketPipelineClient {
    fn list(&self, owner: &str, repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>> {
        let mut url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pipelines/?sort=-created_on&pagelen={}",
            owner, repo, filters.per_page.clamp(1, 100)
        );
        if let Some(ref s) = filters.status {
            // Bitbucket: PENDING / IN_PROGRESS / SUCCESSFUL / FAILED /
            // STOPPED / PAUSED / HALTED.
            let bb = match s.as_str() {
                "success"  => "SUCCESSFUL",
                "failed"   => "FAILED",
                "running"  => "IN_PROGRESS",
                "canceled" => "STOPPED",
                "pending"  => "PENDING",
                other      => other,
            };
            url.push_str(&format!("&status={}", bb));
        }
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Bitbucket (url: {})", url))?;
        let arr = json["values"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Bitbucket returned no `values` array. Body: {}", json
            )))?;
        arr.iter().map(parse_bitbucket_pipeline).collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pipelines/{}/stopPipeline",
            owner, repo, id
        );
        let req = self.client().post(&url).header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Bitbucket cancel pipeline")
    }

    fn retry(&self, _o: &str, _r: &str, _id: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Bitbucket Pipelines doesn't expose a retry endpoint. Resubmit by pushing a \
             new commit or triggering a custom pipeline via the web UI.".to_string()
        ))
    }

    fn delete(&self, _o: &str, _r: &str, _id: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Bitbucket Pipelines doesn't allow deleting pipeline runs — they're \
             retained per the workspace's data-retention policy.".to_string()
        ))
    }

    fn list_jobs(&self, owner: &str, repo: &str, pipeline_id: &str, status_filter: Option<&str>) -> Result<Vec<Job>> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pipelines/{}/steps/?pagelen=100",
            owner, repo, pipeline_id
        );
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Bitbucket (url: {})", url))?;
        let arr = json["values"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Bitbucket returned no `values` array. Body: {}", json
            )))?;
        let mut jobs: Vec<Job> = arr.iter()
            .filter_map(|v| parse_bitbucket_step(v, pipeline_id).ok())
            .collect();
        if let Some(s) = status_filter {
            jobs.retain(|j| j.status == s);
        }
        Ok(jobs)
    }

    fn job_log(&self, owner: &str, repo: &str, job_id: &str) -> Result<String> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pipelines/{}/log",
            owner, repo, job_id
        );
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        crate::http::send_text(req, "Bitbucket pipeline log")
    }

    fn job_retry(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Bitbucket Pipelines has no per-step retry — resubmit the whole pipeline.".to_string()
        ))
    }

    fn job_cancel(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        self.cancel(owner, repo, job_id)
    }

    fn job_artifacts_download(&self, _o: &str, _r: &str, _j: &str, _p: &std::path::Path) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Bitbucket Pipelines artifact download isn't exposed cleanly by REST. \
             Fetch the artifact from the web UI.".to_string()
        ))
    }

    fn job_erase(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Bitbucket Pipelines has no log-erase operation.".to_string()
        ))
    }
}

fn parse_bitbucket_pipeline(v: &serde_json::Value) -> Result<Pipeline> {
    let state_name = v["state"]["name"].as_str().unwrap_or("");
    let result_name = v["state"]["result"]["name"].as_str().unwrap_or("");
    let raw = if !result_name.is_empty() {
        format!("{} ({})", state_name, result_name)
    } else {
        state_name.to_string()
    };
    let normalized = match (state_name, result_name) {
        ("COMPLETED", "SUCCESSFUL")   => "success",
        ("COMPLETED", "FAILED")       => "failed",
        ("COMPLETED", "STOPPED")      => "canceled",
        ("IN_PROGRESS", _)            => "running",
        ("PENDING", _)                => "pending",
        ("PAUSED", _) | ("HALTED", _) => "pending",
        _                             => "other",
    }.to_string();
    let id = v["uuid"].as_str().unwrap_or("")
        .trim_matches(|c| c == '{' || c == '}')
        .to_string();
    Ok(Pipeline {
        id: id.clone(),
        status: normalized,
        raw_status: raw,
        branch:     v["target"]["ref_name"].as_str().unwrap_or("").to_string(),
        sha:        v["target"]["commit"]["hash"].as_str().unwrap_or("").to_string(),
        web_url:    format!(
            "https://bitbucket.org/{}/{}/pipelines/results/{}",
            v["repository"]["workspace"]["slug"].as_str().unwrap_or(""),
            v["repository"]["name"].as_str().unwrap_or(""),
            v["build_number"].as_u64().unwrap_or(0)
        ),
        created_at: v["created_on"].as_str().unwrap_or("").to_string(),
        updated_at: v["completed_on"].as_str()
                        .or_else(|| v["created_on"].as_str())
                        .unwrap_or("").to_string(),
    })
}

fn parse_bitbucket_step(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let state_name = v["state"]["name"].as_str().unwrap_or("");
    let result_name = v["state"]["result"]["name"].as_str().unwrap_or("");
    let raw = if !result_name.is_empty() {
        format!("{} ({})", state_name, result_name)
    } else {
        state_name.to_string()
    };
    let normalized = match (state_name, result_name) {
        ("COMPLETED", "SUCCESSFUL") => "success",
        ("COMPLETED", "FAILED")     => "failed",
        ("COMPLETED", "STOPPED")    => "canceled",
        ("IN_PROGRESS", _)          => "running",
        ("PENDING", _)              => "pending",
        _                           => "other",
    }.to_string();
    let id = v["uuid"].as_str().unwrap_or("")
        .trim_matches(|c| c == '{' || c == '}')
        .to_string();
    Ok(Job {
        id:               id.clone(),
        pipeline_id:      pipeline_id.to_string(),
        name:             v["name"].as_str().unwrap_or("").to_string(),
        status:           normalized,
        raw_status:       raw,
        stage:            String::new(),
        web_url:          String::new(),
        created_at:       v["started_on"].as_str().unwrap_or("").to_string(),
        finished_at:      v["completed_on"].as_str().map(String::from),
        duration_seconds: v["duration_in_seconds"].as_f64(),
    })
}

// ============================================================================
// Factory
// ============================================================================

// ============================================================================
// Azure DevOps Pipelines (Builds API)
// ============================================================================
//
// Azure has two related surfaces here: the older "Build" definitions
// (`_apis/build/builds`) and the newer "Pipelines" runs
// (`_apis/pipelines/{id}/runs`). The Build API covers both — every
// pipeline run shows up there with a richer set of operations
// (cancel via PATCH, delete, logs) — so we use it.
//
// Build cancel happens by PATCHing `status: "cancelling"`. Retry isn't
// a direct endpoint; we POST a new build using the same
// `definition.id` (sourceBranch is filled from the original build).

pub struct AzurePipelineClient {
    token: String,
}

impl AzurePipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("azure", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "Azure DevOps PAT not found. Create at https://dev.azure.com/{org}/_usersSettings/tokens \
                 with `Build (read/execute)` scope, then: torii auth set azure YOUR_PAT".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client { crate::http::make_client() }

    fn auth_header(&self) -> String {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(format!(":{}", self.token));
        format!("Basic {}", b64)
    }
}

impl PipelineClient for AzurePipelineClient {
    fn list(&self, owner: &str, repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        // Azure filters by status + result. status: notStarted, in
        // progress, completed; result: succeeded, partiallySucceeded,
        // failed, canceled. Map ours to those.
        let mut params = vec![
            format!("$top={}", filters.per_page.clamp(1, 100)),
            "queryOrder=finishTimeDescending".to_string(),
            // Azure filters builds by repository name — useful since
            // builds live at the project level.
            format!("repositoryId={}", repo),
            "repositoryType=TfsGit".to_string(),
        ];
        if let Some(ref s) = filters.status {
            match s.as_str() {
                "success"  => { params.push("resultFilter=succeeded".into()); params.push("statusFilter=completed".into()); }
                "failed"   => { params.push("resultFilter=failed".into()); params.push("statusFilter=completed".into()); }
                "canceled" => { params.push("resultFilter=canceled".into()); params.push("statusFilter=completed".into()); }
                "running"  => { params.push("statusFilter=inProgress".into()); }
                "pending"  => { params.push("statusFilter=notStarted".into()); }
                _ => {}
            }
        }
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds?api-version=7.0&{}",
            org, project, params.join("&")
        );
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Azure (url: {})", url))?;
        let arr = json["value"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Azure returned no `value` array. Body: {}", json
            )))?;
        arr.iter().map(|v| parse_azure_build(v, &org, &project)).collect()
    }

    fn cancel(&self, owner: &str, _repo: &str, id: &str) -> Result<()> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}?api-version=7.0",
            org, project, id
        );
        let body = serde_json::json!({ "status": "cancelling" });
        let req = self.client().patch(&url)
            .header("Authorization", self.auth_header())
            .json(&body);
        crate::http::send_empty(req, "Azure cancel build")
    }

    fn retry(&self, owner: &str, _repo: &str, id: &str) -> Result<()> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        // "Retry" on Azure = POST a new build with the same
        // definition.id + sourceBranch. We look up the original first.
        let lookup_url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}?api-version=7.0",
            org, project, id
        );
        let lookup_req = self.client().get(&lookup_url).header("Authorization", self.auth_header());
        let original = crate::http::send_json(lookup_req, "Azure lookup build")?;
        let def_id = original["definition"]["id"].as_u64()
            .ok_or_else(|| ToriiError::InvalidConfig("Azure build has no definition.id".into()))?;
        let source_branch = original["sourceBranch"].as_str().unwrap_or("refs/heads/main").to_string();

        let post_url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds?api-version=7.0",
            org, project
        );
        let body = serde_json::json!({
            "definition":   { "id": def_id },
            "sourceBranch": source_branch,
        });
        let req = self.client().post(&post_url)
            .header("Authorization", self.auth_header())
            .json(&body);
        crate::http::send_empty(req, "Azure re-queue build")
    }

    fn delete(&self, owner: &str, _repo: &str, id: &str) -> Result<()> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}?api-version=7.0",
            org, project, id
        );
        let req = self.client().delete(&url).header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Azure delete build")
    }

    fn list_jobs(&self, owner: &str, _repo: &str, pipeline_id: &str, status_filter: Option<&str>) -> Result<Vec<Job>> {
        // Azure exposes the build timeline (jobs + tasks + phases) at
        // `/builds/{id}/timeline`. Each record has a `type` field; we
        // surface the `Job` records.
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}/timeline?api-version=7.0",
            org, project, pipeline_id
        );
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Azure (url: {})", url))?;
        let records = json["records"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Azure timeline missing `records`. Body: {}", json
            )))?;
        let mut jobs: Vec<Job> = records.iter()
            .filter(|r| r["type"].as_str() == Some("Job"))
            .filter_map(|v| parse_azure_timeline_job(v, pipeline_id).ok())
            .collect();
        if let Some(s) = status_filter {
            jobs.retain(|j| j.status == s);
        }
        Ok(jobs)
    }

    fn job_log(&self, owner: &str, _repo: &str, job_id: &str) -> Result<String> {
        // Azure's per-job log lives under the build's logs list. The
        // `job_id` we receive is actually the timeline-record id which
        // contains the log id in its `log.id` field — but to keep
        // signatures uniform we accept the timeline-record id and look
        // it up. For simplicity we just fetch all build logs by id;
        // callers can pass the build id as job_id (the timeline record
        // approach needs an extra round-trip we skip here).
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}/logs/0?api-version=7.0",
            org, project, job_id
        );
        let req = self.client().get(&url).header("Authorization", self.auth_header());
        crate::http::send_text(req, "Azure build log")
    }

    fn job_retry(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Azure Pipelines doesn't expose a per-job retry — re-queue the whole build \
             with `torii pipeline retry <build-id>`.".to_string()
        ))
    }

    fn job_cancel(&self, owner: &str, _repo: &str, job_id: &str) -> Result<()> {
        // Per-job cancel doesn't exist; fall back to the run-level
        // cancel using the same id (a torii caller may have passed the
        // build id by mistake — that still works).
        self.cancel(owner, "", job_id)
    }

    fn job_artifacts_download(&self, owner: &str, _repo: &str, job_id: &str, output_path: &std::path::Path) -> Result<()> {
        // Azure exposes a build-level artifacts collection. The
        // `job_id` here is interpreted as the build id; we download
        // the first artifact's zip and write it to disk.
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let list_url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}/artifacts?api-version=7.0",
            org, project, job_id
        );
        let list_req = self.client().get(&list_url).header("Authorization", self.auth_header());
        let list_json = crate::http::send_json(list_req, "Azure list artifacts")?;
        let first = list_json["value"].as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Azure build {} has no artifacts.", job_id
            )))?;
        let download_url = first["resource"]["downloadUrl"].as_str()
            .ok_or_else(|| ToriiError::InvalidConfig(
                "Azure artifact has no downloadUrl".to_string()
            ))?;
        let download_req = self.client().get(download_url).header("Authorization", self.auth_header());
        let bytes = crate::http::send_bytes(download_req, "Azure artifact")?;
        std::fs::write(output_path, &bytes)
            .map_err(|e| ToriiError::InvalidConfig(format!("Failed to write artifact to {}: {}", output_path.display(), e)))?;
        Ok(())
    }

    fn job_erase(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Azure Pipelines has no log-erase operation. Delete the whole build with \
             `torii pipeline delete <build-id>` if you need the logs gone.".to_string()
        ))
    }
}

fn parse_azure_build(v: &serde_json::Value, org: &str, project: &str) -> Result<Pipeline> {
    let id = v["id"].as_u64().map(|n| n.to_string()).unwrap_or_default();
    let status = v["status"].as_str().unwrap_or("");
    let result = v["result"].as_str().unwrap_or("");
    let normalized = match (status, result) {
        ("completed", "succeeded")           => "success",
        ("completed", "failed")              => "failed",
        ("completed", "partiallySucceeded")  => "failed",
        ("completed", "canceled")            => "canceled",
        ("inProgress", _)                    => "running",
        ("notStarted", _)                    => "pending",
        ("cancelling", _)                    => "canceled",
        _                                    => "other",
    }.to_string();
    let raw = if !result.is_empty() {
        format!("{} ({})", status, result)
    } else {
        status.to_string()
    };
    Ok(Pipeline {
        id: id.clone(),
        status: normalized,
        raw_status: raw,
        branch:     v["sourceBranch"].as_str().unwrap_or("").trim_start_matches("refs/heads/").to_string(),
        sha:        v["sourceVersion"].as_str().unwrap_or("").to_string(),
        web_url:    format!("https://dev.azure.com/{}/{}/_build/results?buildId={}", org, project, id),
        created_at: v["queueTime"].as_str()
                        .or_else(|| v["startTime"].as_str())
                        .unwrap_or("").to_string(),
        updated_at: v["finishTime"].as_str()
                        .or_else(|| v["queueTime"].as_str())
                        .unwrap_or("").to_string(),
    })
}

fn parse_azure_timeline_job(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let state = v["state"].as_str().unwrap_or("");
    let result = v["result"].as_str().unwrap_or("");
    let normalized = match (state, result) {
        ("completed", "succeeded")          => "success",
        ("completed", "failed")             => "failed",
        ("completed", "canceled")           => "canceled",
        ("completed", "partiallySucceeded") => "failed",
        ("inProgress", _)                   => "running",
        ("pending", _)                      => "pending",
        _                                   => "other",
    }.to_string();
    let raw = if !result.is_empty() {
        format!("{} ({})", state, result)
    } else {
        state.to_string()
    };
    let started  = v["startTime"].as_str().unwrap_or("");
    let finished = v["finishTime"].as_str().unwrap_or("");
    let duration_seconds = if !started.is_empty() && !finished.is_empty() {
        match (DateTime::parse_from_rfc3339(started), DateTime::parse_from_rfc3339(finished)) {
            (Ok(s), Ok(f)) => Some((f - s).num_seconds() as f64),
            _ => None,
        }
    } else { None };
    Ok(Job {
        id:               v["id"].as_str().unwrap_or("").to_string(),
        pipeline_id:      pipeline_id.to_string(),
        name:             v["name"].as_str().unwrap_or("").to_string(),
        status:           normalized,
        raw_status:       raw,
        stage:            v["parentId"].as_str().unwrap_or("").to_string(),
        web_url:          String::new(),
        created_at:       started.to_string(),
        finished_at:      v["finishTime"].as_str().map(String::from),
        duration_seconds,
    })
}

// ============================================================================
// Factory
// ============================================================================

pub fn get_pipeline_client(platform: &str) -> Result<Box<dyn PipelineClient>> {
    match platform.to_lowercase().as_str() {
        "github"    => Ok(Box::new(GitHubPipelineClient::new()?)),
        "gitlab"    => Ok(Box::new(GitLabPipelineClient::new()?)),
        "gitea"     => Ok(Box::new(GiteaPipelineClient::new()?)),
        "sourcehut" => Ok(Box::new(SourcehutPipelineClient::new()?)),
        "radicle"   => Ok(Box::new(RadiclePipelineClient::new()?)),
        "bitbucket" => Ok(Box::new(BitbucketPipelineClient::new()?)),
        "azure"     => Ok(Box::new(AzurePipelineClient::new()?)),
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported: github, gitlab, gitea, sourcehut, radicle, bitbucket, azure", other)
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
