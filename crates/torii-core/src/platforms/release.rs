//! Release page management — `torii release …`.
//!
//! Both GitLab and GitHub expose "Release" entities: a named tag-anchored
//! object with a description (markdown) and a list of assets (links and/or
//! uploaded files). gitorii's CI creates these via the API; this module is
//! the CLI surface to edit notes, fix typos, and delete bad releases without
//! having to rebuild the entire pipeline.
//!
//! Asymmetries between platforms (documented inline where they exist):
//!   - GitLab: release description is one Markdown blob plus a list of
//!     asset *links* (URLs pointing into the Package Registry). To
//!     "delete an asset" you delete the *package* (`torii package
//!     delete`), the release re-renders the link list automatically.
//!   - GitHub: release description is one Markdown blob plus uploaded
//!     binary *assets* attached directly to the release. Each asset
//!     has its own id and is deletable.

use super::azure::AzureReleaseClient;
use super::bitbucket::BitbucketReleaseClient;
use super::gitea::GiteaReleaseClient;
use super::github::GitHubReleaseClient;
use super::gitlab::GitLabReleaseClient;
use super::radicle::RadicleReleaseClient;
use super::sourcehut::SourcehutReleaseClient;
use crate::error::{Result, ToriiError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    /// The tag this release is anchored to (e.g. "v0.7.9"). Used as the
    /// identifier in API paths since releases don't have separate numeric
    /// ids in GitLab; GitHub has both an id and a tag, we use the tag for
    /// CLI ergonomics.
    pub tag: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub web_url: String,
    /// Optional release id (GitHub uses this for API paths; GitLab uses tag).
    pub id: Option<String>,
}

#[allow(dead_code)]
pub trait ReleaseClient: Send {
    fn list(&self, owner: &str, repo: &str, limit: usize) -> Result<Vec<Release>>;
    fn get(&self, owner: &str, repo: &str, tag: &str) -> Result<Release>;
    /// Update release metadata. `name` / `description` are both optional —
    /// pass None to leave them unchanged.
    fn edit(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<()>;
    /// Delete the release. On GitLab this only removes the release entity
    /// (the underlying tag stays); on GitHub the release deletion API
    /// likewise leaves the tag intact (use `torii tag delete` separately
    /// if you want both gone).
    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()>;
}

// ============================================================================
// GitHub Releases
// ============================================================================

pub fn get_release_client(platform: &str) -> Result<Box<dyn ReleaseClient>> {
    match platform.to_lowercase().as_str() {
        "github"    => Ok(Box::new(GitHubReleaseClient::new()?)),
        "gitlab"    => Ok(Box::new(GitLabReleaseClient::new()?)),
        "gitea"     => Ok(Box::new(GiteaReleaseClient::new()?)),
        "sourcehut" => Ok(Box::new(SourcehutReleaseClient::new()?)),
        "radicle"   => Ok(Box::new(RadicleReleaseClient::new()?)),
        "bitbucket" => Ok(Box::new(BitbucketReleaseClient::new()?)),
        "azure"     => Ok(Box::new(AzureReleaseClient::new()?)),
        other => Err(ToriiError::Unsupported(format!("Unsupported platform: {}. Supported: github, gitlab, gitea, sourcehut, radicle, bitbucket, azure", other))),
    }
}

#[cfg(test)]
mod tests {
    use crate::platforms::github::release::parse_github_release;
    use crate::platforms::gitlab::release::parse_gitlab_release;

    #[test]
    fn parse_github_release_basic() {
        let json = serde_json::json!({
            "id": 12345u64,
            "tag_name": "v0.7.9",
            "name": "Gitorii v0.7.9",
            "body": "Release notes here",
            "created_at": "2026-05-19T22:00:00Z",
            "html_url": "https://github.com/paskidev/gitorii/releases/tag/v0.7.9"
        });
        let r = parse_github_release(&json).unwrap();
        assert_eq!(r.tag, "v0.7.9");
        assert_eq!(r.name, "Gitorii v0.7.9");
        assert_eq!(r.id.as_deref(), Some("12345"));
    }

    #[test]
    fn parse_gitlab_release_basic() {
        let json = serde_json::json!({
            "tag_name": "v0.7.9",
            "name": "Gitorii v0.7.9",
            "description": "Release notes",
            "created_at": "2026-05-19T22:00:00Z",
            "_links": { "self": "https://gitlab.com/paskidev/gitorii/-/releases/v0.7.9" }
        });
        let r = parse_gitlab_release(&json).unwrap();
        assert_eq!(r.tag, "v0.7.9");
        assert_eq!(r.id, None);
        assert_eq!(
            r.web_url,
            "https://gitlab.com/paskidev/gitorii/-/releases/v0.7.9"
        );
    }
}
