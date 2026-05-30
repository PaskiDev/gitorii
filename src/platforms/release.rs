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
        crate::http::make_client()
    }
    fn auth(&self) -> String { format!("token {}", self.token) }
}

impl ReleaseClient for GitHubReleaseClient {
    fn list(&self, owner: &str, repo: &str, limit: usize) -> Result<Vec<Release>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases?per_page={}",
            owner, repo, limit.clamp(1, 100)
        );
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_github_release).collect()
    }

    fn get(&self, owner: &str, repo: &str, tag: &str) -> Result<Release> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{}",
            owner, repo, tag
        );
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json");
        let json = crate::http::send_json(req, &format!("GitHub (tag: {})", tag))?;
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
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json")
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "GitHub edit release")
    }

    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()> {
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::InvalidConfig(
            "GitHub release missing id; cannot delete".to_string()
        ))?;
        let url = format!("https://api.github.com/repos/{}/{}/releases/{}", owner, repo, id);
        let req = self.client().delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json");
        crate::http::send_empty(req, "GitHub delete release")
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
        crate::http::make_client()
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
        let req = self.client().get(&url).header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_gitlab_release).collect()
    }

    fn get(&self, owner: &str, repo: &str, tag: &str) -> Result<Release> {
        let url = format!(
            "{}/projects/{}/releases/{}",
            self.base_url, Self::project_path(owner, repo), tag
        );
        let req = self.client().get(&url).header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (tag: {})", tag))?;
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
        let req = self.client().put(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "GitLab edit release")
    }

    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/releases/{}",
            self.base_url, Self::project_path(owner, repo), tag
        );
        let req = self.client().delete(&url).header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab delete release")
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
// Gitea / Codeberg / Forgejo Releases
// ============================================================================
//
// Gitea exposes a GitHub-shaped REST API at `/api/v1/...`. Forgejo (the
// Codeberg fork) is API-compatible. Auth header is `token <token>`, same
// as GitHub. Releases carry an integer `id` separate from the tag,
// matching GitHub's model — `delete` requires the id, not the tag.

pub struct GiteaReleaseClient {
    token: String,
    base_url: String,
}

impl GiteaReleaseClient {
    pub fn new() -> Result<Self> {
        Self::new_with_host(crate::pr::gitea_base_url())
    }

    /// Construct against an arbitrary Gitea/Forgejo host. Today only
    /// called with codeberg.org; in 0.8.0 the platforms.toml resolver
    /// will pass user-declared self-hosted URLs through here.
    pub fn new_with_host(base_url: &str) -> Result<Self> {
        let token = crate::pr::resolve_gitea_token()?;
        Ok(Self {
            token,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
    fn auth(&self) -> String { format!("token {}", self.token) }
}

impl ReleaseClient for GiteaReleaseClient {
    fn list(&self, owner: &str, repo: &str, limit: usize) -> Result<Vec<Release>> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/releases?limit={}",
            self.base_url, owner, repo, limit.clamp(1, 50)
        );
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, &format!("Gitea (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_gitea_release).collect()
    }

    fn get(&self, owner: &str, repo: &str, tag: &str) -> Result<Release> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/releases/tags/{}",
            self.base_url, owner, repo, tag
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Gitea (tag: {})", tag))?;
        parse_gitea_release(&json)
    }

    fn edit(&self, owner: &str, repo: &str, tag: &str, name: Option<&str>, description: Option<&str>) -> Result<()> {
        // Gitea's edit takes the integer release id, not the tag. Resolve
        // it via `get` first.
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::InvalidConfig(
            "Gitea release missing id (cannot edit)".to_string()
        ))?;

        let mut body = serde_json::Map::new();
        if let Some(n) = name        { body.insert("name".into(), serde_json::Value::String(n.to_string())); }
        if let Some(d) = description { body.insert("body".into(), serde_json::Value::String(d.to_string())); }
        if body.is_empty() { return Ok(()); }

        let url = format!("{}/api/v1/repos/{}/{}/releases/{}", self.base_url, owner, repo, id);
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "Gitea edit release")
    }

    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()> {
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::InvalidConfig(
            "Gitea release missing id (cannot delete)".to_string()
        ))?;
        let url = format!("{}/api/v1/repos/{}/{}/releases/{}", self.base_url, owner, repo, id);
        let req = self.client().delete(&url).header("Authorization", self.auth());
        crate::http::send_empty(req, "Gitea delete release")
    }
}

fn parse_gitea_release(v: &serde_json::Value) -> Result<Release> {
    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    let id = v["id"].as_u64().map(|n| n.to_string());
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
// Sourcehut (no native release object)
// ============================================================================
//
// Sourcehut doesn't expose "releases" as a first-class object the
// way GitHub / GitLab / Gitea do — a release is just a git tag, and
// binary distribution happens externally (project homepage, paste.sr.ht,
// etc.). We return a clear error so the CLI surface remains honest.

pub struct SourcehutReleaseClient;

impl SourcehutReleaseClient {
    pub fn new() -> Result<Self> { Ok(Self) }
}

fn srht_release_unsupported() -> ToriiError {
    ToriiError::InvalidConfig(
        "Sourcehut has no Release-page object. A release on sourcehut \
         is just an annotated git tag (`torii tag create vX --release`); \
         binaries live outside the host. If you want listed releases \
         with notes + assets, mirror the project to GitLab/GitHub/Codeberg \
         and use the platform's native release API there.".to_string()
    )
}

impl ReleaseClient for SourcehutReleaseClient {
    fn list(&self, _o: &str, _r: &str, _l: usize) -> Result<Vec<Release>> { Err(srht_release_unsupported()) }
    fn get(&self, _o: &str, _r: &str, _t: &str) -> Result<Release> { Err(srht_release_unsupported()) }
    fn edit(&self, _o: &str, _r: &str, _t: &str, _n: Option<&str>, _d: Option<&str>) -> Result<()> { Err(srht_release_unsupported()) }
    fn delete(&self, _o: &str, _r: &str, _t: &str) -> Result<()> { Err(srht_release_unsupported()) }
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

pub struct RadicleReleaseClient;

impl RadicleReleaseClient {
    pub fn new() -> Result<Self> { Ok(Self) }
}

fn radicle_release_unsupported() -> ToriiError {
    ToriiError::InvalidConfig(
        "Radicle has no Release-page object. A release on radicle is \
         just an annotated git tag (`torii tag create vX --release`); \
         binaries live outside the network. Mirror to GitLab/GitHub/Codeberg \
         if you need a hosted release page with notes + assets.".to_string()
    )
}

impl ReleaseClient for RadicleReleaseClient {
    fn list(&self, _o: &str, _r: &str, _l: usize) -> Result<Vec<Release>> { Err(radicle_release_unsupported()) }
    fn get(&self, _o: &str, _r: &str, _t: &str) -> Result<Release> { Err(radicle_release_unsupported()) }
    fn edit(&self, _o: &str, _r: &str, _t: &str, _n: Option<&str>, _d: Option<&str>) -> Result<()> { Err(radicle_release_unsupported()) }
    fn delete(&self, _o: &str, _r: &str, _t: &str) -> Result<()> { Err(radicle_release_unsupported()) }
}

// ============================================================================
// Bitbucket Cloud (no Release object)
// ============================================================================
//
// Bitbucket Cloud doesn't have GitHub-style Release entities. It has
// "Downloads" (binary files attached to a repo, separate from tags)
// and tags. Most projects use Downloads via the web UI; we expose a
// clear error so the surface stays honest.

pub struct BitbucketReleaseClient;

impl BitbucketReleaseClient {
    pub fn new() -> Result<Self> { Ok(Self) }
}

fn bitbucket_release_unsupported() -> ToriiError {
    ToriiError::InvalidConfig(
        "Bitbucket Cloud has no Release-page object. It exposes \
         'Downloads' (a flat file list, separate from tags, no notes) \
         which isn't equivalent. Use annotated tags + the Downloads tab \
         manually, or mirror to GitHub/GitLab/Codeberg for hosted releases.".to_string()
    )
}

impl ReleaseClient for BitbucketReleaseClient {
    fn list(&self, _o: &str, _r: &str, _l: usize) -> Result<Vec<Release>> { Err(bitbucket_release_unsupported()) }
    fn get(&self, _o: &str, _r: &str, _t: &str) -> Result<Release> { Err(bitbucket_release_unsupported()) }
    fn edit(&self, _o: &str, _r: &str, _t: &str, _n: Option<&str>, _d: Option<&str>) -> Result<()> { Err(bitbucket_release_unsupported()) }
    fn delete(&self, _o: &str, _r: &str, _t: &str) -> Result<()> { Err(bitbucket_release_unsupported()) }
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

pub struct AzureReleaseClient {
    token: String,
}

impl AzureReleaseClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("azure", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "Azure DevOps PAT not found. Create at https://dev.azure.com/{org}/_usersSettings/tokens \
                 with `Release (read/write)` scope, then: torii auth set azure YOUR_PAT".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth(&self) -> String {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(format!(":{}", self.token));
        format!("Basic {}", b64)
    }
}

impl ReleaseClient for AzureReleaseClient {
    fn list(&self, owner: &str, _repo: &str, limit: usize) -> Result<Vec<Release>> {
        // Azure Releases are project-scoped; the `_repo` arg is ignored.
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://vsrm.dev.azure.com/{}/{}/_apis/release/releases?api-version=7.0&$top={}",
            org, project, limit.clamp(1, 100)
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Azure (url: {})", url))?;
        let arr = json["value"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Azure returned no `value` array. Body: {}", json
            )))?;
        let org_clone = org.clone();
        let project_clone = project.clone();
        arr.iter().map(|v| parse_azure_release(v, &org_clone, &project_clone)).collect()
    }

    fn get(&self, owner: &str, _repo: &str, tag_or_id: &str) -> Result<Release> {
        // Azure releases are identified by numeric id, not tag. Callers
        // can pass either — if it's not numeric we try a name lookup.
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let id = if tag_or_id.parse::<u64>().is_ok() {
            tag_or_id.to_string()
        } else {
            // Best-effort name lookup via $filter.
            let list_url = format!(
                "https://vsrm.dev.azure.com/{}/{}/_apis/release/releases?api-version=7.0&$top=200",
                org, project
            );
            let lookup_req = self.client().get(&list_url).header("Authorization", self.auth());
            let lookup_json = crate::http::send_json(lookup_req, "Azure lookup release by name")?;
            lookup_json["value"].as_array()
                .and_then(|arr| arr.iter().find(|v| v["name"].as_str() == Some(tag_or_id)))
                .and_then(|v| v["id"].as_u64().map(|n| n.to_string()))
                .ok_or_else(|| ToriiError::InvalidConfig(format!(
                    "Azure: no release named '{}' in project {}", tag_or_id, project
                )))?
        };
        let url = format!(
            "https://vsrm.dev.azure.com/{}/{}/_apis/release/releases/{}?api-version=7.0",
            org, project, id
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Azure release #{}", id))?;
        parse_azure_release(&json, &org, &project)
    }

    fn edit(&self, _o: &str, _r: &str, _tag: &str, _n: Option<&str>, _d: Option<&str>) -> Result<()> {
        Err(ToriiError::InvalidConfig(
            "Azure Releases doesn't expose a mutation API for already-created releases — \
             metadata is derived from the release definition (template). Edit the definition \
             in the web UI; the next release will pick up the new metadata.".to_string()
        ))
    }

    fn delete(&self, owner: &str, _repo: &str, tag_or_id: &str) -> Result<()> {
        let release = self.get(owner, "", tag_or_id)?;
        let id = release.id.ok_or_else(|| ToriiError::InvalidConfig(
            "Azure release missing id; cannot delete".to_string()
        ))?;
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://vsrm.dev.azure.com/{}/{}/_apis/release/releases/{}?api-version=7.0",
            org, project, id
        );
        let req = self.client().delete(&url).header("Authorization", self.auth());
        crate::http::send_empty(req, "Azure delete release")
    }
}

fn parse_azure_release(v: &serde_json::Value, org: &str, project: &str) -> Result<Release> {
    let id = v["id"].as_u64().map(|n| n.to_string());
    let name = v["name"].as_str().unwrap_or("").to_string();
    Ok(Release {
        // Azure Releases don't tie to a git tag — we surface the
        // release name as `tag` so the CLI display stays consistent.
        tag: name.clone(),
        name,
        description: v["description"].as_str().unwrap_or("").to_string(),
        created_at: v["createdOn"].as_str().unwrap_or("").to_string(),
        web_url:    id.as_ref().map(|i| format!(
            "https://dev.azure.com/{}/{}/_releaseProgress?releaseId={}", org, project, i
        )).unwrap_or_default(),
        id,
    })
}

// ============================================================================
// Factory
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
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported: github, gitlab, gitea, sourcehut, radicle, bitbucket, azure", other)
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
