//! GitLab — package client.

use crate::error::{Result, ToriiError};
use crate::platforms::package::*;
use reqwest::blocking::Client;

pub struct GitLabPackageClient {
    token: String,
    base_url: String,
}

impl GitLabPackageClient {
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

impl PackageClient for GitLabPackageClient {
    fn list(&self, owner: &str, repo: &str, filters: &PackageListFilters) -> Result<Vec<Package>> {
        let mut url = format!(
            "{}/projects/{}/packages?per_page={}",
            self.base_url,
            Self::project_path(owner, repo),
            filters.per_page.clamp(1, 100)
        );
        if let Some(t) = &filters.package_type {
            url.push_str(&format!("&package_type={}", t));
        }
        if let Some(n) = &filters.name_search {
            url.push_str(&format!("&package_name={}", crate::url::encode(n)));
        }
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_gitlab_package)
            .collect()
    }

    fn delete(&self, owner: &str, repo: &str, id: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/packages/{}",
            self.base_url,
            Self::project_path(owner, repo),
            id
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        crate::http::send_empty(req, "GitLab delete package")
    }

    fn list_files(&self, owner: &str, repo: &str, id: &str) -> Result<Vec<PackageFile>> {
        let url = format!(
            "{}/projects/{}/packages/{}/package_files?per_page=100",
            self.base_url,
            Self::project_path(owner, repo),
            id
        );
        let req = self
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(|v| parse_gitlab_package_file(v, id))
            .collect()
    }
}

pub(crate) fn parse_gitlab_package(v: &serde_json::Value) -> Result<Package> {
    let id = v["id"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "gitlab".into(),
            message: "GitLab package missing id".into(),
        })?;
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

pub(crate) fn parse_gitlab_package_file(
    v: &serde_json::Value,
    package_id: &str,
) -> Result<PackageFile> {
    let id = v["id"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| v["id"].as_str().map(String::from))
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "gitlab".into(),
            message: "GitLab package_file missing id".into(),
        })?;
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

#[cfg(test)]
mod tests {
    // NOTE: `parse_gitlab_package` / `parse_gitlab_package_file` are already
    // covered by the tests in `src/platforms/package.rs` — only the HTTP
    // client is tested here.
    use super::*;
    use httpmock::prelude::*;

    fn client(server: &MockServer) -> GitLabPackageClient {
        GitLabPackageClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn list_passes_type_and_name_filters() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/packages")
                .query_param("per_page", "20")
                .query_param("package_type", "generic")
                .query_param("package_name", "torii")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([{
                "id": 12345u64, "name": "torii", "version": "v0.9.2",
                "package_type": "generic", "status": "default",
                "created_at": "", "_links": { "web_path": "/acme/widget/-/packages/12345" }
            }]));
        });
        let filters = PackageListFilters {
            package_type: Some("generic".into()),
            name_search: Some("torii".into()),
            per_page: 20,
        };
        let packages = client(&server).list("acme", "widget", &filters).unwrap();
        m.assert();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].id, "12345");
        assert_eq!(packages[0].package_type, "generic");
    }

    #[test]
    fn list_files_parses_package_files() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/packages/12345/package_files")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([{
                "id": 99u64, "file_name": "torii-linux-x86_64",
                "size": 1024u64, "created_at": ""
            }]));
        });
        let files = client(&server)
            .list_files("acme", "widget", "12345")
            .unwrap();
        m.assert();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].package_id, "12345");
        assert_eq!(files[0].file_name, "torii-linux-x86_64");
    }

    #[test]
    fn delete_sends_delete_with_bearer_auth() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(DELETE)
                .path("/projects/acme%2Fwidget/packages/12345")
                .header("Authorization", "Bearer test-token");
            then.status(204);
        });
        client(&server).delete("acme", "widget", "12345").unwrap();
        m.assert();
    }

    #[test]
    fn list_files_non_2xx_maps_to_platform_api_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/packages/12345/package_files");
            then.status(500)
                .json_body(serde_json::json!({ "message": "boom" }));
        });
        let err = client(&server)
            .list_files("acme", "widget", "12345")
            .unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { .. }),
            "expected PlatformApi, got: {err:?}"
        );
    }
}
