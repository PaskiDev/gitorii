//! Gitea / Codeberg / Forgejo — pipeline client.

use crate::error::{Result, ToriiError};
use crate::platforms::pipeline::*;
use chrono::DateTime;
use reqwest::blocking::Client;

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
        Ok(Self {
            token,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
    fn auth_header(&self) -> String {
        format!("token {}", self.token)
    }
}

impl PipelineClient for GiteaPipelineClient {
    fn list(&self, owner: &str, repo: &str, filters: &ListFilters) -> Result<Vec<Pipeline>> {
        let mut url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs?limit={}",
            self.base_url,
            owner,
            repo,
            filters.per_page.clamp(1, 50)
        );
        if let Some(ref s) = filters.status {
            // Gitea Actions matches GitHub's vocabulary.
            let g = match s.as_str() {
                "success" => "success",
                "failed" => "failure",
                "running" => "in_progress",
                "canceled" => "cancelled",
                "pending" => "queued",
                other => other,
            };
            url.push_str(&format!("&status={}", g));
        }
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        let json = crate::http::send_json(
            req,
            &format!("Gitea (url: {}) — Actions API requires Gitea >=1.19", url),
        )?;
        let arr =
            json["workflow_runs"]
                .as_array()
                .ok_or_else(|| ToriiError::MalformedResponse {
                    provider: "gitea".into(),
                    message: format!("Gitea returned no workflow_runs array. Body: {}", json),
                })?;
        arr.iter().map(parse_gitea_run).collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs/{}/cancel",
            self.base_url, owner, repo, id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Gitea cancel run")
    }

    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs/{}/rerun",
            self.base_url, owner, repo, id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Gitea retry run")
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs/{}",
            self.base_url, owner, repo, id
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Gitea delete run")
    }

    fn list_jobs(
        &self,
        owner: &str,
        repo: &str,
        pipeline_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<Job>> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/runs/{}/jobs",
            self.base_url, owner, repo, pipeline_id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Gitea (url: {})", url))?;
        let arr = json["jobs"]
            .as_array()
            .or_else(|| json.as_array())
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "gitea".into(),
                message: format!("Gitea returned no jobs array. Body: {}", json),
            })?;
        let mut jobs: Vec<Job> = arr
            .iter()
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
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_text(req, "Gitea job log")
    }

    fn job_retry(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/actions/jobs/{}/rerun",
            self.base_url, owner, repo, job_id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Gitea job retry")
    }

    fn job_cancel(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        // Gitea Actions exposes cancel at the run level, not per-job.
        // Direct callers should cancel the whole run instead.
        Err(ToriiError::Unsupported(
            "Gitea Actions cancels at run level — use `torii pipeline cancel <id>`".to_string(),
        ))
    }

    fn job_artifacts_download(
        &self,
        _owner: &str,
        _repo: &str,
        _job_id: &str,
        _output_path: &std::path::Path,
    ) -> Result<()> {
        Err(ToriiError::Unsupported("Gitea Actions: per-job artifact download not exposed by the v1 API. Fetch the run's artifact from the web UI.".to_string()))
    }

    fn job_erase(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        // GitLab-specific concept — Gitea doesn't model "erase trace +
        // artifacts but keep job row". Closest analog is deleting the
        // whole run.
        Err(ToriiError::Unsupported("Gitea Actions has no per-job erase. Delete the whole run with `torii pipeline delete <id>`.".to_string()))
    }
}

pub(crate) fn parse_gitea_run(v: &serde_json::Value) -> Result<Pipeline> {
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let conclusion = v["conclusion"].as_str().unwrap_or("");
    // Gitea mirrors GitHub's status/conclusion split for completed runs.
    let normalized = match (raw_status.as_str(), conclusion) {
        ("completed", "success") => "success",
        ("completed", "failure") => "failed",
        ("completed", "cancelled") => "canceled",
        ("in_progress", _) => "running",
        ("queued", _) => "pending",
        ("waiting", _) => "pending",
        (other, _) => other,
    }
    .to_string();
    let raw_display = if !conclusion.is_empty() {
        format!("{} ({})", raw_status, conclusion)
    } else {
        raw_status
    };
    Ok(Pipeline {
        id: v["id"]
            .as_u64()
            .map(|n| n.to_string())
            .or_else(|| v["id"].as_str().map(String::from))
            .unwrap_or_default(),
        status: normalized,
        raw_status: raw_display,
        branch: v["head_branch"].as_str().unwrap_or("").to_string(),
        sha: v["head_sha"].as_str().unwrap_or("").to_string(),
        web_url: v["html_url"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        updated_at: v["updated_at"].as_str().unwrap_or("").to_string(),
    })
}

fn parse_gitea_job(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let conclusion = v["conclusion"].as_str().unwrap_or("");
    let normalized = match (raw_status.as_str(), conclusion) {
        ("completed", "success") => "success",
        ("completed", "failure") => "failed",
        ("completed", "cancelled") => "canceled",
        ("in_progress", _) => "running",
        ("queued", _) => "pending",
        ("waiting", _) => "pending",
        (other, _) => other,
    }
    .to_string();
    let raw_display = if !conclusion.is_empty() {
        format!("{} ({})", raw_status, conclusion)
    } else {
        raw_status
    };
    let started = v["started_at"].as_str().unwrap_or("");
    let finished = v["completed_at"].as_str().unwrap_or("");
    let duration_seconds = if !started.is_empty() && !finished.is_empty() {
        match (
            DateTime::parse_from_rfc3339(started),
            DateTime::parse_from_rfc3339(finished),
        ) {
            (Ok(s), Ok(f)) => Some((f - s).num_seconds() as f64),
            _ => None,
        }
    } else {
        None
    };
    Ok(Job {
        id: v["id"]
            .as_u64()
            .map(|n| n.to_string())
            .or_else(|| v["id"].as_str().map(String::from))
            .unwrap_or_default(),
        pipeline_id: pipeline_id.to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        status: normalized,
        raw_status: raw_display,
        stage: v["workflow_name"].as_str().unwrap_or("").to_string(),
        web_url: v["html_url"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        finished_at: v["completed_at"].as_str().map(String::from),
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

// `parse_gitea_run` is exercised from src/platforms/pipeline.rs's test
// module — here we cover `parse_gitea_job` and the HTTP plumbing.
#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn client(server: &MockServer) -> GiteaPipelineClient {
        GiteaPipelineClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    fn run_json(id: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "status": "completed",
            "conclusion": "success",
            "head_branch": "main",
            "head_sha": "abc123",
            "html_url": "https://codeberg.org/o/r/actions/runs/1",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:05:00Z",
        })
    }

    #[test]
    fn parse_gitea_job_extracts_fields_and_computes_duration() {
        let json = serde_json::json!({
            "id": 88u64,
            "name": "build",
            "status": "completed",
            "conclusion": "success",
            "workflow_name": "ci",
            "html_url": "https://codeberg.org/o/r/actions/runs/1/jobs/88",
            "created_at": "2026-01-01T00:00:00Z",
            "started_at": "2026-01-01T00:00:10Z",
            "completed_at": "2026-01-01T00:01:40Z",
        });
        let job = parse_gitea_job(&json, "42").unwrap();
        assert_eq!(job.id, "88");
        assert_eq!(job.pipeline_id, "42");
        assert_eq!(job.name, "build");
        assert_eq!(job.status, "success");
        assert_eq!(job.raw_status, "completed (success)");
        assert_eq!(job.stage, "ci");
        assert_eq!(
            job.web_url,
            "https://codeberg.org/o/r/actions/runs/1/jobs/88"
        );
        assert_eq!(job.finished_at.as_deref(), Some("2026-01-01T00:01:40Z"));
        assert_eq!(job.duration_seconds, Some(90.0));
    }

    #[test]
    fn parse_gitea_job_handles_missing_timestamps_and_string_id() {
        let json = serde_json::json!({
            "id": "j-7",
            "name": "lint",
            "status": "queued",
        });
        let job = parse_gitea_job(&json, "1").unwrap();
        assert_eq!(job.id, "j-7");
        assert_eq!(job.status, "pending");
        assert_eq!(job.raw_status, "queued");
        assert_eq!(job.finished_at, None);
        assert_eq!(job.duration_seconds, None);
    }

    #[test]
    fn list_translates_status_filter_and_parses_workflow_runs() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/repos/owner/repo/actions/runs")
                .query_param("limit", "10")
                // torii's "failed" maps to Gitea's "failure".
                .query_param("status", "failure")
                .header("Authorization", "token test-token");
            then.status(200)
                .json_body(serde_json::json!({ "workflow_runs": [run_json(5)] }));
        });
        let filters = ListFilters {
            status: Some("failed".into()),
            per_page: 10,
        };
        let runs = client(&server).list("owner", "repo", &filters).unwrap();
        mock.assert();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "5");
        assert_eq!(runs[0].status, "success");
        assert_eq!(runs[0].branch, "main");
    }

    #[test]
    fn cancel_posts_to_cancel_endpoint_with_token_auth() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v1/repos/owner/repo/actions/runs/99/cancel")
                .header("Authorization", "token test-token");
            then.status(204);
        });
        client(&server).cancel("owner", "repo", "99").unwrap();
        mock.assert();
    }

    #[test]
    fn list_maps_non_2xx_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/repos/owner/repo/actions/runs");
            then.status(404)
                .json_body(serde_json::json!({ "message": "no Actions" }));
        });
        let filters = ListFilters {
            status: None,
            per_page: 10,
        };
        let err = client(&server).list("owner", "repo", &filters).unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 404, .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
