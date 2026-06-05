//! Sourcehut — pipeline client.

use crate::error::{Result, ToriiError};
use crate::platforms::pipeline::*;
use reqwest::blocking::Client;

pub struct SourcehutPipelineClient {
    token: String,
}

impl SourcehutPipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("sourcehut", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "sourcehut".into(),
                message:
                    "Sourcehut token not found. Generate one at https://meta.sr.ht/oauth and run: \
                 torii auth set sourcehut YOUR_TOKEN"
                        .to_string(),
            })?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
    fn auth(&self) -> String {
        format!("token {}", self.token)
    }
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
        let arr = json["results"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "sourcehut".into(),
                message: format!("Sourcehut returned no `results` array. Body: {}", json),
            })?;
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
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth());
        crate::http::send_empty(req, "Sourcehut cancel build")
    }

    fn retry(&self, _owner: &str, _repo: &str, id: &str) -> Result<()> {
        // builds.sr.ht allows resubmitting a job from its manifest. The
        // canonical endpoint is `/api/jobs/{id}/start`, but it only
        // works for jobs that haven't been started yet — for actually
        // failed jobs you have to POST a new job from the same manifest
        // via `/api/jobs`. That's not exposed today; point the user at
        // the web UI.
        Err(ToriiError::Unsupported(format!(
            "Sourcehut builds doesn't expose a retry endpoint for finished jobs. \
             Resubmit job #{} from the web UI (https://builds.sr.ht/~user/job/{}) \
             or POST the same manifest again via the API.",
            id, id
        )))
    }

    fn delete(&self, _owner: &str, _repo: &str, _id: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Sourcehut builds doesn't allow deleting jobs — they're \
             retained per the host's retention policy and aren't user-deletable."
                .to_string(),
        ))
    }

    fn list_jobs(
        &self,
        _owner: &str,
        _repo: &str,
        pipeline_id: &str,
        _status_filter: Option<&str>,
    ) -> Result<Vec<Job>> {
        // On sourcehut a "job" IS the pipeline. We return the same
        // record reshaped as a single Job so the CLI surface stays
        // uniform with GitLab/GitHub.
        let url = format!("https://builds.sr.ht/api/jobs/{}", pipeline_id);
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Sourcehut build #{}", pipeline_id))?;
        let pipeline = parse_sourcehut_build(&json)?;
        Ok(vec![Job {
            id: pipeline.id.clone(),
            pipeline_id: pipeline.id.clone(),
            name: json["note"]
                .as_str()
                .unwrap_or("(sourcehut job)")
                .to_string(),
            status: pipeline.status.clone(),
            raw_status: pipeline.raw_status.clone(),
            stage: "build".to_string(),
            web_url: pipeline.web_url.clone(),
            created_at: pipeline.created_at.clone(),
            finished_at: None,
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

    fn job_artifacts_download(
        &self,
        _owner: &str,
        _repo: &str,
        _job_id: &str,
        _output_path: &std::path::Path,
    ) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Sourcehut builds doesn't expose artifacts via the REST API. \
             The job manifest can declare `triggers` that upload to a \
             URL, but there's no per-job artifacts endpoint."
                .to_string(),
        ))
    }

    fn job_erase(&self, _owner: &str, _repo: &str, _job_id: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Sourcehut builds has no log-erase operation.".to_string(),
        ))
    }
}

fn parse_sourcehut_build(v: &serde_json::Value) -> Result<Pipeline> {
    let id = v["id"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .unwrap_or_default();
    let raw_status = v["status"].as_str().unwrap_or("").to_string();
    // builds.sr.ht statuses: pending, queued, running, success, failed,
    // cancelled, timeout. Normalize:
    let status = match raw_status.as_str() {
        "success" => "success",
        "failed" | "timeout" => "failed",
        "running" => "running",
        "cancelled" => "canceled",
        "pending" | "queued" => "pending",
        _ => "other",
    }
    .to_string();
    let owner = v["owner"]["canonical_name"].as_str().unwrap_or("");
    Ok(Pipeline {
        id: id.clone(),
        status,
        raw_status,
        // builds.sr.ht jobs aren't anchored to a single repo+branch in
        // the API response — these come from the manifest's `tags` if
        // the user set them.
        branch: v["tags"]
            .as_array()
            .and_then(|a| a.iter().filter_map(|v| v.as_str()).next().map(String::from))
            .unwrap_or_default(),
        sha: String::new(),
        web_url: format!("https://builds.sr.ht/{}/job/{}", owner, id),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sourcehut_build_full() {
        let v = serde_json::json!({
            "id": 1234u64,
            "status": "success",
            "owner": { "canonical_name": "~alice" },
            "tags": ["gitorii", "ci"],
            "created": "2026-01-01T00:00:00Z",
            "updated": "2026-01-01T00:10:00Z",
        });
        let p = parse_sourcehut_build(&v).unwrap();
        assert_eq!(p.id, "1234");
        assert_eq!(p.status, "success");
        assert_eq!(p.raw_status, "success");
        // First manifest tag stands in for the branch.
        assert_eq!(p.branch, "gitorii");
        assert_eq!(p.sha, "");
        assert_eq!(p.web_url, "https://builds.sr.ht/~alice/job/1234");
        assert_eq!(p.created_at, "2026-01-01T00:00:00Z");
        assert_eq!(p.updated_at, "2026-01-01T00:10:00Z");
    }

    #[test]
    fn parse_sourcehut_build_status_mapping() {
        for (srht, ours) in [
            ("success", "success"),
            ("failed", "failed"),
            ("timeout", "failed"),
            ("running", "running"),
            ("cancelled", "canceled"),
            ("pending", "pending"),
            ("queued", "pending"),
            ("weird", "other"),
        ] {
            let v = serde_json::json!({ "status": srht });
            let p = parse_sourcehut_build(&v).unwrap();
            assert_eq!(p.status, ours, "srht status {}", srht);
            assert_eq!(p.raw_status, srht);
        }
    }

    #[test]
    fn parse_sourcehut_build_string_id_fallback() {
        let v = serde_json::json!({ "id": "abc", "status": "running" });
        assert_eq!(parse_sourcehut_build(&v).unwrap().id, "abc");
    }

    #[test]
    fn parse_sourcehut_build_minimal_defaults() {
        let v = serde_json::json!({});
        let p = parse_sourcehut_build(&v).unwrap();
        assert_eq!(p.id, "");
        assert_eq!(p.status, "other");
        assert_eq!(p.branch, ""); // no tags → empty branch
        assert_eq!(p.created_at, "");
        assert_eq!(p.updated_at, "");
    }
}
