//! Sourcehut — pr client.

use crate::error::{Result, ToriiError};
use crate::platforms::pr::*;

pub struct SourcehutPrClient;

impl SourcehutPrClient {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

fn srht_pr_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Sourcehut doesn't have server-side pull requests — \
         contributions are sent as `git format-patch` style emails to \
         the project's `*-devel@lists.sr.ht` mailing list. Use \
         `torii patch export <range>` to produce the .patch files and \
         mail them with `git send-email` (or your MUA). The maintainer \
         applies them with `torii patch apply`."
            .to_string(),
    )
}

impl PrClient for SourcehutPrClient {
    fn create(&self, _o: &str, _r: &str, _opts: CreatePrOptions) -> Result<PullRequest> {
        Err(srht_pr_unsupported())
    }
    fn list(&self, _o: &str, _r: &str, _state: &str) -> Result<Vec<PullRequest>> {
        Err(srht_pr_unsupported())
    }
    fn get(&self, _o: &str, _r: &str, _n: u64) -> Result<PullRequest> {
        Err(srht_pr_unsupported())
    }
    fn merge(&self, _o: &str, _r: &str, _n: u64, _m: MergeMethod) -> Result<()> {
        Err(srht_pr_unsupported())
    }
    fn close(&self, _o: &str, _r: &str, _n: u64) -> Result<()> {
        Err(srht_pr_unsupported())
    }
    fn update(&self, _o: &str, _r: &str, _n: u64, _opts: UpdatePrOptions) -> Result<()> {
        Err(srht_pr_unsupported())
    }
    fn delete_branch(&self, _o: &str, _r: &str, _b: &str) -> Result<()> {
        Err(srht_pr_unsupported())
    }
    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

// ============================================================================
// Radicle (peer-to-peer, via `rad patch` CLI)
// ============================================================================
//
// Radicle calls "pull requests" *patches*. They're stored as refs
// inside the project's collaborative space (`refs/cobs/xyz.radicle.patch`)
// and synchronised peer-to-peer. There is no HTTP API; everything goes
// through the local `rad` binary.
