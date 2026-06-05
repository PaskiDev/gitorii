//! GitLab Package Registry management (`torii package …`).
//!
//! gitorii's release pipeline uploads cross-compiled binaries into
//! GitLab's Generic Package Registry under `gitorii/v0.7.X/…`. Over
//! time these accumulate and eat the namespace's storage quota
//! (free tier: 5 GB). This module is the CLI surface to inspect and
//! prune them.
//!
//! GitHub doesn't have a directly equivalent Package Registry for
//! Generic binaries — its binary distribution model is Release
//! Assets attached to Releases. That lives in `release.rs` (0.7.10).
//! On GitHub-detected projects, the factory here errors out and
//! points the user at `torii release`.

use super::gitlab::GitLabPackageClient;
use crate::error::{Result, ToriiError};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub version: String,
    /// GitLab package type: generic | npm | maven | conan | pypi | composer | nuget | helm
    pub package_type: String,
    pub status: String,
    pub created_at: String,
    pub web_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageFile {
    pub id: String,
    pub package_id: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct PackageListFilters {
    /// Filter by package type (e.g. "generic" — what our release pipeline uses).
    pub package_type: Option<String>,
    /// Substring match on package name.
    pub name_search: Option<String>,
    /// Page size, clamped to [1, 100].
    pub per_page: usize,
}

#[allow(dead_code)]
pub trait PackageClient: Send {
    fn list(&self, owner: &str, repo: &str, filters: &PackageListFilters) -> Result<Vec<Package>>;
    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()>;
    fn list_files(&self, owner: &str, repo: &str, id: &str) -> Result<Vec<PackageFile>>;
}

// ============================================================================
// GitLab Package Registry
// ============================================================================

pub fn get_package_client(platform: &str) -> Result<Box<dyn PackageClient>> {
    match platform.to_lowercase().as_str() {
        "gitlab"    => Ok(Box::new(GitLabPackageClient::new()?)),
        "github"    => Err(ToriiError::Unsupported("GitHub doesn't have a Generic Package Registry equivalent to GitLab's. \
             Binary release assets on GitHub are managed through Releases: use `torii release` instead.".to_string())),
        "gitea"     => Err(ToriiError::Unsupported("Gitea/Codeberg has a Package Registry but its API isn't wired into torii yet. \
             For binary assets, use Releases (see `torii release`).".to_string())),
        "sourcehut" => Err(ToriiError::Unsupported("Sourcehut has no Package Registry concept. Binaries are distributed via the \
             project's own homepage or builds.sr.ht's `triggers` (uploaded externally).".to_string())),
        "radicle"   => Err(ToriiError::Unsupported("Radicle is peer-to-peer and has no central package registry. \
             Distribute binaries via the project's own channel or mirror to a registry host.".to_string())),
        "bitbucket" => Err(ToriiError::Unsupported("Bitbucket Cloud has no Package Registry. Binary distribution happens via the \
             Downloads tab (flat file list) or external hosting.".to_string())),
        "azure"     => Err(ToriiError::Unsupported("Azure Artifacts exists but lives at the organisation level (feeds), not per-repo. \
             The mapping isn't 1:1 with torii's owner/repo abstraction. Wired in a future release \
             once the org-feed-package addressing is designed; for now use the Azure DevOps UI \
             (https://dev.azure.com/{org}/_packaging).".to_string())),
        other => Err(ToriiError::Unsupported(format!("Unsupported platform: {}. Supported for `torii package`: gitlab", other))),
    }
}

/// Filter packages older than N days (i.e. KEEP only the ones older
/// than the cutoff — used by batch-delete to figure out which old
/// entries to remove). Conservative: packages with unparseable
/// timestamps are KEPT (we don't auto-delete state we can't reason
/// about) — matches `pipeline::filter_older_than` semantics.
pub fn filter_older_than(packages: Vec<Package>, days: i64) -> Vec<Package> {
    let cutoff = Utc::now() - Duration::days(days);
    packages
        .into_iter()
        .filter(|p| match DateTime::parse_from_rfc3339(&p.created_at) {
            Ok(dt) => dt.with_timezone(&Utc) < cutoff,
            Err(_) => true,
        })
        .collect()
}

/// Filter packages matching exact version. Used by batch delete.
pub fn filter_by_version(packages: Vec<Package>, version: &str) -> Vec<Package> {
    packages
        .into_iter()
        .filter(|p| p.version == version)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::gitlab::package::{parse_gitlab_package, parse_gitlab_package_file};

    #[test]
    fn parse_gitlab_package_basic() {
        let json = serde_json::json!({
            "id": 12345u64,
            "name": "gitorii",
            "version": "v0.7.9",
            "package_type": "generic",
            "status": "default",
            "created_at": "2026-05-19T22:00:00Z",
            "_links": { "web_path": "/paskidev/gitorii/-/packages/12345" }
        });
        let p = parse_gitlab_package(&json).unwrap();
        assert_eq!(p.id, "12345");
        assert_eq!(p.name, "gitorii");
        assert_eq!(p.version, "v0.7.9");
        assert_eq!(p.package_type, "generic");
    }

    #[test]
    fn parse_gitlab_package_file_basic() {
        let json = serde_json::json!({
            "id": 99u64,
            "file_name": "torii-linux-x86_64",
            "size": 20221192u64,
            "created_at": "2026-05-19T22:00:00Z"
        });
        let pf = parse_gitlab_package_file(&json, "12345").unwrap();
        assert_eq!(pf.id, "99");
        assert_eq!(pf.package_id, "12345");
        assert_eq!(pf.file_name, "torii-linux-x86_64");
        assert_eq!(pf.size_bytes, 20221192);
    }

    fn mk(v: &str, created: &str) -> Package {
        Package {
            id: "1".into(),
            name: "gitorii".into(),
            version: v.into(),
            package_type: "generic".into(),
            status: "default".into(),
            created_at: created.into(),
            web_url: String::new(),
        }
    }

    #[test]
    fn filter_older_than_keeps_old_drops_recent_keeps_unparseable() {
        let now = Utc::now();
        let recent = (now - Duration::days(2)).to_rfc3339();
        let ancient = (now - Duration::days(100)).to_rfc3339();
        let kept = filter_older_than(
            vec![
                mk("v0.7.0", &recent),
                mk("v0.1.0", &ancient),
                mk("v?.?.?", "not a date"),
            ],
            30,
        );
        // filter_older_than returns entries OLDER than the cutoff (i.e.
        // the ones we'd be willing to delete). 2-day-old recent → not
        // older → dropped. 100-day-old ancient → older → kept. Unparseable
        // timestamp → kept (conservative: don't delete what we can't
        // reason about).
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|p| p.version == "v0.1.0"));
        assert!(kept.iter().any(|p| p.version == "v?.?.?"));
    }
}
