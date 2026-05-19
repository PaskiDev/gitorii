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

use serde::{Deserialize, Serialize};
use reqwest::blocking::Client;
use crate::error::{Result, ToriiError};

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
    fn edit(&self, owner: &str, repo: &str, tag: &str, name: Option<&str>, description: Option<&str>) -> Result<()>;
    /// Delete the release. On GitLab this only removes the release entity
    /// (the underlying tag stays); on GitHub the release deletion API
    /// likewise leaves the tag intact (use `torii tag delete` separately
    /// if you want both gone).
    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()>;
}

// ============================================================================
// GitHub Releases
// ============================================================================

pub struct GitHubReleaseClient { token: String }

impl GitHubReleaseClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("github", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "GitHub token not found. Run: torii auth set github YOUR_TOKEN".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        Client::builder().user_agent("gitorii-cli").build().unwrap()
    }
    fn auth(&self) -> String { format!("token {}", self.token) }
}

impl ReleaseClient for GitHubReleaseClient {
    fn list(&self, owner: &str, repo: &str, limit: usize) -> Result<Vec<Release>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases?per_page={}",
            owner, repo, limit.clamp(1, 100)
        );
        let resp = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str().unwrap_or("(no message)");
            return Err(ToriiError::InvalidConfig(format!(
                "GitHub API {}: {} (url: {})", status, msg, url
            )));
        }
        let arr = json.as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitHub returned non-array body for {}. Body: {}", url, json
            )))?;
        arr.iter().map(parse_github_release).collect()
    }

    fn get(&self, owner: &str, repo: &str, tag: &str) -> Result<Release> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{}",
            owner, repo, tag
        );
        let resp = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str().unwrap_or("(no message)");
            return Err(ToriiError::InvalidConfig(format!(
                "GitHub API {}: {} (tag: {})", status, msg, tag
            )));
        }
        parse_github_release(&json)
    }

    fn edit(&self, owner: &str, repo: &str, tag: &str, name: Option<&str>, description: Option<&str>) -> Result<()> {
        // GitHub edit uses the numeric release id, not the tag — fetch it first.
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::InvalidConfig(
            "GitHub release missing id field; cannot edit".to_string()
        ))?;
        let url = format!("https://api.github.com/repos/{}/{}/releases/{}", owner, repo, id);
        let mut body = serde_json::Map::new();
        if let Some(n) = name { body.insert("name".into(), serde_json::Value::String(n.into())); }
        if let Some(d) = description { body.insert("body".into(), serde_json::Value::String(d.into())); }
        if body.is_empty() {
            return Err(ToriiError::InvalidConfig(
                "edit needs at least one of --name or --notes".to_string()
            ));
        }
        let resp = self.client().patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json")
            .json(&serde_json::Value::Object(body))
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let txt = resp.text().unwrap_or_default();
            return Err(ToriiError::InvalidConfig(format!("GitHub API {} edit failed: {}", s, txt)));
        }
        Ok(())
    }

    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()> {
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::InvalidConfig(
            "GitHub release missing id; cannot delete".to_string()
        ))?;
        let url = format!("https://api.github.com/repos/{}/{}/releases/{}", owner, repo, id);
        let resp = self.client().delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let txt = resp.text().unwrap_or_default();
            return Err(ToriiError::InvalidConfig(format!("GitHub API {} delete failed: {}", s, txt)));
        }
        Ok(())
    }
}

fn parse_github_release(v: &serde_json::Value) -> Result<Release> {
    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    let id = v["id"].as_u64().map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from));
    Ok(Release {
        tag: tag.clone(),
        name: v["name"].as_str().unwrap_or(&tag).to_string(),
        description: v["body"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        web_url: v["html_url"].as_str().unwrap_or("").to_string(),
        id,
    })
}

// ============================================================================
// GitLab Releases
// ============================================================================

pub struct GitLabReleaseClient {
    token: String,
    base_url: String,
}

impl GitLabReleaseClient {
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
        Client::builder().user_agent("gitorii-cli").build().unwrap()
    }

    fn project_path(owner: &str, repo: &str) -> String {
        crate::url::encode(&format!("{}/{}", owner, repo))
    }
}

impl ReleaseClient for GitLabReleaseClient {
    fn list(&self, owner: &str, repo: &str, limit: usize) -> Result<Vec<Release>> {
        let url = format!(
            "{}/projects/{}/releases?per_page={}",
            self.base_url, Self::project_path(owner, repo),
            limit.clamp(1, 100)
        );
        let resp = self.client().get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str()
                .or_else(|| json["error"].as_str())
                .unwrap_or("(no message)");
            return Err(ToriiError::InvalidConfig(format!(
                "GitLab API {}: {} (url: {})", status, msg, url
            )));
        }
        let arr = json.as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitLab returned non-array for {}. Body: {}", url, json
            )))?;
        arr.iter().map(parse_gitlab_release).collect()
    }

    fn get(&self, owner: &str, repo: &str, tag: &str) -> Result<Release> {
        let url = format!(
            "{}/projects/{}/releases/{}",
            self.base_url, Self::project_path(owner, repo), tag
        );
        let resp = self.client().get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str().unwrap_or("(no message)");
            return Err(ToriiError::InvalidConfig(format!(
                "GitLab API {}: {} (tag: {})", status, msg, tag
            )));
        }
        parse_gitlab_release(&json)
    }

    fn edit(&self, owner: &str, repo: &str, tag: &str, name: Option<&str>, description: Option<&str>) -> Result<()> {
        let url = format!(
            "{}/projects/{}/releases/{}",
            self.base_url, Self::project_path(owner, repo), tag
        );
        let mut body = serde_json::Map::new();
        if let Some(n) = name { body.insert("name".into(), serde_json::Value::String(n.into())); }
        if let Some(d) = description { body.insert("description".into(), serde_json::Value::String(d.into())); }
        if body.is_empty() {
            return Err(ToriiError::InvalidConfig(
                "edit needs at least one of --name or --notes".to_string()
            ));
        }
        let resp = self.client().put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::Value::Object(body))
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let txt = resp.text().unwrap_or_default();
            return Err(ToriiError::InvalidConfig(format!("GitLab API {} edit failed: {}", s, txt)));
        }
        Ok(())
    }

    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/releases/{}",
            self.base_url, Self::project_path(owner, repo), tag
        );
        let resp = self.client().delete(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let txt = resp.text().unwrap_or_default();
            return Err(ToriiError::InvalidConfig(format!("GitLab API {} delete failed: {}", s, txt)));
        }
        Ok(())
    }
}

fn parse_gitlab_release(v: &serde_json::Value) -> Result<Release> {
    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    Ok(Release {
        tag: tag.clone(),
        name: v["name"].as_str().unwrap_or(&tag).to_string(),
        description: v["description"].as_str().unwrap_or("").to_string(),
        created_at: v["created_at"].as_str().unwrap_or("").to_string(),
        web_url: v["_links"]["self"].as_str().unwrap_or("").to_string(),
        id: None, // GitLab uses the tag as the identifier
    })
}

// ============================================================================
// Factory
// ============================================================================

pub fn get_release_client(platform: &str) -> Result<Box<dyn ReleaseClient>> {
    match platform.to_lowercase().as_str() {
        "github" => Ok(Box::new(GitHubReleaseClient::new()?)),
        "gitlab" => Ok(Box::new(GitLabReleaseClient::new()?)),
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported: github, gitlab", other)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(r.web_url, "https://gitlab.com/paskidev/gitorii/-/releases/v0.7.9");
    }
}
