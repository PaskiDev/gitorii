//! Gitea / Codeberg / Forgejo — release client.

use crate::error::{Result, ToriiError};
use crate::platforms::release::*;
use reqwest::blocking::Client;

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
    fn auth(&self) -> String {
        format!("token {}", self.token)
    }
}

impl ReleaseClient for GiteaReleaseClient {
    fn list(&self, owner: &str, repo: &str, limit: usize) -> Result<Vec<Release>> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/releases?limit={}",
            self.base_url,
            owner,
            repo,
            limit.clamp(1, 50)
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, &format!("Gitea (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_gitea_release)
            .collect()
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

    fn edit(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        // Gitea's edit takes the integer release id, not the tag. Resolve
        // it via `get` first.
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::MalformedResponse {
            provider: "gitea".into(),
            message: "Gitea release missing id (cannot edit)".to_string(),
        })?;

        let mut body = serde_json::Map::new();
        if let Some(n) = name {
            body.insert("name".into(), serde_json::Value::String(n.to_string()));
        }
        if let Some(d) = description {
            body.insert("body".into(), serde_json::Value::String(d.to_string()));
        }
        if body.is_empty() {
            return Ok(());
        }

        let url = format!(
            "{}/api/v1/repos/{}/{}/releases/{}",
            self.base_url, owner, repo, id
        );
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "Gitea edit release")
    }

    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()> {
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::MalformedResponse {
            provider: "gitea".into(),
            message: "Gitea release missing id (cannot delete)".to_string(),
        })?;
        let url = format!(
            "{}/api/v1/repos/{}/{}/releases/{}",
            self.base_url, owner, repo, id
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth());
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn client(server: &MockServer) -> GiteaReleaseClient {
        GiteaReleaseClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    fn release_json(id: u64, tag: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "tag_name": tag,
            "name": "Big Release",
            "body": "changelog here",
            "created_at": "2026-03-04T05:06:07Z",
            "html_url": "https://codeberg.org/o/r/releases/tag/v1.0.0",
        })
    }

    #[test]
    fn parse_gitea_release_extracts_all_fields() {
        let r = parse_gitea_release(&release_json(12, "v1.0.0")).unwrap();
        assert_eq!(r.tag, "v1.0.0");
        assert_eq!(r.name, "Big Release");
        assert_eq!(r.description, "changelog here");
        assert_eq!(r.created_at, "2026-03-04T05:06:07Z");
        assert_eq!(r.web_url, "https://codeberg.org/o/r/releases/tag/v1.0.0");
        assert_eq!(r.id.as_deref(), Some("12"));
    }

    #[test]
    fn parse_gitea_release_falls_back_to_tag_when_name_missing() {
        let r = parse_gitea_release(&serde_json::json!({ "tag_name": "v0.1.0" })).unwrap();
        assert_eq!(r.tag, "v0.1.0");
        assert_eq!(r.name, "v0.1.0");
        assert_eq!(r.description, "");
        assert_eq!(r.id, None);
    }

    #[test]
    fn list_parses_releases_from_mocked_endpoint() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/repos/owner/repo/releases")
                .query_param("limit", "5")
                .header("Authorization", "token test-token");
            then.status(200).json_body(serde_json::json!([
                release_json(1, "v1.0.0"),
                release_json(2, "v1.1.0"),
            ]));
        });
        let releases = client(&server).list("owner", "repo", 5).unwrap();
        mock.assert();
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag, "v1.0.0");
        assert_eq!(releases[1].id.as_deref(), Some("2"));
    }

    #[test]
    fn delete_resolves_tag_to_id_then_deletes_with_token_auth() {
        let server = MockServer::start();
        let get_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/repos/owner/repo/releases/tags/v1.0.0")
                .header("Authorization", "token test-token");
            then.status(200).json_body(release_json(12, "v1.0.0"));
        });
        let delete_mock = server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v1/repos/owner/repo/releases/12")
                .header("Authorization", "token test-token");
            then.status(204);
        });
        client(&server).delete("owner", "repo", "v1.0.0").unwrap();
        get_mock.assert();
        delete_mock.assert();
    }

    #[test]
    fn get_maps_non_2xx_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/repos/owner/repo/releases/tags/v9.9.9");
            then.status(404)
                .json_body(serde_json::json!({ "message": "Not Found" }));
        });
        let err = client(&server).get("owner", "repo", "v9.9.9").unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 404, .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
