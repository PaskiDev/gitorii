//! GitHub — pipeline client.

use crate::error::{Result, ToriiError};
use crate::platforms::pipeline::*;
use reqwest::blocking::Client;

pub struct GitHubPipelineClient {
    token: String,
    base_url: String,
}

impl GitHubPipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("github", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "github".into(),
                message: "GitHub token not found. Run: torii auth set github YOUR_TOKEN"
                    .to_string(),
            })?;
        Ok(Self {
            token,
            base_url: "https://api.github.com".to_string(),
        })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }

    fn auth_header(&self) -> String {
        format!("token {}", self.token)
    }
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
            "{}/repos/{}/{}/actions/runs?per_page={}",
            self.base_url,
            owner,
            repo,
            filters.per_page.clamp(1, 100)
        );
        if let Some(ref s) = filters.status {
            let gh = match s.as_str() {
                "success" => "success",
                "failed" => "failure",
                "running" => "in_progress",
                "canceled" => "cancelled",
                "pending" => "queued",
                other => other,
            };
            url.push_str(&format!("&status={}", gh));
        }
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        let arr =
            json["workflow_runs"]
                .as_array()
                .ok_or_else(|| ToriiError::MalformedResponse {
                    provider: "github".into(),
                    message: format!("GitHub returned no workflow_runs array. Body: {}", json),
                })?;
        arr.iter().map(parse_github_run).collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}/cancel",
            self.base_url, owner, repo, id
        );
        post_no_body(&self.client(), &url, &self.auth_header(), "cancel")
    }

    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}/rerun",
            self.base_url, owner, repo, id
        );
        post_no_body(&self.client(), &url, &self.auth_header(), "retry")
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}",
            self.base_url, owner, repo, id
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json");
        crate::http::send_empty(req, "GitHub delete run")
    }

    // ---- job ops on GitHub Actions ----

    fn list_jobs(
        &self,
        owner: &str,
        repo: &str,
        pipeline_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<Job>> {
        // GitHub Actions: "jobs in a workflow run". The `filter` query
        // param accepts `latest` | `all`; per-status filtering happens
        // client-side after the fetch.
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}/jobs?per_page=100",
            self.base_url, owner, repo, pipeline_id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        let arr = json["jobs"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "github".into(),
                message: format!("GitHub returned no `jobs` array. Body: {}", json),
            })?;
        let jobs: Vec<Job> = arr
            .iter()
            .filter_map(|v| parse_github_job(v, pipeline_id).ok())
            .collect();
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
            "{}/repos/{}/{}/actions/jobs/{}/logs",
            self.base_url, owner, repo, job_id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github+json");
        crate::http::send_text(req, "GitHub job log")
    }

    fn job_retry(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        // GitHub Actions has no per-job retry — only `/runs/:run_id/rerun`
        // and `/runs/:run_id/rerun-failed-jobs`. Both operate at the run
        // level. Point the user at `torii pipeline retry <run-id>` so
        // the CLI surface stays honest.
        Err(ToriiError::Unsupported("GitHub Actions doesn't support per-job retry. Use `torii pipeline retry <run-id>` to re-run failed jobs in a workflow run.".to_string()))
    }

    fn job_cancel(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        Err(ToriiError::Unsupported("GitHub Actions doesn't support per-job cancel. Use `torii pipeline cancel <run-id>` to stop a workflow run.".to_string()))
    }

    fn job_artifacts_download(
        &self,
        _owner: &str,
        _repo: &str,
        _job_id: &str,
        _output_path: &std::path::Path,
    ) -> Result<()> {
        Err(ToriiError::Unsupported("GitHub Actions artifacts are scoped to the workflow run, not the job. List artifacts with `torii pipeline list` then use the GitHub UI / API directly until torii adds per-run artifact download.".to_string()))
    }

    fn job_erase(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        // GitLab-only operation; GitHub Actions doesn't expose log-erase.
        Err(ToriiError::Unsupported("GitHub Actions doesn't support per-job log erase. Logs are retained for the run lifetime; use `torii pipeline delete <run-id>` to discard the run entirely.".to_string()))
    }
}

fn parse_github_job(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let id = v["id"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: "GitHub job missing id".into(),
        })?;
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let conclusion = v["conclusion"].as_str().unwrap_or("");
    let label = if raw_status == "completed" && !conclusion.is_empty() {
        conclusion.to_string()
    } else {
        raw_status.clone()
    };
    let status = match raw_status.as_str() {
        "queued" => "pending".to_string(),
        "in_progress" => "running".to_string(),
        "completed" => match conclusion {
            "success" => "success",
            "failure" | "timed_out" => "failed",
            "cancelled" => "canceled",
            _ => "other",
        }
        .to_string(),
        _ => "other".to_string(),
    };
    // GitHub job duration = finished_at - started_at if both set.
    let started_at = v["started_at"].as_str();
    let finished_at = v["completed_at"].as_str();
    let duration = match (started_at, finished_at) {
        (Some(s), Some(f)) => {
            use chrono::DateTime;
            match (
                DateTime::parse_from_rfc3339(s),
                DateTime::parse_from_rfc3339(f),
            ) {
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

pub(crate) fn parse_github_run(v: &serde_json::Value) -> Result<Pipeline> {
    let id = v["id"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: "GitHub run missing id".into(),
        })?;
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let conclusion = v["conclusion"].as_str().unwrap_or("");
    let label = if raw_status == "completed" && !conclusion.is_empty() {
        conclusion.to_string()
    } else {
        raw_status.clone()
    };
    let status = match raw_status.as_str() {
        "queued" => "pending".to_string(),
        "in_progress" => "running".to_string(),
        "completed" => match conclusion {
            "success" => "success",
            "failure" | "timed_out" => "failed",
            "cancelled" => "canceled",
            _ => "other",
        }
        .to_string(),
        _ => "other".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    // Note: parse_github_run's completed/failure and in_progress mappings
    // are already covered in src/platforms/pipeline.rs — the tests here
    // cover only the cases those don't (missing id, queued, jobs).

    fn client_for(server: &MockServer) -> GitHubPipelineClient {
        GitHubPipelineClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn parse_github_run_missing_id_is_malformed() {
        let err = parse_github_run(&serde_json::json!({ "status": "queued" })).unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { .. }),
            "expected MalformedResponse, got: {err:?}"
        );
    }

    #[test]
    fn parse_github_run_queued_normalizes_to_pending() {
        let json = serde_json::json!({ "id": 9u64, "status": "queued" });
        let p = parse_github_run(&json).unwrap();
        assert_eq!(p.id, "9");
        assert_eq!(p.status, "pending");
        assert_eq!(p.raw_status, "queued");
        assert_eq!(p.branch, "");
    }

    #[test]
    fn parse_github_job_completed_success_with_duration() {
        let json = serde_json::json!({
            "id": 111u64,
            "name": "build",
            "status": "completed",
            "conclusion": "success",
            "html_url": "https://x/job/111",
            "created_at": "2026-01-01T00:00:00Z",
            "started_at": "2026-01-01T00:00:10Z",
            "completed_at": "2026-01-01T00:01:40Z",
        });
        let job = parse_github_job(&json, "555").unwrap();
        assert_eq!(job.id, "111");
        assert_eq!(job.pipeline_id, "555");
        assert_eq!(job.name, "build");
        assert_eq!(job.status, "success");
        assert_eq!(job.raw_status, "success");
        assert_eq!(job.stage, "");
        assert_eq!(job.web_url, "https://x/job/111");
        assert_eq!(job.finished_at.as_deref(), Some("2026-01-01T00:01:40Z"));
        assert_eq!(job.duration_seconds, Some(90.0));
    }

    #[test]
    fn parse_github_job_in_progress_has_no_duration() {
        let json = serde_json::json!({
            "id": 1u64, "name": "test", "status": "in_progress",
            "started_at": "2026-01-01T00:00:00Z",
        });
        let job = parse_github_job(&json, "p").unwrap();
        assert_eq!(job.status, "running");
        assert_eq!(job.raw_status, "in_progress");
        assert_eq!(job.finished_at, None);
        assert_eq!(job.duration_seconds, None);
    }

    #[test]
    fn parse_github_job_missing_id_is_malformed() {
        let err = parse_github_job(&serde_json::json!({ "status": "queued" }), "p").unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { .. }),
            "expected MalformedResponse, got: {err:?}"
        );
    }

    #[test]
    fn list_translates_status_filter_and_parses_runs() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/octo/demo/actions/runs")
                .query_param("per_page", "30")
                // torii's normalized "failed" → GitHub's "failure"
                .query_param("status", "failure")
                .header("Authorization", "token test-token");
            then.status(200).json_body(serde_json::json!({
                "workflow_runs": [{
                    "id": 1001u64,
                    "status": "completed",
                    "conclusion": "failure",
                    "head_branch": "main",
                    "head_sha": "abc123",
                    "html_url": "https://x/runs/1001",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:05:00Z",
                }]
            }));
        });
        let filters = ListFilters {
            status: Some("failed".into()),
            per_page: 30,
        };
        let runs = client_for(&server).list("octo", "demo", &filters).unwrap();
        m.assert();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "1001");
        assert_eq!(runs[0].status, "failed");
        assert_eq!(runs[0].branch, "main");
        assert_eq!(runs[0].sha, "abc123");
    }

    #[test]
    fn list_without_workflow_runs_array_is_malformed() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/repos/octo/demo/actions/runs");
            then.status(200)
                .json_body(serde_json::json!({ "total_count": 0 }));
        });
        let filters = ListFilters {
            status: None,
            per_page: 10,
        };
        let err = client_for(&server)
            .list("octo", "demo", &filters)
            .unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { .. }),
            "expected MalformedResponse, got: {err:?}"
        );
    }

    #[test]
    fn cancel_posts_to_cancel_endpoint_with_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/repos/octo/demo/actions/runs/99/cancel")
                .header("Authorization", "token test-token");
            then.status(202);
        });
        client_for(&server).cancel("octo", "demo", "99").unwrap();
        m.assert();
    }

    #[test]
    fn cancel_maps_500_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/repos/octo/demo/actions/runs/99/cancel");
            then.status(500).body("boom");
        });
        let err = client_for(&server)
            .cancel("octo", "demo", "99")
            .unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 500, .. }),
            "expected PlatformApi 500, got: {err:?}"
        );
    }

    #[test]
    fn list_jobs_filters_client_side_by_normalized_status() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/octo/demo/actions/runs/7/jobs")
                .header("Authorization", "token test-token");
            then.status(200).json_body(serde_json::json!({
                "jobs": [
                    { "id": 1u64, "name": "ok",   "status": "completed", "conclusion": "success" },
                    { "id": 2u64, "name": "boom", "status": "completed", "conclusion": "failure" },
                ]
            }));
        });
        let jobs = client_for(&server)
            .list_jobs("octo", "demo", "7", Some("failed"))
            .unwrap();
        m.assert();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, "2");
        assert_eq!(jobs[0].status, "failed");
        assert_eq!(jobs[0].pipeline_id, "7");
    }
}
