//! Bitbucket Cloud — pipeline client.

use crate::error::{Result, ToriiError};
use crate::platforms::pipeline::*;
use reqwest::blocking::Client;

pub struct BitbucketPipelineClient {
    token: String,
}

impl BitbucketPipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("bitbucket", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "bitbucket".into(),
                message: "Bitbucket token not found. Create an app password at \
                 https://bitbucket.org/account/settings/app-passwords/ \
                 and run: torii auth set bitbucket USERNAME:APP_PASSWORD"
                    .to_string(),
            })?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
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
                "success" => "SUCCESSFUL",
                "failed" => "FAILED",
                "running" => "IN_PROGRESS",
                "canceled" => "STOPPED",
                "pending" => "PENDING",
                other => other,
            };
            url.push_str(&format!("&status={}", bb));
        }
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Bitbucket (url: {})", url))?;
        let arr = json["values"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "bitbucket".into(),
                message: format!("Bitbucket returned no `values` array. Body: {}", json),
            })?;
        arr.iter().map(parse_bitbucket_pipeline).collect()
    }

    fn cancel(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pipelines/{}/stopPipeline",
            owner, repo, id
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Bitbucket cancel pipeline")
    }

    fn retry(&self, _o: &str, _r: &str, _id: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Bitbucket Pipelines doesn't expose a retry endpoint. Resubmit by pushing a \
             new commit or triggering a custom pipeline via the web UI."
                .to_string(),
        ))
    }

    fn delete(&self, _o: &str, _r: &str, _id: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Bitbucket Pipelines doesn't allow deleting pipeline runs — they're \
             retained per the workspace's data-retention policy."
                .to_string(),
        ))
    }

    fn list_jobs(
        &self,
        owner: &str,
        repo: &str,
        pipeline_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<Job>> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/pipelines/{}/steps/?pagelen=100",
            owner, repo, pipeline_id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Bitbucket (url: {})", url))?;
        let arr = json["values"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "bitbucket".into(),
                message: format!("Bitbucket returned no `values` array. Body: {}", json),
            })?;
        let mut jobs: Vec<Job> = arr
            .iter()
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
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_text(req, "Bitbucket pipeline log")
    }

    fn job_retry(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Bitbucket Pipelines has no per-step retry — resubmit the whole pipeline.".to_string(),
        ))
    }

    fn job_cancel(&self, owner: &str, repo: &str, job_id: &str) -> Result<()> {
        self.cancel(owner, repo, job_id)
    }

    fn job_artifacts_download(
        &self,
        _o: &str,
        _r: &str,
        _j: &str,
        _p: &std::path::Path,
    ) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Bitbucket Pipelines artifact download isn't exposed cleanly by REST. \
             Fetch the artifact from the web UI."
                .to_string(),
        ))
    }

    fn job_erase(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Bitbucket Pipelines has no log-erase operation.".to_string(),
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
        ("COMPLETED", "SUCCESSFUL") => "success",
        ("COMPLETED", "FAILED") => "failed",
        ("COMPLETED", "STOPPED") => "canceled",
        ("IN_PROGRESS", _) => "running",
        ("PENDING", _) => "pending",
        ("PAUSED", _) | ("HALTED", _) => "pending",
        _ => "other",
    }
    .to_string();
    let id = v["uuid"]
        .as_str()
        .unwrap_or("")
        .trim_matches(|c| c == '{' || c == '}')
        .to_string();
    Ok(Pipeline {
        id: id.clone(),
        status: normalized,
        raw_status: raw,
        branch: v["target"]["ref_name"].as_str().unwrap_or("").to_string(),
        sha: v["target"]["commit"]["hash"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        web_url: format!(
            "https://bitbucket.org/{}/{}/pipelines/results/{}",
            v["repository"]["workspace"]["slug"].as_str().unwrap_or(""),
            v["repository"]["name"].as_str().unwrap_or(""),
            v["build_number"].as_u64().unwrap_or(0)
        ),
        created_at: v["created_on"].as_str().unwrap_or("").to_string(),
        updated_at: v["completed_on"]
            .as_str()
            .or_else(|| v["created_on"].as_str())
            .unwrap_or("")
            .to_string(),
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
        ("COMPLETED", "FAILED") => "failed",
        ("COMPLETED", "STOPPED") => "canceled",
        ("IN_PROGRESS", _) => "running",
        ("PENDING", _) => "pending",
        _ => "other",
    }
    .to_string();
    let id = v["uuid"]
        .as_str()
        .unwrap_or("")
        .trim_matches(|c| c == '{' || c == '}')
        .to_string();
    Ok(Job {
        id: id.clone(),
        pipeline_id: pipeline_id.to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        status: normalized,
        raw_status: raw,
        stage: String::new(),
        web_url: String::new(),
        created_at: v["started_on"].as_str().unwrap_or("").to_string(),
        finished_at: v["completed_on"].as_str().map(String::from),
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

// The client's URLs are hardcoded to api.bitbucket.org, so only the
// parsing layer is testable without touching production code.
#[cfg(test)]
mod tests {
    use super::*;

    fn pipeline_json(state: &str, result: Option<&str>) -> serde_json::Value {
        let mut state_obj = serde_json::json!({ "name": state });
        if let Some(r) = result {
            state_obj["result"] = serde_json::json!({ "name": r });
        }
        serde_json::json!({
            "uuid": "{11111111-2222-3333-4444-555555555555}",
            "build_number": 42,
            "state": state_obj,
            "target": {
                "ref_name": "main",
                "commit": { "hash": "abc123def" },
            },
            "repository": {
                "name": "repo",
                "workspace": { "slug": "workspace" },
            },
            "created_on": "2026-01-01T10:00:00.000000+00:00",
            "completed_on": "2026-01-01T10:05:00.000000+00:00",
        })
    }

    fn step_json(state: &str, result: Option<&str>) -> serde_json::Value {
        let mut state_obj = serde_json::json!({ "name": state });
        if let Some(r) = result {
            state_obj["result"] = serde_json::json!({ "name": r });
        }
        serde_json::json!({
            "uuid": "{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}",
            "name": "Build and test",
            "state": state_obj,
            "started_on": "2026-01-01T10:00:30.000000+00:00",
            "completed_on": "2026-01-01T10:03:30.000000+00:00",
            "duration_in_seconds": 180,
        })
    }

    #[test]
    fn parse_bitbucket_pipeline_extracts_fields_and_normalizes_success() {
        let p = parse_bitbucket_pipeline(&pipeline_json("COMPLETED", Some("SUCCESSFUL"))).unwrap();
        // Braces are stripped from the uuid.
        assert_eq!(p.id, "11111111-2222-3333-4444-555555555555");
        assert_eq!(p.status, "success");
        assert_eq!(p.raw_status, "COMPLETED (SUCCESSFUL)");
        assert_eq!(p.branch, "main");
        assert_eq!(p.sha, "abc123def");
        // web_url is reconstructed from workspace slug + repo name + build number.
        assert_eq!(
            p.web_url,
            "https://bitbucket.org/workspace/repo/pipelines/results/42"
        );
        assert_eq!(p.created_at, "2026-01-01T10:00:00.000000+00:00");
        assert_eq!(p.updated_at, "2026-01-01T10:05:00.000000+00:00");
    }

    #[test]
    fn parse_bitbucket_pipeline_normalizes_terminal_and_active_states() {
        let cases = [
            (("COMPLETED", Some("FAILED")), "failed"),
            (("COMPLETED", Some("STOPPED")), "canceled"),
            (("IN_PROGRESS", None), "running"),
            (("PENDING", None), "pending"),
            (("PAUSED", None), "pending"),
            (("HALTED", None), "pending"),
            (("SOMETHING_NEW", None), "other"),
        ];
        for ((state, result), expected) in cases {
            let p = parse_bitbucket_pipeline(&pipeline_json(state, result)).unwrap();
            assert_eq!(p.status, expected, "state {state} / result {result:?}");
        }
        // Without a result, raw_status is the bare state name.
        let p = parse_bitbucket_pipeline(&pipeline_json("IN_PROGRESS", None)).unwrap();
        assert_eq!(p.raw_status, "IN_PROGRESS");
    }

    #[test]
    fn parse_bitbucket_pipeline_falls_back_to_created_on_when_not_completed() {
        let mut json = pipeline_json("IN_PROGRESS", None);
        json.as_object_mut().unwrap().remove("completed_on");
        let p = parse_bitbucket_pipeline(&json).unwrap();
        assert_eq!(p.updated_at, "2026-01-01T10:00:00.000000+00:00");
    }

    #[test]
    fn parse_bitbucket_step_extracts_fields() {
        let j =
            parse_bitbucket_step(&step_json("COMPLETED", Some("SUCCESSFUL")), "pipe-1").unwrap();
        assert_eq!(j.id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(j.pipeline_id, "pipe-1");
        assert_eq!(j.name, "Build and test");
        assert_eq!(j.status, "success");
        assert_eq!(j.raw_status, "COMPLETED (SUCCESSFUL)");
        assert_eq!(j.stage, "");
        assert_eq!(j.web_url, "");
        assert_eq!(j.created_at, "2026-01-01T10:00:30.000000+00:00");
        assert_eq!(
            j.finished_at.as_deref(),
            Some("2026-01-01T10:03:30.000000+00:00")
        );
        assert_eq!(j.duration_seconds, Some(180.0));
    }

    #[test]
    fn parse_bitbucket_step_handles_running_step_with_missing_optionals() {
        let json = serde_json::json!({
            "uuid": "{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}",
            "name": "Deploy",
            "state": { "name": "IN_PROGRESS" },
        });
        let j = parse_bitbucket_step(&json, "pipe-2").unwrap();
        assert_eq!(j.status, "running");
        assert_eq!(j.raw_status, "IN_PROGRESS");
        assert_eq!(j.created_at, "");
        assert_eq!(j.finished_at, None);
        assert_eq!(j.duration_seconds, None);
        // Unknown step states normalize to "other".
        let json = serde_json::json!({ "state": { "name": "PAUSED" } });
        assert_eq!(parse_bitbucket_step(&json, "p").unwrap().status, "other");
    }

    #[test]
    fn parses_steps_out_of_paginated_values_envelope() {
        // `list_jobs` reads Bitbucket's `{"values": [...]}` page shape.
        let page = serde_json::json!({
            "pagelen": 100,
            "values": [
                step_json("COMPLETED", Some("SUCCESSFUL")),
                step_json("COMPLETED", Some("FAILED")),
            ],
        });
        let jobs: Vec<Job> = page["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| parse_bitbucket_step(v, "pipe-3").unwrap())
            .collect();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].status, "success");
        assert_eq!(jobs[1].status, "failed");
    }
}
