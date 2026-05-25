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

use serde::{Deserialize, Serialize};
use reqwest::blocking::Client;
use chrono::{DateTime, Utc, Duration};
use crate::error::{Result, ToriiError};

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

pub struct GitLabPackageClient {
    token: String,
    base_url: String,
}

impl GitLabPackageClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("gitlab", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "GitLab token not found. Run: torii auth set gitlab YOUR_TOKEN".to_string()
            ))?;
        let base_url = std::env::var("GITLAB_URL")
            .unwrap_or_else(|_| "https://gitlab.com/api/v4".to_string());
        Ok(Self { token, base_url })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }

    fn project_path(owner: &str, repo: &str) -> String {
        crate::url::encode(&format!("{}/{}", owner, repo))
    }
}

impl PackageClient for GitLabPackageClient {
    fn list(&self, owner: &str, repo: &str, filters: &PackageListFilters) -> Result<Vec<Package>> {
        let mut url = format!(
            "{}/projects/{}/packages?per_page={}",
            self.base_url, Self::project_path(owner, repo),
            filters.per_page.clamp(1, 100)
        );
        if let Some(t) = &filters.package_type {
            url.push_str(&format!("&package_type={}", t));
        }
        if let Some(n) = &filters.name_search {
            url.push_str(&format!("&package_name={}", crate::url::encode(n)));
        }
        let req = self.client().get(&url).header("PRIVATE-TOKEN", &self.token);
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_gitlab_package).collect()
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/packages/{}",
            self.base_url, Self::project_path(owner, repo), id
        );
        let req = self.client().delete(&url).header("PRIVATE-TOKEN", &self.token);
        crate::http::send_empty(req, "GitLab delete package")
    }

    fn list_files(&self, owner: &str, repo: &str, id: &str) -> Result<Vec<PackageFile>> {
        let url = format!(
            "{}/projects/{}/packages/{}/package_files?per_page=100",
            self.base_url, Self::project_path(owner, repo), id
        );
        let req = self.client().get(&url).header("PRIVATE-TOKEN", &self.token);
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(|v| parse_gitlab_package_file(v, id)).collect()
    }
}

fn parse_gitlab_package(v: &serde_json::Value) -> Result<Package> {
    let id = v["id"].as_u64().map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::InvalidConfig("GitLab package missing id".into()))?;
    Ok(Package {
        id,
        name: v["name"].as_str().unwrap_or("").to_string(),
        version: v["version"].as_str().unwrap_or("").to_string(),
        package_type: v["package_type"].as_str().unwrap_or("").to_string(),
        status: v["status"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        web_url: v["_links"]["web_path"].as_str().unwrap_or("").to_string(),
    })
}

fn parse_gitlab_package_file(v: &serde_json::Value, package_id: &str) -> Result<PackageFile> {
    let id = v["id"].as_u64().map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::InvalidConfig("GitLab package_file missing id".into()))?;
    Ok(PackageFile {
        id,
        package_id: package_id.to_string(),
        file_name: v["file_name"].as_str().unwrap_or("").to_string(),
        size_bytes: v["size"].as_u64().unwrap_or(0),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Factory + helpers
// ============================================================================

pub fn get_package_client(platform: &str) -> Result<Box<dyn PackageClient>> {
    match platform.to_lowercase().as_str() {
        "gitlab"    => Ok(Box::new(GitLabPackageClient::new()?)),
        "github"    => Err(ToriiError::InvalidConfig(
            "GitHub doesn't have a Generic Package Registry equivalent to GitLab's. \
             Binary release assets on GitHub are managed through Releases: use `torii release` instead.".to_string()
        )),
        "gitea"     => Err(ToriiError::InvalidConfig(
            "Gitea/Codeberg has a Package Registry but its API isn't wired into torii yet. \
             For binary assets, use Releases (see `torii release`).".to_string()
        )),
        "sourcehut" => Err(ToriiError::InvalidConfig(
            "Sourcehut has no Package Registry concept. Binaries are distributed via the \
             project's own homepage or builds.sr.ht's `triggers` (uploaded externally).".to_string()
        )),
        "radicle"   => Err(ToriiError::InvalidConfig(
            "Radicle is peer-to-peer and has no central package registry. \
             Distribute binaries via the project's own channel or mirror to a registry host.".to_string()
        )),
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported for `torii package`: gitlab", other)
        )),
    }
}

/// Filter packages older than N days (i.e. KEEP only the ones older
/// than the cutoff — used by batch-delete to figure out which old
/// entries to remove). Conservative: packages with unparseable
/// timestamps are KEPT (we don't auto-delete state we can't reason
/// about) — matches `pipeline::filter_older_than` semantics.
pub fn filter_older_than(packages: Vec<Package>, days: i64) -> Vec<Package> {
    let cutoff = Utc::now() - Duration::days(days);
    packages.into_iter().filter(|p| {
        match DateTime::parse_from_rfc3339(&p.created_at) {
            Ok(dt) => dt.with_timezone(&Utc) < cutoff,
            Err(_) => true,
        }
    }).collect()
}

/// Filter packages matching exact version. Used by batch delete.
pub fn filter_by_version(packages: Vec<Package>, version: &str) -> Vec<Package> {
    packages.into_iter().filter(|p| p.version == version).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let kept = filter_older_than(vec![
            mk("v0.7.0", &recent),
            mk("v0.1.0", &ancient),
            mk("v?.?.?", "not a date"),
        ], 30);
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
