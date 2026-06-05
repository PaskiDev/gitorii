//! GitLab — release client.

use crate::error::{Result, ToriiError};
use crate::platforms::release::*;
use reqwest::blocking::Client;

pub struct GitLabReleaseClient {
    token: String,
    base_url: String,
}

impl GitLabReleaseClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("gitlab", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "gitlab".into(),
                message: "GitLab token not found. Run: torii auth set gitlab YOUR_TOKEN"
                    .to_string(),
            })?;
        let base_url =
            std::env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com/api/v4".to_string());
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
            self.base_url,
            Self::project_path(owner, repo),
            limit.clamp(1, 100)
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_gitlab_release)
            .collect()
    }

    fn get(&self, owner: &str, repo: &str, tag: &str) -> Result<Release> {
        let url = format!(
            "{}/projects/{}/releases/{}",
            self.base_url,
            Self::project_path(owner, repo),
            tag
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (tag: {})", tag))?;
        parse_gitlab_release(&json)
    }

    fn edit(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        let url = format!(
            "{}/projects/{}/releases/{}",
            self.base_url,
            Self::project_path(owner, repo),
            tag
        );
        let mut body = serde_json::Map::new();
        if let Some(n) = name {
            body.insert("name".into(), serde_json::Value::String(n.into()));
        }
        if let Some(d) = description {
            body.insert("description".into(), serde_json::Value::String(d.into()));
        }
        if body.is_empty() {
            return Err(ToriiError::Usage(
                "edit needs at least one of --name or --notes".to_string(),
            ));
        }
        let req = self
            .client()
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "GitLab edit release")
    }

    fn delete(&self, owner: &str, repo: &str, tag: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/releases/{}",
            self.base_url,
            Self::project_path(owner, repo),
            tag
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab delete release")
    }
}

pub(crate) fn parse_gitlab_release(v: &serde_json::Value) -> Result<Release> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    // ── parser ───────────────────────────────────────────────────────────

    #[test]
    fn parse_gitlab_release_full() {
        let json = serde_json::json!({
            "tag_name": "v0.9.2",
            "name": "Torii 0.9.2",
            "description": "Bug fixes.",
            "created_at": "2026-06-01T00:00:00Z",
            "_links": { "self": "https://gitlab.com/acme/widget/-/releases/v0.9.2" }
        });
        let r = parse_gitlab_release(&json).unwrap();
        assert_eq!(r.tag, "v0.9.2");
        assert_eq!(r.name, "Torii 0.9.2");
        assert_eq!(r.description, "Bug fixes.");
        assert_eq!(r.created_at, "2026-06-01T00:00:00Z");
        assert_eq!(
            r.web_url,
            "https://gitlab.com/acme/widget/-/releases/v0.9.2"
        );
        assert_eq!(r.id, None);
    }

    #[test]
    fn parse_gitlab_release_name_falls_back_to_tag() {
        let json = serde_json::json!({ "tag_name": "v1.0.0" });
        let r = parse_gitlab_release(&json).unwrap();
        assert_eq!(r.tag, "v1.0.0");
        assert_eq!(r.name, "v1.0.0");
        assert_eq!(r.description, "");
        assert_eq!(r.web_url, "");
    }

    // ── client (httpmock) ────────────────────────────────────────────────

    fn client(server: &MockServer) -> GitLabReleaseClient {
        GitLabReleaseClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn list_parses_releases() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/releases")
                .query_param("per_page", "10")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([{
                "tag_name": "v0.9.2", "name": "Torii 0.9.2",
                "description": "", "created_at": "",
                "_links": { "self": "https://x" }
            }]));
        });
        let releases = client(&server).list("acme", "widget", 10).unwrap();
        m.assert();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].tag, "v0.9.2");
    }

    #[test]
    fn delete_sends_delete_with_bearer_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(DELETE)
                .path("/projects/acme%2Fwidget/releases/v1.0.0")
                .header("Authorization", "Bearer test-token");
            then.status(200);
        });
        client(&server).delete("acme", "widget", "v1.0.0").unwrap();
        m.assert();
    }

    #[test]
    fn edit_without_fields_is_usage_error() {
        let server = MockServer::start();
        let err = client(&server)
            .edit("acme", "widget", "v1.0.0", None, None)
            .unwrap_err();
        assert!(
            matches!(err, ToriiError::Usage(_)),
            "expected Usage, got: {err:?}"
        );
    }

    #[test]
    fn get_non_2xx_maps_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/releases/v9.9.9");
            then.status(404)
                .json_body(serde_json::json!({ "message": "404 Not Found" }));
        });
        let err = client(&server).get("acme", "widget", "v9.9.9").unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
