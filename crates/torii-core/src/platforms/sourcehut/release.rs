//! Sourcehut — release client.

use crate::error::{Result, ToriiError};
use crate::platforms::release::*;

pub struct SourcehutReleaseClient;

impl SourcehutReleaseClient {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

fn srht_release_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Sourcehut has no Release-page object. A release on sourcehut \
         is just an annotated git tag (`torii tag create vX --release`); \
         binaries live outside the host. If you want listed releases \
         with notes + assets, mirror the project to GitLab/GitHub/Codeberg \
         and use the platform's native release API there."
            .to_string(),
    )
}

impl ReleaseClient for SourcehutReleaseClient {
    fn list(&self, _o: &str, _r: &str, _l: usize) -> Result<Vec<Release>> {
        Err(srht_release_unsupported())
    }
    fn get(&self, _o: &str, _r: &str, _t: &str) -> Result<Release> {
        Err(srht_release_unsupported())
    }
    fn edit(&self, _o: &str, _r: &str, _t: &str, _n: Option<&str>, _d: Option<&str>) -> Result<()> {
        Err(srht_release_unsupported())
    }
    fn delete(&self, _o: &str, _r: &str, _t: &str) -> Result<()> {
        Err(srht_release_unsupported())
    }
}

// ============================================================================
// Factory
// ============================================================================

// ============================================================================
// Radicle (peer-to-peer, no native release object)
// ============================================================================
//
// Radicle has no Release-page concept — same as Sourcehut. Annotated
// tags travel via the gossip protocol; binary distribution happens off
// the network (project's own website, IPFS, etc.).
