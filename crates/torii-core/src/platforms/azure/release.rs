//! Azure DevOps — release client.

use crate::error::{Result, ToriiError};
use crate::platforms::release::*;
use reqwest::blocking::Client;

pub struct AzureReleaseClient {
    token: String,
}

impl AzureReleaseClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("azure", ".").value
            .ok_or_else(|| ToriiError::Auth { provider: "azure".into(), message: "Azure DevOps PAT not found. Create at https://dev.azure.com/{org}/_usersSettings/tokens \
                 with `Release (read/write)` scope, then: torii auth set azure YOUR_PAT".to_string() })?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
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
            org,
            project,
            limit.clamp(1, 100)
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Azure (url: {})", url))?;
        let arr = json["value"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "azure".into(),
                message: format!("Azure returned no `value` array. Body: {}", json),
            })?;
        let org_clone = org.clone();
        let project_clone = project.clone();
        arr.iter()
            .map(|v| parse_azure_release(v, &org_clone, &project_clone))
            .collect()
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
            let lookup_req = self
                .client()
                .get(&list_url)
                .header("Authorization", self.auth());
            let lookup_json = crate::http::send_json(lookup_req, "Azure lookup release by name")?;
            lookup_json["value"]
                .as_array()
                .and_then(|arr| arr.iter().find(|v| v["name"].as_str() == Some(tag_or_id)))
                .and_then(|v| v["id"].as_u64().map(|n| n.to_string()))
                .ok_or_else(|| {
                    ToriiError::InvalidConfig(format!(
                        "Azure: no release named '{}' in project {}",
                        tag_or_id, project
                    ))
                })?
        };
        let url = format!(
            "https://vsrm.dev.azure.com/{}/{}/_apis/release/releases/{}?api-version=7.0",
            org, project, id
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Azure release #{}", id))?;
        parse_azure_release(&json, &org, &project)
    }

    fn edit(
        &self,
        _o: &str,
        _r: &str,
        _tag: &str,
        _n: Option<&str>,
        _d: Option<&str>,
    ) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Azure Releases doesn't expose a mutation API for already-created releases — \
             metadata is derived from the release definition (template). Edit the definition \
             in the web UI; the next release will pick up the new metadata."
                .to_string(),
        ))
    }

    fn delete(&self, owner: &str, _repo: &str, tag_or_id: &str) -> Result<()> {
        let release = self.get(owner, "", tag_or_id)?;
        let id = release.id.ok_or_else(|| ToriiError::MalformedResponse {
            provider: "azure".into(),
            message: "Azure release missing id; cannot delete".to_string(),
        })?;
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://vsrm.dev.azure.com/{}/{}/_apis/release/releases/{}?api-version=7.0",
            org, project, id
        );
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth());
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
        web_url: id
            .as_ref()
            .map(|i| {
                format!(
                    "https://dev.azure.com/{}/{}/_releaseProgress?releaseId={}",
                    org, project, i
                )
            })
            .unwrap_or_default(),
        id,
    })
}

// ============================================================================
// Factory
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_azure_release_full() {
        let v = serde_json::json!({
            "id": 17u64,
            "name": "Release-3",
            "description": "Deploy to prod",
            "createdOn": "2026-02-03T04:05:06Z",
        });
        let r = parse_azure_release(&v, "org", "proj").unwrap();
        assert_eq!(r.id.as_deref(), Some("17"));
        // Azure releases have no git tag — the name doubles as both.
        assert_eq!(r.tag, "Release-3");
        assert_eq!(r.name, "Release-3");
        assert_eq!(r.description, "Deploy to prod");
        assert_eq!(r.created_at, "2026-02-03T04:05:06Z");
        assert_eq!(
            r.web_url,
            "https://dev.azure.com/org/proj/_releaseProgress?releaseId=17"
        );
    }

    #[test]
    fn parse_azure_release_missing_id_has_empty_web_url() {
        let v = serde_json::json!({ "name": "Release-4" });
        let r = parse_azure_release(&v, "org", "proj").unwrap();
        assert_eq!(r.id, None);
        assert_eq!(r.web_url, "");
        assert_eq!(r.description, "");
        assert_eq!(r.created_at, "");
    }

    #[test]
    fn parse_azure_release_minimal_defaults() {
        let v = serde_json::json!({});
        let r = parse_azure_release(&v, "org", "proj").unwrap();
        assert_eq!(r.tag, "");
        assert_eq!(r.name, "");
        assert_eq!(r.id, None);
    }
}
