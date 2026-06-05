//! Radicle — pipeline client.

use crate::error::{Result, ToriiError};
use crate::platforms::pipeline::*;

pub struct RadiclePipelineClient;

impl RadiclePipelineClient {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

fn radicle_ci_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Radicle has no built-in CI. Mirror the project to a host with CI \
         (GitLab, GitHub, Codeberg, Sourcehut) and use that platform's \
         pipeline surface, or run CI locally / on your own runner."
            .to_string(),
    )
}

impl PipelineClient for RadiclePipelineClient {
    fn list(&self, _o: &str, _r: &str, _f: &ListFilters) -> Result<Vec<Pipeline>> {
        Err(radicle_ci_unsupported())
    }
    fn cancel(&self, _o: &str, _r: &str, _id: &str) -> Result<()> {
        Err(radicle_ci_unsupported())
    }
    fn retry(&self, _o: &str, _r: &str, _id: &str) -> Result<()> {
        Err(radicle_ci_unsupported())
    }
    fn delete(&self, _o: &str, _r: &str, _id: &str) -> Result<()> {
        Err(radicle_ci_unsupported())
    }
    fn list_jobs(&self, _o: &str, _r: &str, _p: &str, _f: Option<&str>) -> Result<Vec<Job>> {
        Err(radicle_ci_unsupported())
    }
    fn job_log(&self, _o: &str, _r: &str, _j: &str) -> Result<String> {
        Err(radicle_ci_unsupported())
    }
    fn job_retry(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(radicle_ci_unsupported())
    }
    fn job_cancel(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(radicle_ci_unsupported())
    }
    fn job_artifacts_download(
        &self,
        _o: &str,
        _r: &str,
        _j: &str,
        _p: &std::path::Path,
    ) -> Result<()> {
        Err(radicle_ci_unsupported())
    }
    fn job_erase(&self, _o: &str, _r: &str, _j: &str) -> Result<()> {
        Err(radicle_ci_unsupported())
    }
}

// ============================================================================
// Bitbucket Pipelines (REST v2)
// ============================================================================
//
// Bitbucket runs CI via pipelines + steps. Pipeline ≈ pipeline, step ≈
// job. UUIDs are the canonical identifiers (build_number works too).
// `retry` and `delete` aren't exposed via the public REST API — return
// clear errors.
