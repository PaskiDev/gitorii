//! GitHub — release client.

use crate::error::{Result, ToriiError};
use crate::platforms::release::*;
use reqwest::blocking::Client;

pub struct GitHubReleaseClient {
    token: String,
    base_url: String,
}

impl GitHubReleaseClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("github", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "github".into(),
                message: "GitHub token not found. Run: torii auth set github YOUR_TOKEN"
                    .to_string(),
            })?;
        Ok(Self {
            token,
            base_url: "https://api.github.com".to_string(),
        })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
    fn auth(&self) -> String {
        format!("token {}", self.token)
    }
}

impl ReleaseClient for GitHubReleaseClient {
    fn list(&self, owner: &str, repo: &str, limit: usize) -> Result<Vec<Release>> {
        let url = format!(
            "{}/repos/{}/{}/releases?per_page={}",
            self.base_url,
            owner,
            repo,
            limit.clamp(1, 100)
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_github_release)
            .collect()
    }

    fn get(&self, owner: &str, repo: &str, tag: &str) -> Result<Release> {
        let url = format!(
            "{}/repos/{}/{}/releases/tags/{}",
            self.base_url, owner, repo, tag
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json");
        let json = crate::http::send_json(req, &format!("GitHub (tag: {})", tag))?;
        parse_github_release(&json)
    }

    fn edit(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        // GitHub edit uses the numeric release id, not the tag — fetch it first.
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: "GitHub release missing id field; cannot edit".to_string(),
        })?;
        let url = format!("{}/repos/{}/{}/releases/{}", self.base_url, owner, repo, id);
        let mut body = serde_json::Map::new();
        if let Some(n) = name {
            body.insert("name".into(), serde_json::Value::String(n.into()));
        }
        if let Some(d) = description {
            body.insert("body".into(), serde_json::Value::String(d.into()));
        }
        if body.is_empty() {
            return Err(ToriiError::Usage(
                "edit needs at least one of --name or --notes".to_string(),
            ));
        }
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json")
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "GitHub edit release")
    }

    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()> {
        let release = self.get(owner, repo, tag)?;
        let id = release.id.ok_or_else(|| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: "GitHub release missing id; cannot delete".to_string(),
        })?;
        let url = format!("{}/repos/{}/{}/releases/{}", self.base_url, owner, repo, id);
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github+json");
        crate::http::send_empty(req, "GitHub delete release")
    }
}

pub(crate) fn parse_github_release(v: &serde_json::Value) -> Result<Release> {
    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    let id = v["id"]
        .as_u64()
        .map(|n| n.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn client_for(server: &MockServer) -> GitHubReleaseClient {
        GitHubReleaseClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn parse_github_release_maps_all_fields() {
        let json = serde_json::json!({
            "tag_name": "v1.2.3",
            "name": "Release 1.2.3",
            "body": "Changelog",
            "created_at": "2026-03-01T00:00:00Z",
            "html_url": "https://github.com/o/r/releases/tag/v1.2.3",
            "id": 987u64,
        });
        let r = parse_github_release(&json).unwrap();
        assert_eq!(r.tag, "v1.2.3");
        assert_eq!(r.name, "Release 1.2.3");
        assert_eq!(r.description, "Changelog");
        assert_eq!(r.created_at, "2026-03-01T00:00:00Z");
        assert_eq!(r.web_url, "https://github.com/o/r/releases/tag/v1.2.3");
        assert_eq!(r.id.as_deref(), Some("987"));
    }

    #[test]
    fn parse_github_release_falls_back_to_tag_when_name_missing() {
        let json = serde_json::json!({ "tag_name": "v0.1.0" });
        let r = parse_github_release(&json).unwrap();
        assert_eq!(r.tag, "v0.1.0");
        assert_eq!(r.name, "v0.1.0");
        assert_eq!(r.description, "");
        assert_eq!(r.id, None);
    }

    #[test]
    fn list_parses_releases_from_api() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/octo/demo/releases")
                .query_param("per_page", "2")
                .header("Authorization", "token test-token");
            then.status(200).json_body(serde_json::json!([
                { "tag_name": "v2.0.0", "name": "Two", "id": 2u64 },
                { "tag_name": "v1.0.0", "name": "One", "id": 1u64 },
            ]));
        });
        let releases = client_for(&server).list("octo", "demo", 2).unwrap();
        m.assert();
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag, "v2.0.0");
        assert_eq!(releases[0].name, "Two");
        assert_eq!(releases[1].id.as_deref(), Some("1"));
    }

    #[test]
    fn delete_fetches_id_by_tag_then_deletes_with_auth() {
        let server = MockServer::start();
        let get_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/octo/demo/releases/tags/v1.0.0")
                .header("Authorization", "token test-token");
            then.status(200)
                .json_body(serde_json::json!({ "tag_name": "v1.0.0", "id": 55u64 }));
        });
        let del_mock = server.mock(|when, then| {
            when.method(DELETE)
                .path("/repos/octo/demo/releases/55")
                .header("Authorization", "token test-token");
            then.status(204);
        });
        client_for(&server)
            .delete("octo", "demo", "v1.0.0")
            .unwrap();
        get_mock.assert();
        del_mock.assert();
    }

    #[test]
    fn get_maps_404_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/repos/octo/demo/releases/tags/v9.9.9");
            then.status(404)
                .json_body(serde_json::json!({ "message": "Not Found" }));
        });
        let err = client_for(&server)
            .get("octo", "demo", "v9.9.9")
            .unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 404, .. }),
            "expected PlatformApi 404, got: {err:?}"
        );
    }
}
