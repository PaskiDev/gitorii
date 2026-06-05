//! Radicle — release client.

use crate::error::{Result, ToriiError};
use crate::platforms::release::*;

pub struct RadicleReleaseClient;

impl RadicleReleaseClient {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

fn radicle_release_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Radicle has no Release-page object. A release on radicle is \
         just an annotated git tag (`torii tag create vX --release`); \
         binaries live outside the network. Mirror to GitLab/GitHub/Codeberg \
         if you need a hosted release page with notes + assets."
            .to_string(),
    )
}

impl ReleaseClient for RadicleReleaseClient {
    fn list(&self, _o: &str, _r: &str, _l: usize) -> Result<Vec<Release>> {
        Err(radicle_release_unsupported())
    }
    fn get(&self, _o: &str, _r: &str, _t: &str) -> Result<Release> {
        Err(radicle_release_unsupported())
    }
    fn edit(&self, _o: &str, _r: &str, _t: &str, _n: Option<&str>, _d: Option<&str>) -> Result<()> {
        Err(radicle_release_unsupported())
    }
    fn delete(&self, _o: &str, _r: &str, _t: &str) -> Result<()> {
        Err(radicle_release_unsupported())
    }
}

// ============================================================================
// Bitbucket Cloud (no Release object)
// ============================================================================
//
// Bitbucket Cloud doesn't have GitHub-style Release entities. It has
// "Downloads" (binary files attached to a repo, separate from tags)
// and tags. Most projects use Downloads via the web UI; we expose a
// clear error so the surface stays honest.
