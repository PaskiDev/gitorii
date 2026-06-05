//! Bitbucket Cloud — release client.

use crate::error::{Result, ToriiError};
use crate::platforms::release::*;

pub struct BitbucketReleaseClient;

impl BitbucketReleaseClient {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

fn bitbucket_release_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Bitbucket Cloud has no Release-page object. It exposes \
         'Downloads' (a flat file list, separate from tags, no notes) \
         which isn't equivalent. Use annotated tags + the Downloads tab \
         manually, or mirror to GitHub/GitLab/Codeberg for hosted releases."
            .to_string(),
    )
}

impl ReleaseClient for BitbucketReleaseClient {
    fn list(&self, _o: &str, _r: &str, _l: usize) -> Result<Vec<Release>> {
        Err(bitbucket_release_unsupported())
    }
    fn get(&self, _o: &str, _r: &str, _t: &str) -> Result<Release> {
        Err(bitbucket_release_unsupported())
    }
    fn edit(&self, _o: &str, _r: &str, _t: &str, _n: Option<&str>, _d: Option<&str>) -> Result<()> {
        Err(bitbucket_release_unsupported())
    }
    fn delete(&self, _o: &str, _r: &str, _t: &str) -> Result<()> {
        Err(bitbucket_release_unsupported())
    }
}

// ============================================================================
// Factory
// ============================================================================

// ============================================================================
// Azure DevOps Releases (classic Release Management — vsrm.dev.azure.com)
// ============================================================================
//
// Azure DevOps has two ways to release: the modern "Pipelines"
// YAML-defined stages (lives at dev.azure.com under Builds API) and
// the classic "Releases" service (lives at *vsrm.dev.azure.com*). The
// classic Releases service is what every legacy project uses; the new
// YAML stages live under the Pipelines API and aren't 1:1 with our
// Release abstraction. We wire the *classic* surface — list / get /
// delete only, edit isn't really a thing on releases (you edit the
// definition).
//
// Tag identifier: torii's `Release.tag` slot stores the release name
// (e.g. "Release-42"); the numeric id goes in `id` for the
// edit/delete paths.
