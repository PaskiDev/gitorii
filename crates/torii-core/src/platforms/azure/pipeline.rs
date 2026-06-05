//! Azure DevOps — pipeline client.

use crate::error::{Result, ToriiError};
use crate::platforms::pipeline::*;
use chrono::DateTime;
use reqwest::blocking::Client;

pub struct AzurePipelineClient {
    token: String,
}

impl AzurePipelineClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("azure", ".").value
            .ok_or_else(|| ToriiError::Auth { provider: "azure".into(), message: "Azure DevOps PAT not found. Create at https://dev.azure.com/{org}/_usersSettings/tokens \
                 with `Build (read/execute)` scope, then: torii auth set azure YOUR_PAT".to_string() })?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }

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
                "success" => {
                    params.push("resultFilter=succeeded".into());
                    params.push("statusFilter=completed".into());
                }
                "failed" => {
                    params.push("resultFilter=failed".into());
                    params.push("statusFilter=completed".into());
                }
                "canceled" => {
                    params.push("resultFilter=canceled".into());
                    params.push("statusFilter=completed".into());
                }
                "running" => {
                    params.push("statusFilter=inProgress".into());
                }
                "pending" => {
                    params.push("statusFilter=notStarted".into());
                }
                _ => {}
            }
        }
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds?api-version=7.0&{}",
            org,
            project,
            params.join("&")
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Azure (url: {})", url))?;
        let arr = json["value"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "azure".into(),
                message: format!("Azure returned no `value` array. Body: {}", json),
            })?;
        arr.iter()
            .map(|v| parse_azure_build(v, &org, &project))
            .collect()
    }

    fn cancel(&self, owner: &str, _repo: &str, id: &str) -> Result<()> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}?api-version=7.0",
            org, project, id
        );
        let body = serde_json::json!({ "status": "cancelling" });
        let req = self
            .client()
            .patch(&url)
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
        let lookup_req = self
            .client()
            .get(&lookup_url)
            .header("Authorization", self.auth_header());
        let original = crate::http::send_json(lookup_req, "Azure lookup build")?;
        let def_id =
            original["definition"]["id"]
                .as_u64()
                .ok_or_else(|| ToriiError::MalformedResponse {
                    provider: "azure".into(),
                    message: "Azure build has no definition.id".into(),
                })?;
        let source_branch = original["sourceBranch"]
            .as_str()
            .unwrap_or("refs/heads/main")
            .to_string();

        let post_url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds?api-version=7.0",
            org, project
        );
        let body = serde_json::json!({
            "definition":   { "id": def_id },
            "sourceBranch": source_branch,
        });
        let req = self
            .client()
            .post(&post_url)
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
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_empty(req, "Azure delete build")
    }

    fn list_jobs(
        &self,
        owner: &str,
        _repo: &str,
        pipeline_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<Job>> {
        // Azure exposes the build timeline (jobs + tasks + phases) at
        // `/builds/{id}/timeline`. Each record has a `type` field; we
        // surface the `Job` records.
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}/timeline?api-version=7.0",
            org, project, pipeline_id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        let json = crate::http::send_json(req, &format!("Azure (url: {})", url))?;
        let records = json["records"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "azure".into(),
                message: format!("Azure timeline missing `records`. Body: {}", json),
            })?;
        let mut jobs: Vec<Job> = records
            .iter()
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
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth_header());
        crate::http::send_text(req, "Azure build log")
    }

    fn job_retry(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Azure Pipelines doesn't expose a per-job retry — re-queue the whole build \
             with `torii pipeline retry <build-id>`."
                .to_string(),
        ))
    }

    fn job_cancel(&self, owner: &str, _repo: &str, job_id: &str) -> Result<()> {
        // Per-job cancel doesn't exist; fall back to the run-level
        // cancel using the same id (a torii caller may have passed the
        // build id by mistake — that still works).
        self.cancel(owner, "", job_id)
    }

    fn job_artifacts_download(
        &self,
        owner: &str,
        _repo: &str,
        job_id: &str,
        output_path: &std::path::Path,
    ) -> Result<()> {
        // Azure exposes a build-level artifacts collection. The
        // `job_id` here is interpreted as the build id; we download
        // the first artifact's zip and write it to disk.
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let list_url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}/artifacts?api-version=7.0",
            org, project, job_id
        );
        let list_req = self
            .client()
            .get(&list_url)
            .header("Authorization", self.auth_header());
        let list_json = crate::http::send_json(list_req, "Azure list artifacts")?;
        let first = list_json["value"]
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "azure".into(),
                message: format!("Azure build {} has no artifacts.", job_id),
            })?;
        let download_url = first["resource"]["downloadUrl"].as_str().ok_or_else(|| {
            ToriiError::MalformedResponse {
                provider: "azure".into(),
                message: "Azure artifact has no downloadUrl".to_string(),
            }
        })?;
        let download_req = self
            .client()
            .get(download_url)
            .header("Authorization", self.auth_header());
        let bytes = crate::http::send_bytes(download_req, "Azure artifact")?;
        std::fs::write(output_path, &bytes).map_err(|e| {
            ToriiError::Fs(format!(
                "Failed to write artifact to {}: {}",
                output_path.display(),
                e
            ))
        })?;
        Ok(())
    }

    fn job_erase(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Azure Pipelines has no log-erase operation. Delete the whole build with \
             `torii pipeline delete <build-id>` if you need the logs gone."
                .to_string(),
        ))
    }
}

fn parse_azure_build(v: &serde_json::Value, org: &str, project: &str) -> Result<Pipeline> {
    let id = v["id"].as_u64().map(|n| n.to_string()).unwrap_or_default();
    let status = v["status"].as_str().unwrap_or("");
    let result = v["result"].as_str().unwrap_or("");
    let normalized = match (status, result) {
        ("completed", "succeeded") => "success",
        ("completed", "failed") => "failed",
        ("completed", "partiallySucceeded") => "failed",
        ("completed", "canceled") => "canceled",
        ("inProgress", _) => "running",
        ("notStarted", _) => "pending",
        ("cancelling", _) => "canceled",
        _ => "other",
    }
    .to_string();
    let raw = if !result.is_empty() {
        format!("{} ({})", status, result)
    } else {
        status.to_string()
    };
    Ok(Pipeline {
        id: id.clone(),
        status: normalized,
        raw_status: raw,
        branch: v["sourceBranch"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches("refs/heads/")
            .to_string(),
        sha: v["sourceVersion"].as_str().unwrap_or("").to_string(),
        web_url: format!(
            "https://dev.azure.com/{}/{}/_build/results?buildId={}",
            org, project, id
        ),
        created_at: v["queueTime"]
            .as_str()
            .or_else(|| v["startTime"].as_str())
            .unwrap_or("")
            .to_string(),
        updated_at: v["finishTime"]
            .as_str()
            .or_else(|| v["queueTime"].as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn parse_azure_timeline_job(v: &serde_json::Value, pipeline_id: &str) -> Result<Job> {
    let state = v["state"].as_str().unwrap_or("");
    let result = v["result"].as_str().unwrap_or("");
    let normalized = match (state, result) {
        ("completed", "succeeded") => "success",
        ("completed", "failed") => "failed",
        ("completed", "canceled") => "canceled",
        ("completed", "partiallySucceeded") => "failed",
        ("inProgress", _) => "running",
        ("pending", _) => "pending",
        _ => "other",
    }
    .to_string();
    let raw = if !result.is_empty() {
        format!("{} ({})", state, result)
    } else {
        state.to_string()
    };
    let started = v["startTime"].as_str().unwrap_or("");
    let finished = v["finishTime"].as_str().unwrap_or("");
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
        id: v["id"].as_str().unwrap_or("").to_string(),
        pipeline_id: pipeline_id.to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        status: normalized,
        raw_status: raw,
        stage: v["parentId"].as_str().unwrap_or("").to_string(),
        web_url: String::new(),
        created_at: started.to_string(),
        finished_at: v["finishTime"].as_str().map(String::from),
        duration_seconds,
    })
}

// ============================================================================
// Factory
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_azure_build ─────────────────────────────────────────────

    #[test]
    fn parse_azure_build_completed_succeeded() {
        let v = serde_json::json!({
            "id": 99u64,
            "status": "completed",
            "result": "succeeded",
            "sourceBranch": "refs/heads/main",
            "sourceVersion": "abc123",
            "queueTime": "2026-01-01T00:00:00Z",
            "startTime": "2026-01-01T00:01:00Z",
            "finishTime": "2026-01-01T00:05:00Z",
        });
        let p = parse_azure_build(&v, "org", "proj").unwrap();
        assert_eq!(p.id, "99");
        assert_eq!(p.status, "success");
        assert_eq!(p.raw_status, "completed (succeeded)");
        assert_eq!(p.branch, "main");
        assert_eq!(p.sha, "abc123");
        assert_eq!(
            p.web_url,
            "https://dev.azure.com/org/proj/_build/results?buildId=99"
        );
        assert_eq!(p.created_at, "2026-01-01T00:00:00Z");
        assert_eq!(p.updated_at, "2026-01-01T00:05:00Z");
    }

    #[test]
    fn parse_azure_build_status_mapping() {
        for (status, result, ours) in [
            ("completed", "succeeded", "success"),
            ("completed", "failed", "failed"),
            ("completed", "partiallySucceeded", "failed"),
            ("completed", "canceled", "canceled"),
            ("inProgress", "", "running"),
            ("notStarted", "", "pending"),
            ("cancelling", "", "canceled"),
            ("postponed", "", "other"),
        ] {
            let v = serde_json::json!({ "status": status, "result": result });
            assert_eq!(
                parse_azure_build(&v, "org", "proj").unwrap().status,
                ours,
                "status={} result={}",
                status,
                result
            );
        }
    }

    #[test]
    fn parse_azure_build_raw_status_without_result() {
        // While running there's no `result` — raw status is just the
        // status, no parenthesised suffix.
        let v = serde_json::json!({ "status": "inProgress" });
        let p = parse_azure_build(&v, "org", "proj").unwrap();
        assert_eq!(p.raw_status, "inProgress");
    }

    #[test]
    fn parse_azure_build_timestamp_fallbacks() {
        // No queueTime → created_at falls back to startTime; no
        // finishTime → updated_at falls back to queueTime (empty here).
        let v = serde_json::json!({
            "id": 1u64,
            "status": "inProgress",
            "startTime": "2026-01-01T00:01:00Z",
        });
        let p = parse_azure_build(&v, "org", "proj").unwrap();
        assert_eq!(p.created_at, "2026-01-01T00:01:00Z");
        assert_eq!(p.updated_at, "");
    }

    // ── parse_azure_timeline_job ──────────────────────────────────────

    #[test]
    fn parse_azure_timeline_job_completed_with_duration() {
        let v = serde_json::json!({
            "id": "rec-1",
            "name": "Build job",
            "type": "Job",
            "state": "completed",
            "result": "succeeded",
            "parentId": "phase-1",
            "startTime": "2026-01-01T00:00:00Z",
            "finishTime": "2026-01-01T00:02:30Z",
        });
        let j = parse_azure_timeline_job(&v, "99").unwrap();
        assert_eq!(j.id, "rec-1");
        assert_eq!(j.pipeline_id, "99");
        assert_eq!(j.name, "Build job");
        assert_eq!(j.status, "success");
        assert_eq!(j.raw_status, "completed (succeeded)");
        assert_eq!(j.stage, "phase-1");
        assert_eq!(j.duration_seconds, Some(150.0));
        assert_eq!(j.finished_at.as_deref(), Some("2026-01-01T00:02:30Z"));
    }

    #[test]
    fn parse_azure_timeline_job_status_mapping() {
        for (state, result, ours) in [
            ("completed", "succeeded", "success"),
            ("completed", "failed", "failed"),
            ("completed", "canceled", "canceled"),
            ("completed", "partiallySucceeded", "failed"),
            ("inProgress", "", "running"),
            ("pending", "", "pending"),
            ("unknown", "", "other"),
        ] {
            let v = serde_json::json!({ "state": state, "result": result });
            assert_eq!(
                parse_azure_timeline_job(&v, "1").unwrap().status,
                ours,
                "state={} result={}",
                state,
                result
            );
        }
    }

    #[test]
    fn parse_azure_timeline_job_running_has_no_duration() {
        let v = serde_json::json!({
            "id": "rec-2",
            "state": "inProgress",
            "startTime": "2026-01-01T00:00:00Z",
        });
        let j = parse_azure_timeline_job(&v, "99").unwrap();
        assert_eq!(j.status, "running");
        assert_eq!(j.duration_seconds, None);
        assert_eq!(j.finished_at, None);
    }

    #[test]
    fn parse_azure_timeline_job_unparseable_timestamps_no_duration() {
        let v = serde_json::json!({
            "state": "completed",
            "result": "succeeded",
            "startTime": "not a date",
            "finishTime": "also not a date",
        });
        let j = parse_azure_timeline_job(&v, "99").unwrap();
        assert_eq!(j.duration_seconds, None);
    }
}
