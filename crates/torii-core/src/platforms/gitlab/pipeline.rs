//! GitLab — pipeline client.

use crate::error::{Result, ToriiError};
use crate::platforms::pipeline::*;
use reqwest::blocking::Client;

pub struct GitLabPipelineClient {
    token: String,
    base_url: String,
}

impl GitLabPipelineClient {
    /// Kept for parity with the other platform clients — the factory
    /// constructs GitLab via `new_with_base_url` (self-hosted support).
    #[allow(dead_code)]
    pub fn new() -> Result<Self> {
        Self::new_with_base_url(None)
    }

    /// 0.8.0 — construct against a custom GitLab API base URL
    /// (self-hosted instances declared in `platforms.toml`). `None`
    /// falls back to `GITLAB_URL` env or `https://gitlab.com/api/v4`.
    pub fn new_with_base_url(base_url: Option<&str>) -> Result<Self> {
        let token = crate::auth::resolve_token("gitlab", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "gitlab".into(),
                message: "GitLab token not found. Run: torii auth set gitlab YOUR_TOKEN"
                    .to_string(),
            })?;
        let resolved = base_url
            .map(|s| s.trim_end_matches('/').to_string())
            .or_else(|| std::env::var("GITLAB_URL").ok())
            .unwrap_or_else(|| "https://gitlab.com/api/v4".to_string());
        Ok(Self {
            token,
            base_url: resolved,
        })
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
            self.base_url,
            Self::project_path(owner, repo),
            filters.per_page.clamp(1, 100)
        );
        if let Some(ref s) = filters.status {
            let gl = s.as_str();
            url.push_str(&format!("&status={}", gl));
        }
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_gitlab_pipeline)
            .collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}/cancel",
            self.base_url,
            Self::project_path(owner, repo),
            id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab cancel pipeline")
    }

    fn retry(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}/retry",
            self.base_url,
            Self::project_path(owner, repo),
            id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab retry pipeline")
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/pipelines/{}",
            self.base_url,
            Self::project_path(owner, repo),
            id
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab delete pipeline")
    }

    // ---- job ops on GitLab Pipelines ----

    fn list_jobs(
        &self,
        owner: &str,
        repo: &str,
        pipeline_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<Job>> {
        // GitLab supports `?scope[]=failed&scope[]=success` for server-side
        // filtering, but a single client-side filter is simpler and
        // doesn't risk an empty array because of a typo in the scope name.
        let url = format!(
            "{}/projects/{}/pipelines/{}/jobs?per_page=100",
            self.base_url,
            Self::project_path(owner, repo),
            pipeline_id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        let arr = crate::http::extract_array(&json, &url)?;
        let jobs: Vec<Job> = arr
            .iter()
            .filter_map(|v| parse_gitlab_job(v, pipeline_id).ok())
            .collect();
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
            self.base_url,
            Self::project_path(owner, repo),
            job_id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_text(req, "GitLab job trace")
    }

    fn job_retry(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/jobs/{}/retry",
            self.base_url,
            Self::project_path(owner, repo),
            job_id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab job retry")
    }

    fn job_cancel(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/jobs/{}/cancel",
            self.base_url,
            Self::project_path(owner, repo),
            job_id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab job cancel")
    }

    fn job_artifacts_download(
        &self,
        owner: &str,
        repo: &str,
        job_id: &str,
        output_path: &std::path::Path,
    ) -> Result<()> {
        let url = format!(
            "{}/projects/{}/jobs/{}/artifacts",
            self.base_url,
            Self::project_path(owner, repo),
            job_id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let bytes = crate::http::send_bytes(req, "GitLab artifacts")?;
        std::fs::write(output_path, &bytes).map_err(|e| {
            ToriiError::Fs(format!(
                "Failed to write artifacts to {}: {}",
                output_path.display(),
                e
            ))
        })?;
        Ok(())
    }

    fn job_erase(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/jobs/{}/erase",
            self.base_url,
            Self::project_path(owner, repo),
            job_id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab job erase")
    }
}

fn parse_gitlab_job(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let id = v["id"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "gitlab".into(),
            message: "GitLab job missing id".into(),
        })?;
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let status = match raw_status.as_str() {
        "success" => "success",
        "failed" => "failed",
        "running" | "preparing" | "waiting_for_resource" => "running",
        "canceled" | "cancelled" => "canceled",
        "pending" | "scheduled" | "created" | "manual" => "pending",
        "skipped" => "canceled",
        _ => "other",
    }
    .to_string();
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

pub(crate) fn parse_gitlab_pipeline(v: &serde_json::Value) -> Result<Pipeline> {
    let id = v["id"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "gitlab".into(),
            message: "GitLab pipeline missing id".into(),
        })?;
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    let status = match raw_status.as_str() {
        "success" => "success",
        "failed" => "failed",
        "running" | "preparing" | "waiting_for_resource" => "running",
        "canceled" | "cancelled" => "canceled",
        "pending" | "scheduled" | "created" | "manual" => "pending",
        _ => "other",
    }
    .to_string();
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

#[cfg(test)]
mod tests {
    // NOTE: `parse_gitlab_pipeline` is already covered by the tests in
    // `src/platforms/pipeline.rs` — only the job parser and the HTTP
    // client are tested here.
    use super::*;
    use httpmock::prelude::*;

    // ── parser (jobs) ────────────────────────────────────────────────────

    #[test]
    fn parse_gitlab_job_full() {
        let json = serde_json::json!({
            "id": 555u64,
            "status": "failed",
            "name": "build-linux",
            "stage": "build",
            "web_url": "https://gitlab.com/acme/widget/-/jobs/555",
            "created_at": "2026-06-01T10:00:00Z",
            "finished_at": "2026-06-01T10:05:00Z",
            "duration": 300.5
        });
        let j = parse_gitlab_job(&json, "99").unwrap();
        assert_eq!(j.id, "555");
        assert_eq!(j.pipeline_id, "99");
        assert_eq!(j.name, "build-linux");
        assert_eq!(j.status, "failed");
        assert_eq!(j.raw_status, "failed");
        assert_eq!(j.stage, "build");
        assert_eq!(j.web_url, "https://gitlab.com/acme/widget/-/jobs/555");
        assert_eq!(j.finished_at.as_deref(), Some("2026-06-01T10:05:00Z"));
        assert_eq!(j.duration_seconds, Some(300.5));
    }

    #[test]
    fn parse_gitlab_job_normalizes_statuses() {
        for (raw, want) in [
            ("waiting_for_resource", "running"),
            ("manual", "pending"),
            ("skipped", "canceled"),
            ("cancelled", "canceled"),
            ("something_new", "other"),
        ] {
            let json = serde_json::json!({ "id": 1u64, "status": raw });
            let j = parse_gitlab_job(&json, "1").unwrap();
            assert_eq!(j.status, want, "raw status `{raw}`");
            assert_eq!(j.raw_status, raw);
            assert_eq!(j.finished_at, None);
            assert_eq!(j.duration_seconds, None);
        }
    }

    #[test]
    fn parse_gitlab_job_missing_id_is_malformed_response() {
        let err = parse_gitlab_job(&serde_json::json!({ "status": "failed" }), "1").unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { .. }),
            "expected MalformedResponse, got: {err:?}"
        );
    }

    // ── client (httpmock) ────────────────────────────────────────────────

    fn client(server: &MockServer) -> GitLabPipelineClient {
        GitLabPipelineClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn list_passes_status_and_per_page_filters() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/pipelines")
                .query_param("per_page", "5")
                .query_param("status", "failed")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([{
                "id": 42u64, "status": "failed", "ref": "main", "sha": "abc",
                "web_url": "https://x", "created_at": "", "updated_at": ""
            }]));
        });
        let filters = ListFilters {
            status: Some("failed".into()),
            per_page: 5,
        };
        let pipelines = client(&server).list("acme", "widget", &filters).unwrap();
        m.assert();
        assert_eq!(pipelines.len(), 1);
        assert_eq!(pipelines[0].id, "42");
        assert_eq!(pipelines[0].status, "failed");
    }

    #[test]
    fn list_jobs_filters_by_normalized_status_client_side() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/pipelines/99/jobs")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([
                { "id": 1u64, "status": "failed",  "name": "a" },
                { "id": 2u64, "status": "success", "name": "b" }
            ]));
        });
        let c = client(&server);
        let failed = c.list_jobs("acme", "widget", "99", Some("failed")).unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].id, "1");
        let all = c.list_jobs("acme", "widget", "99", None).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn cancel_posts_with_bearer_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/projects/acme%2Fwidget/pipelines/9/cancel")
                .header("Authorization", "Bearer test-token");
            then.status(200);
        });
        client(&server).cancel("acme", "widget", "9").unwrap();
        m.assert();
    }

    #[test]
    fn job_log_returns_raw_trace_text() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/jobs/555/trace");
            then.status(200).body("line one\nline two");
        });
        let log = client(&server).job_log("acme", "widget", "555").unwrap();
        assert_eq!(log, "line one\nline two");
    }

    #[test]
    fn retry_non_2xx_maps_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/projects/acme%2Fwidget/pipelines/9/retry");
            then.status(403)
                .json_body(serde_json::json!({ "message": "403 Forbidden" }));
        });
        let err = client(&server).retry("acme", "widget", "9").unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
