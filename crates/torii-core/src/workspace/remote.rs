use crate::error::{Result, ToriiError};
use serde::{Deserialize, Serialize};

/// Remote repository visibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Internal, // GitLab only
}

/// Remote repository information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRepo {
    pub name: String,
    pub description: Option<String>,
    pub visibility: Visibility,
    pub default_branch: String,
    pub url: String,
    pub ssh_url: String,
    pub clone_url: String,
}

/// Platform-specific API client trait
pub trait PlatformClient {
    /// Create a new repository
    /// Create a repository.
    /// `namespace`: None → authenticated user's personal account.
    /// Some(owner) → organization (GitHub/Gitea/Forgejo/Codeberg) or
    /// group/subgroup path (GitLab).
    fn create_repo(
        &self,
        name: &str,
        description: Option<&str>,
        visibility: Visibility,
        namespace: Option<&str>,
    ) -> Result<RemoteRepo>;

    /// Delete a repository
    fn delete_repo(&self, owner: &str, repo: &str) -> Result<()>;

    /// Update repository settings
    fn update_repo(&self, owner: &str, repo: &str, settings: RepoSettings) -> Result<RemoteRepo>;

    /// Get repository information
    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo>;

    /// List user repositories
    fn list_repos(&self) -> Result<Vec<RemoteRepo>>;

    /// Set repository visibility
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()>;

    /// Enable/disable features
    fn configure_features(&self, owner: &str, repo: &str, features: RepoFeatures) -> Result<()>;
}

/// Repository settings for updates
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct RepoSettings {
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub visibility: Option<Visibility>,
    pub default_branch: Option<String>,
    pub has_issues: Option<bool>,
    pub has_wiki: Option<bool>,
    pub has_downloads: Option<bool>,
    pub allow_squash_merge: Option<bool>,
    pub allow_merge_commit: Option<bool>,
    pub allow_rebase_merge: Option<bool>,
}

/// Repository features configuration
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct RepoFeatures {
    pub issues: Option<bool>,
    pub wiki: Option<bool>,
    pub downloads: Option<bool>,
    pub projects: Option<bool>,
    pub discussions: Option<bool>,
}

/// GitHub API client (placeholder - requires reqwest)
#[allow(dead_code)]
pub struct GitHubClient {
    token: String,
    base_url: String,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        Self {
            token,
            base_url: "https://api.github.com".to_string(),
        }
    }

    fn get_token() -> Result<String> {
        crate::auth::resolve_token("github", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "github".into(),
                message: "GitHub token not found. Run: torii auth set github YOUR_TOKEN"
                    .to_string(),
            })
    }

    /// Map a GitHub repo JSON object (from the REST API) into a `RemoteRepo`.
    fn parse_repo(json: &serde_json::Value) -> RemoteRepo {
        let owner = json["owner"]["login"].as_str().unwrap_or("");
        let name = json["name"].as_str().unwrap_or("").to_string();
        let private = json["private"].as_bool().unwrap_or(true);
        RemoteRepo {
            name: name.clone(),
            description: json["description"].as_str().map(|s| s.to_string()),
            visibility: if private {
                Visibility::Private
            } else {
                Visibility::Public
            },
            default_branch: json["default_branch"]
                .as_str()
                .unwrap_or("main")
                .to_string(),
            url: json["html_url"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("https://github.com/{}/{}", owner, name)),
            ssh_url: json["ssh_url"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("git@github.com:{}/{}.git", owner, name)),
            clone_url: json["clone_url"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("https://github.com/{}/{}.git", owner, name)),
        }
    }
}

impl PlatformClient for GitHubClient {
    fn create_repo(
        &self,
        name: &str,
        description: Option<&str>,
        visibility: Visibility,
        namespace: Option<&str>,
    ) -> Result<RemoteRepo> {
        let private = matches!(visibility, Visibility::Private | Visibility::Internal);

        let mut body = serde_json::json!({
            "name": name,
            "private": private,
            "auto_init": false,
        });
        if let Some(desc) = description {
            body["description"] = serde_json::Value::String(desc.to_string());
        }

        // GitHub: org repos go through `/orgs/{org}/repos`. Personal repos
        // through `/user/repos`. Same body shape; endpoint switches.
        let url = match namespace {
            Some(org) => format!("https://api.github.com/orgs/{}/repos", org),
            None => "https://api.github.com/user/repos".to_string(),
        };

        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(&url)
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "torii-cli")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "github".into(),
                message: e.to_string(),
            })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let msg = resp.text().unwrap_or_default();
            return Err(ToriiError::PlatformApi {
                provider: "github".into(),
                status,
                message: msg,
            });
        }

        let json: serde_json::Value = resp.json().map_err(|e| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: format!("Failed to parse GitHub response: {}", e),
        })?;

        let repo_name = json["name"].as_str().unwrap_or(name).to_string();
        let owner = json["owner"]["login"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(RemoteRepo {
            name: repo_name.clone(),
            description: description.map(|s| s.to_string()),
            visibility,
            default_branch: "main".to_string(),
            url: format!("https://github.com/{}/{}", owner, repo_name),
            ssh_url: format!("git@github.com:{}/{}.git", owner, repo_name),
            clone_url: format!("https://github.com/{}/{}.git", owner, repo_name),
        })
    }

    fn delete_repo(&self, owner: &str, repo: &str) -> Result<()> {
        // Native API call — no longer requires `gh` to be installed.
        // Permissions: requires the token to have the `delete_repo` scope.
        let url = format!("https://api.github.com/repos/{}/{}", owner, repo);
        let resp = reqwest::blocking::Client::new()
            .delete(&url)
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "torii-cli")
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "github".into(),
                message: e.to_string(),
            })?;

        match resp.status().as_u16() {
            204 => {
                println!("✅ Repository deleted from GitHub");
                Ok(())
            }
            403 => Err(ToriiError::Auth {
                provider: "github".into(),
                message: format!(
                    "GitHub refused the delete (HTTP 403). Token needs the `delete_repo` scope; \
                 add it at https://github.com/settings/tokens or use a fine-grained token \
                 with `Administration: write` on `{}/{}`.",
                    owner, repo
                ),
            }),
            404 => Err(ToriiError::Auth {
                provider: "github".into(),
                message: format!(
                    "GitHub returned 404 for `{}/{}` — repo doesn't exist or token can't see it.",
                    owner, repo
                ),
            }),
            other => {
                let msg = resp.text().unwrap_or_default();
                Err(ToriiError::PlatformApi {
                    provider: "github".into(),
                    status: other,
                    message: msg,
                })
            }
        }
    }

    fn update_repo(&self, owner: &str, repo: &str, settings: RepoSettings) -> Result<RemoteRepo> {
        // Native API call — no longer requires `gh` to be installed.
        // Permissions: token needs `repo` scope (or fine-grained `Administration: write`).
        let url = format!("{}/repos/{}/{}", self.base_url, owner, repo);

        let mut body = serde_json::json!({});
        if let Some(desc) = &settings.description {
            body["description"] = serde_json::Value::String(desc.clone());
        }
        if let Some(homepage) = &settings.homepage {
            body["homepage"] = serde_json::Value::String(homepage.clone());
        }
        if let Some(vis) = &settings.visibility {
            // GitHub takes a `private` bool; Internal (GitLab-only) maps to private.
            let private = matches!(vis, Visibility::Private | Visibility::Internal);
            body["private"] = serde_json::Value::Bool(private);
        }
        if let Some(branch) = &settings.default_branch {
            body["default_branch"] = serde_json::Value::String(branch.clone());
        }

        let resp = reqwest::blocking::Client::new()
            .patch(&url)
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "torii-cli")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "github".into(),
                message: e.to_string(),
            })?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let msg = resp.text().unwrap_or_default();
            return Err(ToriiError::PlatformApi {
                provider: "github".into(),
                status,
                message: msg,
            });
        }

        let json: serde_json::Value = resp.json().map_err(|e| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: format!("Failed to parse GitHub response: {}", e),
        })?;

        println!("✅ Repository settings updated");
        Ok(Self::parse_repo(&json))
    }

    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo> {
        // Native API call — no longer requires `gh` to be installed.
        let url = format!("{}/repos/{}/{}", self.base_url, owner, repo);

        let resp = reqwest::blocking::Client::new()
            .get(&url)
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "torii-cli")
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "github".into(),
                message: e.to_string(),
            })?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let msg = resp.text().unwrap_or_default();
            return Err(ToriiError::PlatformApi {
                provider: "github".into(),
                status,
                message: msg,
            });
        }

        let json: serde_json::Value = resp.json().map_err(|e| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: format!("Failed to parse GitHub response: {}", e),
        })?;

        Ok(Self::parse_repo(&json))
    }

    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        // Native API call — no longer requires `gh` to be installed. Previously
        // this shelled out to `gh` and then returned an empty Vec regardless.
        let url = format!(
            "{}/user/repos?per_page=100&affiliation=owner",
            self.base_url
        );

        let resp = reqwest::blocking::Client::new()
            .get(&url)
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "torii-cli")
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "github".into(),
                message: e.to_string(),
            })?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let msg = resp.text().unwrap_or_default();
            return Err(ToriiError::PlatformApi {
                provider: "github".into(),
                status,
                message: msg,
            });
        }

        let json: serde_json::Value = resp.json().map_err(|e| ToriiError::MalformedResponse {
            provider: "github".into(),
            message: format!("Failed to parse GitHub response: {}", e),
        })?;

        let repos = json
            .as_array()
            .map(|arr| arr.iter().map(Self::parse_repo).collect())
            .unwrap_or_default();
        Ok(repos)
    }

    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        let settings = RepoSettings {
            visibility: Some(visibility),
            ..Default::default()
        };
        self.update_repo(owner, repo, settings)?;
        Ok(())
    }

    fn configure_features(&self, owner: &str, repo: &str, features: RepoFeatures) -> Result<()> {
        // Native API call — no longer requires `gh` to be installed.
        let url = format!("{}/repos/{}/{}", self.base_url, owner, repo);

        let mut body = serde_json::json!({});
        if let Some(issues) = features.issues {
            body["has_issues"] = serde_json::Value::Bool(issues);
        }
        if let Some(wiki) = features.wiki {
            body["has_wiki"] = serde_json::Value::Bool(wiki);
        }
        if let Some(projects) = features.projects {
            body["has_projects"] = serde_json::Value::Bool(projects);
        }
        if let Some(downloads) = features.downloads {
            body["has_downloads"] = serde_json::Value::Bool(downloads);
        }
        // GitHub Discussions can't be toggled via the repos PATCH endpoint, so
        // `features.discussions` is intentionally not sent here.

        let resp = reqwest::blocking::Client::new()
            .patch(&url)
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "torii-cli")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "github".into(),
                message: e.to_string(),
            })?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let msg = resp.text().unwrap_or_default();
            return Err(ToriiError::PlatformApi {
                provider: "github".into(),
                status,
                message: msg,
            });
        }

        println!("✅ Repository features configured");
        Ok(())
    }
}

/// GitLab API client (placeholder)
pub struct GitLabClient {
    token: Option<String>,
    base_url: String,
}

impl GitLabClient {
    pub fn new(token: Option<String>, base_url: Option<String>) -> Self {
        Self {
            token,
            base_url: base_url.unwrap_or_else(|| "https://gitlab.com/api/v4".to_string()),
        }
    }

    #[allow(dead_code)]
    pub fn with_url(token: String, base_url: String) -> Self {
        Self {
            token: Some(token),
            base_url,
        }
    }
}

impl PlatformClient for GitLabClient {
    fn create_repo(
        &self,
        name: &str,
        description: Option<&str>,
        visibility: Visibility,
        namespace: Option<&str>,
    ) -> Result<RemoteRepo> {
        let token = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "gitlab".into(),
            message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string(),
        })?;

        let visibility_str = match visibility {
            Visibility::Public => "public",
            Visibility::Private => "private",
            Visibility::Internal => "internal",
        };

        let mut body = serde_json::json!({
            "name": name,
            "path": name,  // url slug = name (GitLab default)
            "visibility": visibility_str,
        });

        if let Some(desc) = description {
            body["description"] = serde_json::json!(desc);
        }

        // GitLab: groups/subgroups need a numeric namespace_id. Resolve the
        // path → id via the groups API. Personal projects omit it.
        let client = reqwest::blocking::Client::new();
        if let Some(ns) = namespace {
            // GitLab namespaces can be groups (org-style) OR users (personal).
            // Try /groups/{ns} first; on 404 fall back to /users?username={ns}
            // because groups/<username> always 404s.
            let ns_encoded = crate::url::encode(ns);
            let group_url = format!("{}/groups/{}", self.base_url, ns_encoded);
            let group_resp = client
                .get(&group_url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .map_err(|e| ToriiError::Network {
                    provider: "gitlab".into(),
                    message: format!("group lookup failed: {}", e),
                })?;

            let ns_id = if group_resp.status().is_success() {
                let group: serde_json::Value =
                    group_resp
                        .json()
                        .map_err(|e| ToriiError::MalformedResponse {
                            provider: "gitlab".into(),
                            message: format!("GitLab group parse: {}", e),
                        })?;
                group["id"]
                    .as_i64()
                    .ok_or_else(|| ToriiError::MalformedResponse {
                        provider: "gitlab".into(),
                        message: format!("GitLab group `{}` returned no id", ns),
                    })?
            } else if group_resp.status().as_u16() == 404 {
                // Try as a user. /users?username=… returns an array.
                let user_url = format!("{}/users?username={}", self.base_url, ns_encoded);
                let user_resp = client
                    .get(&user_url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send()
                    .map_err(|e| ToriiError::Network {
                        provider: "gitlab".into(),
                        message: format!("user lookup failed: {}", e),
                    })?;
                if !user_resp.status().is_success() {
                    return Err(ToriiError::MalformedResponse {
                        provider: "gitlab".into(),
                        message: format!("namespace `{}` is neither a group nor a user", ns),
                    });
                }
                let users: serde_json::Value =
                    user_resp
                        .json()
                        .map_err(|e| ToriiError::MalformedResponse {
                            provider: "gitlab".into(),
                            message: format!("GitLab user parse: {}", e),
                        })?;
                let user = users.as_array().and_then(|a| a.first()).ok_or_else(|| {
                    ToriiError::Usage(format!("GitLab namespace `{}` not found", ns))
                })?;
                user["namespace_id"]
                    .as_i64()
                    .or_else(|| user["id"].as_i64())
                    .ok_or_else(|| ToriiError::MalformedResponse {
                        provider: "gitlab".into(),
                        message: format!("GitLab user `{}` returned no namespace_id", ns),
                    })?
            } else {
                let group_status = group_resp.status().as_u16();
                let err = group_resp.text().unwrap_or_default();
                return Err(ToriiError::PlatformApi {
                    provider: "gitlab".into(),
                    status: group_status,
                    message: format!("namespace `{}` lookup failed: {}", ns, err),
                });
            };
            body["namespace_id"] = serde_json::json!(ns_id);
        }

        let response = client
            .post(format!("{}/projects", self.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "gitlab".into(),
                message: format!("GitLab API request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        let project: serde_json::Value =
            response.json().map_err(|e| ToriiError::MalformedResponse {
                provider: "gitlab".into(),
                message: format!("Failed to parse GitLab response: {}", e),
            })?;

        Ok(RemoteRepo {
            name: project["name"].as_str().unwrap_or(name).to_string(),
            description: project["description"].as_str().map(|s| s.to_string()),
            visibility,
            default_branch: project["default_branch"]
                .as_str()
                .unwrap_or("main")
                .to_string(),
            url: project["web_url"].as_str().unwrap_or("").to_string(),
            ssh_url: project["ssh_url_to_repo"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            clone_url: project["http_url_to_repo"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        })
    }

    fn delete_repo(&self, owner: &str, repo: &str) -> Result<()> {
        let token = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "gitlab".into(),
            message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string(),
        })?;

        let path_str = format!("{}/{}", owner, repo);
        let project_path = crate::url::encode(&path_str);
        let client = reqwest::blocking::Client::new();
        let response = client
            .delete(format!("{}/projects/{}", self.base_url, project_path))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "gitlab".into(),
                message: format!("GitLab API request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        Ok(())
    }

    fn update_repo(&self, owner: &str, repo: &str, settings: RepoSettings) -> Result<RemoteRepo> {
        let token = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "gitlab".into(),
            message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string(),
        })?;

        let path_str = format!("{}/{}", owner, repo);
        let project_path = crate::url::encode(&path_str);
        let mut body = serde_json::json!({});

        if let Some(desc) = settings.description {
            body["description"] = serde_json::json!(desc);
        }
        if let Some(vis) = settings.visibility {
            let vis_str = match vis {
                Visibility::Public => "public",
                Visibility::Private => "private",
                Visibility::Internal => "internal",
            };
            body["visibility"] = serde_json::json!(vis_str);
        }
        if let Some(branch) = settings.default_branch {
            body["default_branch"] = serde_json::json!(branch);
        }

        let client = reqwest::blocking::Client::new();
        let response = client
            .put(format!("{}/projects/{}", self.base_url, project_path))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "gitlab".into(),
                message: format!("GitLab API request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        let project: serde_json::Value =
            response.json().map_err(|e| ToriiError::MalformedResponse {
                provider: "gitlab".into(),
                message: format!("Failed to parse GitLab response: {}", e),
            })?;

        let visibility = match project["visibility"].as_str() {
            Some("public") => Visibility::Public,
            Some("internal") => Visibility::Internal,
            _ => Visibility::Private,
        };

        Ok(RemoteRepo {
            name: project["name"].as_str().unwrap_or(repo).to_string(),
            description: project["description"].as_str().map(|s| s.to_string()),
            visibility,
            default_branch: project["default_branch"]
                .as_str()
                .unwrap_or("main")
                .to_string(),
            url: project["web_url"].as_str().unwrap_or("").to_string(),
            ssh_url: project["ssh_url_to_repo"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            clone_url: project["http_url_to_repo"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        })
    }

    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo> {
        let token = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "gitlab".into(),
            message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string(),
        })?;

        let path_str = format!("{}/{}", owner, repo);
        let project_path = crate::url::encode(&path_str);
        let client = reqwest::blocking::Client::new();
        let response = client
            .get(format!("{}/projects/{}", self.base_url, project_path))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "gitlab".into(),
                message: format!("GitLab API request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        let project: serde_json::Value =
            response.json().map_err(|e| ToriiError::MalformedResponse {
                provider: "gitlab".into(),
                message: format!("Failed to parse GitLab response: {}", e),
            })?;

        let visibility = match project["visibility"].as_str() {
            Some("public") => Visibility::Public,
            Some("internal") => Visibility::Internal,
            _ => Visibility::Private,
        };

        Ok(RemoteRepo {
            name: project["name"].as_str().unwrap_or(repo).to_string(),
            description: project["description"].as_str().map(|s| s.to_string()),
            visibility,
            default_branch: project["default_branch"]
                .as_str()
                .unwrap_or("main")
                .to_string(),
            url: project["web_url"].as_str().unwrap_or("").to_string(),
            ssh_url: project["ssh_url_to_repo"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            clone_url: project["http_url_to_repo"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        })
    }

    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        let token = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "gitlab".into(),
            message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string(),
        })?;

        let client = reqwest::blocking::Client::new();
        let response = client
            .get(format!(
                "{}/projects?membership=true&per_page=100",
                self.base_url
            ))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "gitlab".into(),
                message: format!("GitLab API request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        let projects: Vec<serde_json::Value> =
            response.json().map_err(|e| ToriiError::MalformedResponse {
                provider: "gitlab".into(),
                message: format!("Failed to parse GitLab response: {}", e),
            })?;

        Ok(projects
            .iter()
            .map(|project| {
                let visibility = match project["visibility"].as_str() {
                    Some("public") => Visibility::Public,
                    Some("internal") => Visibility::Internal,
                    _ => Visibility::Private,
                };

                RemoteRepo {
                    name: project["name"].as_str().unwrap_or("").to_string(),
                    description: project["description"].as_str().map(|s| s.to_string()),
                    visibility,
                    default_branch: project["default_branch"]
                        .as_str()
                        .unwrap_or("main")
                        .to_string(),
                    url: project["web_url"].as_str().unwrap_or("").to_string(),
                    ssh_url: project["ssh_url_to_repo"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    clone_url: project["http_url_to_repo"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                }
            })
            .collect())
    }

    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        let token = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "gitlab".into(),
            message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string(),
        })?;

        let path_str = format!("{}/{}", owner, repo);
        let project_path = crate::url::encode(&path_str);
        let visibility_str = match visibility {
            Visibility::Public => "public",
            Visibility::Private => "private",
            Visibility::Internal => "internal",
        };

        let body = serde_json::json!({
            "visibility": visibility_str,
        });

        let client = reqwest::blocking::Client::new();
        let response = client
            .put(format!("{}/projects/{}", self.base_url, project_path))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "gitlab".into(),
                message: format!("GitLab API request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        Ok(())
    }

    fn configure_features(&self, owner: &str, repo: &str, features: RepoFeatures) -> Result<()> {
        let token = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "gitlab".into(),
            message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string(),
        })?;

        let path_str = format!("{}/{}", owner, repo);
        let project_path = crate::url::encode(&path_str);
        let mut body = serde_json::json!({});

        if let Some(issues) = features.issues {
            body["issues_enabled"] = serde_json::json!(issues);
        }
        if let Some(wiki) = features.wiki {
            body["wiki_enabled"] = serde_json::json!(wiki);
        }

        let client = reqwest::blocking::Client::new();
        let response = client
            .put(format!("{}/projects/{}", self.base_url, project_path))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::Network {
                provider: "gitlab".into(),
                message: format!("GitLab API request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        Ok(())
    }
}

/// Get appropriate platform client based on platform name
pub fn get_platform_client(platform: &str) -> Result<Box<dyn PlatformClient>> {
    match platform.to_lowercase().as_str() {
        "github" => {
            let token = GitHubClient::get_token()?;
            Ok(Box::new(GitHubClient::new(token)))
        }
        "gitlab" => {
            let token = crate::auth::resolve_token("gitlab", ".").value
                .ok_or_else(|| ToriiError::Auth { provider: "gitlab".into(), message: "GitLab token not found. Run: torii auth set gitlab YOUR_TOKEN".to_string() })?;
            let base_url = std::env::var("GITLAB_URL").ok();
            Ok(Box::new(GitLabClient::new(Some(token), base_url)))
        }
        "gitea" => {
            // Codeberg and Forgejo share the Gitea API — accept the
            // Codeberg/Forgejo token as a fallback so a single
            // `torii auth set codeberg ...` works.
            let token = crate::auth::resolve_token("gitea", ".").value
                .or_else(|| crate::auth::resolve_token("codeberg", ".").value)
                .or_else(|| crate::auth::resolve_token("forgejo", ".").value);
            let base_url = std::env::var("GITEA_URL")
                .unwrap_or_else(|_| "https://gitea.com".to_string());
            Ok(Box::new(GiteaClient::new(token, base_url)))
        }
        "forgejo" => {
            let token = crate::auth::resolve_token("forgejo", ".").value
                .or_else(|| crate::auth::resolve_token("gitea", ".").value)
                .or_else(|| crate::auth::resolve_token("codeberg", ".").value);
            let base_url = std::env::var("FORGEJO_URL")
                .unwrap_or_else(|_| "https://codeberg.org".to_string());
            Ok(Box::new(ForgejoClient::new(token, base_url)))
        }
        "codeberg" => {
            let token = crate::auth::resolve_token("codeberg", ".").value
                .or_else(|| crate::auth::resolve_token("gitea", ".").value)
                .or_else(|| crate::auth::resolve_token("forgejo", ".").value);
            Ok(Box::new(CodebergClient::new(token)))
        }
        "bitbucket" => {
            let token = crate::auth::resolve_token("bitbucket", ".").value;
            Ok(Box::new(BitbucketClient::new(token)))
        }
        "sourcehut" => {
            let token = crate::auth::resolve_token("sourcehut", ".").value;
            Ok(Box::new(SourcehutClient::new(token)))
        }
        "azure" => {
            let token = crate::auth::resolve_token("azure", ".").value;
            Ok(Box::new(AzureClient::new(token)))
        }
        "radicle" => {
            Ok(Box::new(RadicleClient::new()))
        }
        _ => Err(ToriiError::Unsupported(
            format!(
                "Unsupported platform: {}. Supported: github, gitlab, gitea, forgejo, codeberg, bitbucket, sourcehut, azure, radicle",
                platform
            )
        )),
    }
}

// ============================================================================
// Gitea/Forgejo/Codeberg Clients
// ============================================================================

#[allow(dead_code)]
pub struct GiteaClient {
    token: Option<String>,
    base_url: String,
}

impl GiteaClient {
    pub fn new(token: Option<String>, base_url: String) -> Self {
        Self { token, base_url }
    }
}

#[allow(dead_code)]
pub struct ForgejoClient {
    token: Option<String>,
    base_url: String,
}

impl ForgejoClient {
    pub fn new(token: Option<String>, base_url: String) -> Self {
        Self { token, base_url }
    }
}

#[allow(dead_code)]
pub struct CodebergClient {
    token: Option<String>,
    base_url: String,
}

impl CodebergClient {
    pub fn new(token: Option<String>) -> Self {
        Self {
            token,
            base_url: "https://codeberg.org".to_string(),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Gitea / Forgejo / Codeberg — all three share the Gitea API.
//
// We implement the shared bits as free functions and delegate from each
// client. Only the base URL differs (gitea.com / codeberg.org / a
// self-hosted Forgejo instance via FORGEJO_URL / GITEA_URL env vars).

fn gitea_token<'a>(label: &str, token: &'a Option<String>) -> Result<&'a String> {
    token.as_ref().ok_or_else(|| ToriiError::Auth {
        provider: label.to_lowercase(),
        message: format!(
            "{label} token not found. Run: torii auth set {} YOUR_TOKEN",
            label.to_lowercase()
        ),
    })
}

/// One repository as the Gitea API describes it. Gitea, Forgejo and
/// Codeberg answer with the same shape, so they share the parse.
fn gitea_parse_repo(json: &serde_json::Value) -> RemoteRepo {
    let private = json["private"].as_bool().unwrap_or(true);
    RemoteRepo {
        name: json["name"].as_str().unwrap_or("").to_string(),
        description: json["description"].as_str().and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }),
        visibility: if private {
            Visibility::Private
        } else {
            Visibility::Public
        },
        default_branch: json["default_branch"]
            .as_str()
            .unwrap_or("main")
            .to_string(),
        url: json["html_url"].as_str().unwrap_or("").to_string(),
        ssh_url: json["ssh_url"].as_str().unwrap_or("").to_string(),
        clone_url: json["clone_url"].as_str().unwrap_or("").to_string(),
    }
}

/// Repositories the token's own account can reach, on any host speaking
/// the Gitea API (Gitea, Forgejo, Codeberg).
///
/// The endpoint is paged and the host decides the page size it is
/// willing to serve, so the loop cannot trust `limit` to be honoured:
/// it stops on the first page that comes back short, and at a ceiling
/// of 20 pages, which is a thousand repositories and a place a
/// misbehaving host cannot spin forever.
fn gitea_list_repos(
    base_url: &str,
    token: &Option<String>,
    label: &str,
) -> Result<Vec<RemoteRepo>> {
    const PAGE: usize = 50;
    let tok = gitea_token(label, token)?;
    let base = base_url.trim_end_matches('/');
    let ctx = format!("{} list repos", label);
    let mut repos: Vec<RemoteRepo> = Vec::new();

    for page in 1..=20 {
        let url = format!("{}/api/v1/user/repos?limit={}&page={}", base, PAGE, page);
        let req = crate::http::make_client()
            .get(&url)
            .header("Authorization", format!("token {}", tok))
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, &ctx)?;
        let arr = crate::http::extract_array(&json, &ctx)?;
        let got = arr.len();
        repos.extend(arr.iter().map(gitea_parse_repo));
        if got < PAGE {
            break;
        }
    }

    Ok(repos)
}

fn gitea_set_visibility(
    base_url: &str,
    token: &Option<String>,
    owner: &str,
    repo: &str,
    visibility: Visibility,
    label: &str,
) -> Result<()> {
    // Gitea visibility is just a `private` boolean. "Internal" doesn't
    // exist on Gitea — collapse to private.
    let private = !matches!(visibility, Visibility::Public);
    let tok = gitea_token(label, token)?;
    let url = format!(
        "{}/api/v1/repos/{}/{}",
        base_url.trim_end_matches('/'),
        owner,
        repo
    );
    let body = serde_json::json!({ "private": private });
    let req = crate::http::make_client()
        .patch(&url)
        .header("Authorization", format!("token {}", tok))
        .header("Accept", "application/json")
        .json(&body);
    crate::http::send_empty(req, &format!("{} set visibility", label))
}

fn gitea_repo_url(base_url: &str, owner: &str, repo: &str) -> String {
    format!(
        "{}/api/v1/repos/{}/{}",
        base_url.trim_end_matches('/'),
        owner,
        repo
    )
}

fn gitea_create_repo(
    base_url: &str,
    token: &Option<String>,
    label: &str,
    name: &str,
    description: Option<&str>,
    visibility: Visibility,
    namespace: Option<&str>,
) -> Result<RemoteRepo> {
    let tok = gitea_token(label, token)?;
    let base = base_url.trim_end_matches('/');
    // Gitea has no Internal; collapse it the way set_visibility does.
    let private = !matches!(visibility, Visibility::Public);
    let mut body = serde_json::json!({
        "name": name,
        "private": private,
        "auto_init": false,
    });
    if let Some(desc) = description {
        body["description"] = serde_json::Value::String(desc.to_string());
    }
    let url = match namespace {
        Some(org) => format!("{base}/api/v1/orgs/{org}/repos"),
        None => format!("{base}/api/v1/user/repos"),
    };
    let req = crate::http::make_client()
        .post(&url)
        .header("Authorization", format!("token {tok}"))
        .header("Accept", "application/json")
        .json(&body);
    let json = crate::http::send_json(req, &format!("{label} create repo"))?;
    Ok(gitea_parse_repo(&json))
}

fn gitea_delete_repo(
    base_url: &str,
    token: &Option<String>,
    owner: &str,
    repo: &str,
    label: &str,
) -> Result<()> {
    let tok = gitea_token(label, token)?;
    let url = gitea_repo_url(base_url, owner, repo);
    let req = crate::http::make_client()
        .delete(&url)
        .header("Authorization", format!("token {tok}"))
        .header("Accept", "application/json");
    crate::http::send_empty(req, &format!("{label} delete repo"))
}

fn gitea_get_repo(
    base_url: &str,
    token: &Option<String>,
    owner: &str,
    repo: &str,
    label: &str,
) -> Result<RemoteRepo> {
    let tok = gitea_token(label, token)?;
    let url = gitea_repo_url(base_url, owner, repo);
    let req = crate::http::make_client()
        .get(&url)
        .header("Authorization", format!("token {tok}"))
        .header("Accept", "application/json");
    let json = crate::http::send_json(req, &format!("{label} get repo"))?;
    Ok(gitea_parse_repo(&json))
}

fn gitea_update_repo(
    base_url: &str,
    token: &Option<String>,
    owner: &str,
    repo: &str,
    settings: RepoSettings,
    label: &str,
) -> Result<RemoteRepo> {
    let tok = gitea_token(label, token)?;
    let url = gitea_repo_url(base_url, owner, repo);
    let mut body = serde_json::json!({});
    if let Some(desc) = settings.description {
        body["description"] = serde_json::Value::String(desc);
    }
    if let Some(homepage) = settings.homepage {
        body["website"] = serde_json::Value::String(homepage);
    }
    if let Some(vis) = settings.visibility {
        body["private"] = serde_json::Value::Bool(!matches!(vis, Visibility::Public));
    }
    if let Some(branch) = settings.default_branch {
        body["default_branch"] = serde_json::Value::String(branch);
    }
    let req = crate::http::make_client()
        .patch(&url)
        .header("Authorization", format!("token {tok}"))
        .header("Accept", "application/json")
        .json(&body);
    let json = crate::http::send_json(req, &format!("{label} update repo"))?;
    Ok(gitea_parse_repo(&json))
}

fn gitea_configure_features(
    base_url: &str,
    token: &Option<String>,
    owner: &str,
    repo: &str,
    features: RepoFeatures,
    label: &str,
) -> Result<()> {
    let tok = gitea_token(label, token)?;
    let url = gitea_repo_url(base_url, owner, repo);
    let mut body = serde_json::json!({});
    if let Some(issues) = features.issues {
        body["has_issues"] = serde_json::Value::Bool(issues);
    }
    if let Some(wiki) = features.wiki {
        body["has_wiki"] = serde_json::Value::Bool(wiki);
    }
    if let Some(projects) = features.projects {
        body["has_projects"] = serde_json::Value::Bool(projects);
    }
    let req = crate::http::make_client()
        .patch(&url)
        .header("Authorization", format!("token {tok}"))
        .header("Accept", "application/json")
        .json(&body);
    crate::http::send_empty(req, &format!("{label} configure features"))
}

impl PlatformClient for GiteaClient {
    fn create_repo(
        &self,
        name: &str,
        description: Option<&str>,
        visibility: Visibility,
        namespace: Option<&str>,
    ) -> Result<RemoteRepo> {
        gitea_create_repo(
            &self.base_url,
            &self.token,
            "Gitea",
            name,
            description,
            visibility,
            namespace,
        )
    }
    fn delete_repo(&self, owner: &str, repo: &str) -> Result<()> {
        gitea_delete_repo(&self.base_url, &self.token, owner, repo, "Gitea")
    }
    fn update_repo(&self, owner: &str, repo: &str, settings: RepoSettings) -> Result<RemoteRepo> {
        gitea_update_repo(&self.base_url, &self.token, owner, repo, settings, "Gitea")
    }
    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo> {
        gitea_get_repo(&self.base_url, &self.token, owner, repo, "Gitea")
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        gitea_list_repos(&self.base_url, &self.token, "Gitea")
    }
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        gitea_set_visibility(
            &self.base_url,
            &self.token,
            owner,
            repo,
            visibility,
            "Gitea",
        )
    }
    fn configure_features(&self, owner: &str, repo: &str, features: RepoFeatures) -> Result<()> {
        gitea_configure_features(&self.base_url, &self.token, owner, repo, features, "Gitea")
    }
}

impl PlatformClient for ForgejoClient {
    fn create_repo(
        &self,
        name: &str,
        description: Option<&str>,
        visibility: Visibility,
        namespace: Option<&str>,
    ) -> Result<RemoteRepo> {
        gitea_create_repo(
            &self.base_url,
            &self.token,
            "Forgejo",
            name,
            description,
            visibility,
            namespace,
        )
    }
    fn delete_repo(&self, owner: &str, repo: &str) -> Result<()> {
        gitea_delete_repo(&self.base_url, &self.token, owner, repo, "Forgejo")
    }
    fn update_repo(&self, owner: &str, repo: &str, settings: RepoSettings) -> Result<RemoteRepo> {
        gitea_update_repo(
            &self.base_url,
            &self.token,
            owner,
            repo,
            settings,
            "Forgejo",
        )
    }
    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo> {
        gitea_get_repo(&self.base_url, &self.token, owner, repo, "Forgejo")
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        gitea_list_repos(&self.base_url, &self.token, "Forgejo")
    }
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        gitea_set_visibility(
            &self.base_url,
            &self.token,
            owner,
            repo,
            visibility,
            "Forgejo",
        )
    }
    fn configure_features(&self, owner: &str, repo: &str, features: RepoFeatures) -> Result<()> {
        gitea_configure_features(
            &self.base_url,
            &self.token,
            owner,
            repo,
            features,
            "Forgejo",
        )
    }
}

impl PlatformClient for CodebergClient {
    fn create_repo(
        &self,
        name: &str,
        description: Option<&str>,
        visibility: Visibility,
        namespace: Option<&str>,
    ) -> Result<RemoteRepo> {
        gitea_create_repo(
            &self.base_url,
            &self.token,
            "Codeberg",
            name,
            description,
            visibility,
            namespace,
        )
    }
    fn delete_repo(&self, owner: &str, repo: &str) -> Result<()> {
        gitea_delete_repo(&self.base_url, &self.token, owner, repo, "Codeberg")
    }
    fn update_repo(&self, owner: &str, repo: &str, settings: RepoSettings) -> Result<RemoteRepo> {
        gitea_update_repo(
            &self.base_url,
            &self.token,
            owner,
            repo,
            settings,
            "Codeberg",
        )
    }
    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo> {
        gitea_get_repo(&self.base_url, &self.token, owner, repo, "Codeberg")
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        gitea_list_repos(&self.base_url, &self.token, "Codeberg")
    }
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        gitea_set_visibility(
            &self.base_url,
            &self.token,
            owner,
            repo,
            visibility,
            "Codeberg",
        )
    }
    fn configure_features(&self, owner: &str, repo: &str, features: RepoFeatures) -> Result<()> {
        gitea_configure_features(
            &self.base_url,
            &self.token,
            owner,
            repo,
            features,
            "Codeberg",
        )
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Bitbucket Cloud — `PUT /2.0/repositories/{ws}/{repo}` with `is_private`.

#[allow(dead_code)]
pub struct BitbucketClient {
    token: Option<String>,
}

impl BitbucketClient {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }

    fn auth(&self) -> Result<String> {
        let tok = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "bitbucket".into(),
            message:
                "Bitbucket token not found. Run: torii auth set bitbucket USERNAME:APP_PASSWORD"
                    .to_string(),
        })?;
        if tok.contains(':') {
            use base64::Engine;
            Ok(format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(tok)
            ))
        } else {
            Ok(format!("Bearer {}", tok))
        }
    }
}

fn bitbucket_clone_link(json: &serde_json::Value, kind: &str) -> String {
    json["links"]["clone"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|l| l["name"].as_str() == Some(kind))
                .and_then(|l| l["href"].as_str())
        })
        .unwrap_or("")
        .to_string()
}

fn bitbucket_parse_repo(json: &serde_json::Value) -> RemoteRepo {
    let private = json["is_private"].as_bool().unwrap_or(true);
    RemoteRepo {
        name: json["name"].as_str().unwrap_or("").to_string(),
        description: json["description"].as_str().and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }),
        visibility: if private {
            Visibility::Private
        } else {
            Visibility::Public
        },
        default_branch: json["mainbranch"]["name"]
            .as_str()
            .unwrap_or("main")
            .to_string(),
        url: json["links"]["html"]["href"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        ssh_url: bitbucket_clone_link(json, "ssh"),
        clone_url: bitbucket_clone_link(json, "https"),
    }
}

/// Repositories the token can see (`role=member`), following Bitbucket's
/// `next` links. Same 20-page ceiling as the Gitea lister so a broken
/// `next` cannot spin.
fn bitbucket_list_repos(api_base: &str, auth: &str) -> Result<Vec<RemoteRepo>> {
    let mut url = Some(format!(
        "{}/repositories?role=member&pagelen=50",
        api_base.trim_end_matches('/')
    ));
    let mut repos = Vec::new();
    for _ in 0..20 {
        let Some(page) = url.take() else {
            break;
        };
        let req = crate::http::make_client()
            .get(&page)
            .header("Authorization", auth)
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, "Bitbucket list repos")?;
        let arr = json["values"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "bitbucket".into(),
                message: format!("Bitbucket returned no `values` array. Body: {json}"),
            })?;
        repos.extend(arr.iter().map(bitbucket_parse_repo));
        url = json["next"].as_str().map(str::to_string);
        if url.is_none() {
            break;
        }
    }
    Ok(repos)
}

impl PlatformClient for BitbucketClient {
    fn create_repo(
        &self,
        _n: &str,
        _d: Option<&str>,
        _v: Visibility,
        _ns: Option<&str>,
    ) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Bitbucket create_repo not yet wired".to_string(),
        ))
    }
    fn delete_repo(&self, _o: &str, _r: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Bitbucket delete_repo not yet wired".to_string(),
        ))
    }
    fn update_repo(&self, _o: &str, _r: &str, _s: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Bitbucket update_repo not yet wired".to_string(),
        ))
    }
    fn get_repo(&self, _o: &str, _r: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Bitbucket get_repo not yet wired".to_string(),
        ))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        bitbucket_list_repos("https://api.bitbucket.org/2.0", &self.auth()?)
    }
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        // Bitbucket: PUT /2.0/repositories/{ws}/{repo} with `is_private`.
        // "Internal" doesn't exist — collapse to private.
        let is_private = !matches!(visibility, Visibility::Public);
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}",
            owner, repo
        );
        let body = serde_json::json!({ "is_private": is_private });
        let req = crate::http::make_client()
            .put(&url)
            .header("Authorization", self.auth()?)
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Bitbucket set visibility")
    }
    fn configure_features(&self, _o: &str, _r: &str, _f: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Bitbucket configure_features not yet wired".to_string(),
        ))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Sourcehut — `meta.sr.ht` GraphQL endpoint for visibility updates.

#[allow(dead_code)]
pub struct SourcehutClient {
    token: Option<String>,
}

impl SourcehutClient {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }

    fn token(&self) -> Result<&str> {
        self.token.as_deref().ok_or_else(|| ToriiError::Auth {
            provider: "sourcehut".into(),
            message: "Sourcehut token not found. Run: torii auth set sourcehut YOUR_TOKEN"
                .to_string(),
        })
    }
}

fn sourcehut_parse_repo(json: &serde_json::Value, fallback_owner: &str) -> RemoteRepo {
    let owner = json["owner"]["canonical_name"]
        .as_str()
        .or_else(|| json["owner"]["name"].as_str())
        .unwrap_or(fallback_owner);
    let owner = if owner.is_empty() {
        fallback_owner
    } else {
        owner
    };
    let name = json["name"].as_str().unwrap_or("");
    let vis = json["visibility"].as_str().unwrap_or("private");
    let visibility = match vis {
        "public" => Visibility::Public,
        "unlisted" => Visibility::Internal,
        _ => Visibility::Private,
    };
    let web = format!("https://git.sr.ht/{owner}/{name}");
    RemoteRepo {
        name: name.to_string(),
        description: json["description"].as_str().and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }),
        visibility,
        default_branch: "main".to_string(),
        url: web,
        ssh_url: format!("git@git.sr.ht:{owner}/{name}"),
        clone_url: format!("https://git.sr.ht/{owner}/{name}"),
    }
}

/// Authenticated user's canonical name (`~user`) from meta.sr.ht, used
/// when a list item does not carry its owner.
fn sourcehut_whoami(meta_base: &str, token: &str) -> Result<String> {
    let url = format!("{}/api/user", meta_base.trim_end_matches('/'));
    let req = crate::http::make_client()
        .get(&url)
        .header("Authorization", format!("token {token}"));
    let json = crate::http::send_json(req, "Sourcehut user")?;
    if let Some(name) = json["canonical_name"].as_str() {
        return Ok(name.to_string());
    }
    match json["name"].as_str() {
        Some(n) if !n.is_empty() => Ok(format!("~{n}")),
        _ => Err(ToriiError::MalformedResponse {
            provider: "sourcehut".into(),
            message: format!("meta.sr.ht user had no name. Body: {json}"),
        }),
    }
}

/// Repositories the token owns on git.sr.ht. The list is paged; `next`
/// is either a URL, a start id, or null. Same 20-page ceiling as Gitea.
fn sourcehut_list_repos(git_base: &str, meta_base: &str, token: &str) -> Result<Vec<RemoteRepo>> {
    let owner = sourcehut_whoami(meta_base, token).unwrap_or_default();
    let mut start: Option<String> = None;
    let mut next_url: Option<String> = None;
    let mut repos = Vec::new();
    for _ in 0..20 {
        let page = if let Some(u) = next_url.take() {
            u
        } else {
            let mut url = format!("{}/api/repos", git_base.trim_end_matches('/'));
            if let Some(s) = &start {
                url.push_str(&format!("?start={s}"));
            }
            url
        };
        let req = crate::http::make_client()
            .get(&page)
            .header("Authorization", format!("token {token}"));
        let json = crate::http::send_json(req, "Sourcehut list repos")?;
        let arr = json["results"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "sourcehut".into(),
                message: format!("Sourcehut returned no `results` array. Body: {json}"),
            })?;
        let got = arr.len();
        repos.extend(arr.iter().map(|v| sourcehut_parse_repo(v, &owner)));
        match json.get("next") {
            None | Some(serde_json::Value::Null) => break,
            Some(serde_json::Value::String(s)) if s.starts_with("http") => {
                next_url = Some(s.clone());
                start = None;
            }
            Some(serde_json::Value::String(s)) if !s.is_empty() => {
                start = Some(s.clone());
            }
            Some(serde_json::Value::Number(n)) => {
                start = Some(n.to_string());
            }
            _ => break,
        }
        if got == 0 {
            break;
        }
    }
    Ok(repos)
}

impl PlatformClient for SourcehutClient {
    fn create_repo(
        &self,
        _n: &str,
        _d: Option<&str>,
        _v: Visibility,
        _ns: Option<&str>,
    ) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Sourcehut create_repo not yet wired".to_string(),
        ))
    }
    fn delete_repo(&self, _o: &str, _r: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Sourcehut delete_repo not yet wired".to_string(),
        ))
    }
    fn update_repo(&self, _o: &str, _r: &str, _s: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Sourcehut update_repo not yet wired".to_string(),
        ))
    }
    fn get_repo(&self, _o: &str, _r: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Sourcehut get_repo not yet wired".to_string(),
        ))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        sourcehut_list_repos("https://git.sr.ht", "https://meta.sr.ht", self.token()?)
    }
    fn set_visibility(&self, _owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        // git.sr.ht exposes visibility via a GraphQL mutation. Three
        // values: PUBLIC, UNLISTED, PRIVATE. We collapse torii's
        // (Public, Private, Internal) → (PUBLIC, PRIVATE, UNLISTED).
        let tok = self.token.as_ref().ok_or_else(|| ToriiError::Auth {
            provider: "sourcehut".into(),
            message: "Sourcehut token not found. Run: torii auth set sourcehut YOUR_TOKEN"
                .to_string(),
        })?;
        let target = match visibility {
            Visibility::Public => "PUBLIC",
            Visibility::Private => "PRIVATE",
            Visibility::Internal => "UNLISTED",
        };
        // git.sr.ht GraphQL is at https://git.sr.ht/query
        let query = serde_json::json!({
            "query": "mutation Update($name: String!, $visibility: Visibility!) { \
                       updateRepository(name: $name, input: { visibility: $visibility }) { id } }",
            "variables": { "name": repo, "visibility": target }
        });
        let req = crate::http::make_client()
            .post("https://git.sr.ht/query")
            .header("Authorization", format!("Bearer {}", tok))
            .header("Accept", "application/json")
            .json(&query);
        // GraphQL servers always return 200 even on logical errors —
        // send_json then check for `errors`.
        let resp = crate::http::send_json(req, "Sourcehut set visibility")?;
        if let Some(errs) = resp.get("errors").and_then(|e| e.as_array()) {
            if !errs.is_empty() {
                return Err(ToriiError::MalformedResponse {
                    provider: "sourcehut".into(),
                    message: format!(
                        "GraphQL errors: {}",
                        serde_json::to_string(errs).unwrap_or_default()
                    ),
                });
            }
        }
        Ok(())
    }
    fn configure_features(&self, _o: &str, _r: &str, _f: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Sourcehut configure_features not yet wired".to_string(),
        ))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Azure DevOps — visibility lives at the *project* level on Azure, not
// per-repo. We surface that clearly so the user knows where to go.

pub struct AzureClient {
    token: Option<String>,
}

impl Default for AzureClient {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AzureClient {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }

    fn token(&self) -> Result<&str> {
        self.token.as_deref().ok_or_else(|| ToriiError::Auth {
            provider: "azure".into(),
            message: "Azure DevOps PAT not found. Create one at \
                 https://dev.azure.com/{org}/_usersSettings/tokens and run: \
                 torii auth set azure YOUR_PAT"
                .to_string(),
        })
    }

    fn auth_header(token: &str) -> String {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(format!(":{token}"));
        format!("Basic {b64}")
    }
}

fn azure_parse_repo(json: &serde_json::Value) -> RemoteRepo {
    let name = json["name"].as_str().unwrap_or("").to_string();
    let org_project = json["project"]["name"].as_str().unwrap_or("");
    let web = json["webUrl"].as_str().unwrap_or("");
    // Drop the `_git` segment so the clone view's slug is `org/project/repo`
    // rather than `org/project/_git/repo`.
    let url = web.replace("/_git/", "/");
    let url = if url.is_empty() {
        format!("https://dev.azure.com/{org_project}/{name}")
    } else {
        url
    };
    let default_branch = json["defaultBranch"]
        .as_str()
        .unwrap_or("refs/heads/main")
        .trim_start_matches("refs/heads/")
        .to_string();
    let vis = json["project"]["visibility"].as_str().unwrap_or("private");
    RemoteRepo {
        name,
        description: None,
        visibility: if vis.eq_ignore_ascii_case("public") {
            Visibility::Public
        } else {
            Visibility::Private
        },
        default_branch,
        url,
        ssh_url: json["sshUrl"].as_str().unwrap_or("").to_string(),
        clone_url: json["remoteUrl"].as_str().unwrap_or("").to_string(),
    }
}

fn azure_list_repos_in_org(dev_base: &str, org: &str, auth: &str) -> Result<Vec<RemoteRepo>> {
    let url = format!(
        "{}/{}/_apis/git/repositories?api-version=7.0",
        dev_base.trim_end_matches('/'),
        org
    );
    let req = crate::http::make_client()
        .get(&url)
        .header("Authorization", auth)
        .header("Accept", "application/json");
    let json = crate::http::send_json(req, "Azure list repos")?;
    let arr = json["value"]
        .as_array()
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "azure".into(),
            message: format!("Azure returned no `value` array. Body: {json}"),
        })?;
    Ok(arr.iter().map(azure_parse_repo).collect())
}

/// Organisations the PAT can see. `AZURE_DEVOPS_ORG` / `AZDO_ORG` skip
/// the profile lookup when the user already knows which org to ask.
fn azure_org_from_env() -> Option<String> {
    std::env::var("AZURE_DEVOPS_ORG")
        .or_else(|_| std::env::var("AZDO_ORG"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn azure_orgs_from_api(vssps_base: &str, auth: &str) -> Result<Vec<String>> {
    let me_url = format!(
        "{}/_apis/profile/profiles/me?api-version=7.0",
        vssps_base.trim_end_matches('/')
    );
    let me_req = crate::http::make_client()
        .get(&me_url)
        .header("Authorization", auth)
        .header("Accept", "application/json");
    let me = crate::http::send_json(me_req, "Azure profile")?;
    let id = me["id"]
        .as_str()
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "azure".into(),
            message: format!("Azure profile had no id. Body: {me}"),
        })?;
    let acc_url = format!(
        "{}/_apis/accounts?memberId={id}&api-version=7.0",
        vssps_base.trim_end_matches('/')
    );
    let acc_req = crate::http::make_client()
        .get(&acc_url)
        .header("Authorization", auth)
        .header("Accept", "application/json");
    let acc = crate::http::send_json(acc_req, "Azure accounts")?;
    let names: Vec<String> = acc["value"]
        .as_array()
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "azure".into(),
            message: format!("Azure accounts had no `value` array. Body: {acc}"),
        })?
        .iter()
        .filter_map(|v| v["accountName"].as_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        return Err(ToriiError::Auth {
            provider: "azure".into(),
            message: "Azure PAT is not a member of any organisation. Set \
                 AZURE_DEVOPS_ORG (or AZDO_ORG) to the org name, or clone by URL."
                .to_string(),
        });
    }
    Ok(names)
}

fn azure_list_repos(dev_base: &str, vssps_base: &str, token: &str) -> Result<Vec<RemoteRepo>> {
    let auth = AzureClient::auth_header(token);
    let orgs = if let Some(org) = azure_org_from_env() {
        vec![org]
    } else {
        azure_orgs_from_api(vssps_base, &auth)?
    };
    let mut repos = Vec::new();
    for org in orgs.into_iter().take(10) {
        repos.extend(azure_list_repos_in_org(dev_base, &org, &auth)?);
    }
    Ok(repos)
}

fn azure_visibility_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Azure DevOps repo visibility is controlled at the *project* level, not \
         per-repo. Change it from `https://dev.azure.com/{org}/{project}/_settings/` \
         → Overview → Visibility. (Individual repos can be disabled but not made \
         public independently of their parent project.)"
            .to_string(),
    )
}

impl PlatformClient for AzureClient {
    fn create_repo(
        &self,
        _n: &str,
        _d: Option<&str>,
        _v: Visibility,
        _ns: Option<&str>,
    ) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Azure DevOps create_repo not yet wired".to_string(),
        ))
    }
    fn delete_repo(&self, _o: &str, _r: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Azure DevOps delete_repo not yet wired".to_string(),
        ))
    }
    fn update_repo(&self, _o: &str, _r: &str, _s: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Azure DevOps update_repo not yet wired".to_string(),
        ))
    }
    fn get_repo(&self, _o: &str, _r: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Azure DevOps get_repo not yet wired".to_string(),
        ))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        azure_list_repos(
            "https://dev.azure.com",
            "https://app.vssps.visualstudio.com",
            self.token()?,
        )
    }
    fn set_visibility(&self, _o: &str, _r: &str, _v: Visibility) -> Result<()> {
        Err(azure_visibility_unsupported())
    }
    fn configure_features(&self, _o: &str, _r: &str, _f: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Azure DevOps configure_features not yet wired".to_string(),
        ))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Radicle — peer-to-peer, no central visibility setting. Replication
// is governed by who seeds the project, not by a host-side flag.

pub struct RadicleClient;

impl Default for RadicleClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RadicleClient {
    pub fn new() -> Self {
        Self
    }
}

fn radicle_visibility_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Radicle is peer-to-peer and has no central visibility setting. \
         Reachability is governed by who seeds the project — make a project \
         less discoverable by removing it from seed nodes, not by toggling a flag. \
         See `rad node` and `rad inspect`."
            .to_string(),
    )
}

impl PlatformClient for RadicleClient {
    fn create_repo(
        &self,
        _n: &str,
        _d: Option<&str>,
        _v: Visibility,
        _ns: Option<&str>,
    ) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported(
            "Radicle uses `rad init` to create projects locally, not a REST API.".to_string(),
        ))
    }
    fn delete_repo(&self, _o: &str, _r: &str) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Radicle has no remote-delete — projects exist as long as someone seeds them."
                .to_string(),
        ))
    }
    fn update_repo(&self, _o: &str, _r: &str, _s: RepoSettings) -> Result<RemoteRepo> {
        Err(radicle_visibility_unsupported())
    }
    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo> {
        // The URL parser stores the RID in `owner` and leaves `repo`
        // empty. A caller that passed the RID as `repo` still works.
        let raw = if repo.is_empty() { owner } else { repo };
        let rid = crate::radicle::normalize_rid(raw)?;
        let payload = crate::radicle::run_rad_json(&["inspect", &rid, "--payload"])?;
        Ok(radicle_repo_from_payload(&rid, &payload))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        // `--all` is every replica in storage; without it `rad ls` is
        // only repos this node has contributed to. Fall back if an
        // older `rad` does not know `--all`.
        let json = crate::radicle::run_rad_json(&["ls", "--all"])
            .or_else(|_| crate::radicle::run_rad_json(&["ls"]))?;
        radicle_repos_from_ls(&json)
    }
    fn set_visibility(&self, _o: &str, _r: &str, _v: Visibility) -> Result<()> {
        Err(radicle_visibility_unsupported())
    }
    fn configure_features(&self, _o: &str, _r: &str, _f: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported(
            "Radicle has no per-repo features knob.".to_string(),
        ))
    }
}

fn radicle_project<'a>(json: &'a serde_json::Value) -> &'a serde_json::Value {
    json.pointer("/payload/xyz.radicle.project")
        .or_else(|| json.pointer("/doc/payload/xyz.radicle.project"))
        .or_else(|| json.pointer("/xyz.radicle.project"))
        .unwrap_or(json)
}

fn radicle_rid_from(json: &serde_json::Value) -> String {
    json["rid"]
        .as_str()
        .or_else(|| json["id"].as_str())
        .or_else(|| json["urn"].as_str())
        .unwrap_or("")
        .to_string()
}

fn radicle_visibility_of(json: &serde_json::Value) -> Visibility {
    let vis = json["visibility"]
        .as_str()
        .or_else(|| json["visibility"]["type"].as_str())
        .or_else(|| json["doc"]["visibility"].as_str())
        .or_else(|| json["doc"]["visibility"]["type"].as_str())
        .unwrap_or("public");
    if vis.eq_ignore_ascii_case("private") {
        Visibility::Private
    } else {
        Visibility::Public
    }
}

/// One repository as `rad ls --format json` (or inspect --payload) describes it.
fn radicle_parse_repo(json: &serde_json::Value) -> RemoteRepo {
    let rid_raw = radicle_rid_from(json);
    let rid = crate::radicle::normalize_rid(&rid_raw).unwrap_or_else(|_| rid_raw.clone());
    let project = radicle_project(json);
    let name = project["name"]
        .as_str()
        .or_else(|| json["name"].as_str())
        .unwrap_or_else(|| rid.trim_start_matches("rad:"))
        .to_string();
    let description = project["description"]
        .as_str()
        .or_else(|| json["description"].as_str())
        .and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        });
    let default_branch = project["defaultBranch"]
        .as_str()
        .or_else(|| json["defaultBranch"].as_str())
        .unwrap_or("main")
        .to_string();
    RemoteRepo {
        name,
        description,
        visibility: radicle_visibility_of(json),
        default_branch,
        url: rid.clone(),
        ssh_url: rid.clone(),
        clone_url: rid,
    }
}

fn radicle_repo_from_payload(rid: &str, payload: &serde_json::Value) -> RemoteRepo {
    let mut json = payload.clone();
    if json.get("rid").is_none() {
        json["rid"] = serde_json::Value::String(rid.to_string());
    }
    radicle_parse_repo(&json)
}

fn radicle_repos_from_ls(json: &serde_json::Value) -> Result<Vec<RemoteRepo>> {
    let items: Vec<&serde_json::Value> = if let Some(arr) = json.as_array() {
        arr.iter().collect()
    } else if let Some(arr) = json["results"].as_array() {
        arr.iter().collect()
    } else if json.is_object() {
        vec![json]
    } else {
        return Err(ToriiError::MalformedResponse {
            provider: "radicle".into(),
            message: format!("rad ls: expected array. Body: {json}"),
        });
    };
    Ok(items
        .into_iter()
        .filter(|v| !radicle_rid_from(v).is_empty())
        .map(radicle_parse_repo)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn gitea_repo_json() -> serde_json::Value {
        serde_json::json!({
            "name": "widget",
            "description": "a thing",
            "private": false,
            "default_branch": "main",
            "html_url": "https://gitea.example/acme/widget",
            "ssh_url": "git@gitea.example:acme/widget.git",
            "clone_url": "https://gitea.example/acme/widget.git",
        })
    }

    #[test]
    fn gitea_parse_repo_maps_urls_and_visibility() {
        let repo = gitea_parse_repo(&gitea_repo_json());
        assert_eq!(repo.name, "widget");
        assert_eq!(repo.description.as_deref(), Some("a thing"));
        assert!(matches!(repo.visibility, Visibility::Public));
        assert_eq!(repo.ssh_url, "git@gitea.example:acme/widget.git");
        assert_eq!(repo.clone_url, "https://gitea.example/acme/widget.git");
    }

    #[test]
    fn gitea_create_posts_to_user_repos() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v1/user/repos")
                .header("Authorization", "token tok")
                .json_body(serde_json::json!({
                    "name": "widget",
                    "private": true,
                    "auto_init": false,
                }));
            then.status(201).json_body(gitea_repo_json());
        });
        let token = Some("tok".to_string());
        let repo = gitea_create_repo(
            &server.base_url(),
            &token,
            "Gitea",
            "widget",
            None,
            Visibility::Private,
            None,
        )
        .unwrap();
        m.assert();
        assert_eq!(repo.name, "widget");
    }

    #[test]
    fn gitea_create_in_an_org_hits_orgs_repos() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST).path("/api/v1/orgs/acme/repos");
            then.status(201).json_body(gitea_repo_json());
        });
        let token = Some("tok".to_string());
        gitea_create_repo(
            &server.base_url(),
            &token,
            "Gitea",
            "widget",
            None,
            Visibility::Public,
            Some("acme"),
        )
        .unwrap();
        m.assert();
    }

    #[test]
    fn gitea_delete_and_get_hit_the_repo_path() {
        let server = MockServer::start();
        let del = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1/repos/acme/widget");
            then.status(204);
        });
        let get = server.mock(|when, then| {
            when.method(GET).path("/api/v1/repos/acme/widget");
            then.status(200).json_body(gitea_repo_json());
        });
        let token = Some("tok".to_string());
        gitea_delete_repo(&server.base_url(), &token, "acme", "widget", "Gitea").unwrap();
        let repo = gitea_get_repo(&server.base_url(), &token, "acme", "widget", "Gitea").unwrap();
        del.assert();
        get.assert();
        assert_eq!(repo.name, "widget");
    }

    #[test]
    fn gitea_configure_features_patches_has_issues() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(PATCH)
                .path("/api/v1/repos/acme/widget")
                .json_body(serde_json::json!({ "has_issues": false, "has_wiki": true }));
            then.status(200).json_body(gitea_repo_json());
        });
        let token = Some("tok".to_string());
        let mut features = RepoFeatures::default();
        features.issues = Some(false);
        features.wiki = Some(true);
        gitea_configure_features(
            &server.base_url(),
            &token,
            "acme",
            "widget",
            features,
            "Gitea",
        )
        .unwrap();
        m.assert();
    }

    #[test]
    fn bitbucket_parse_repo_picks_ssh_and_https_clone_links() {
        let json = serde_json::json!({
            "name": "widget",
            "description": "",
            "is_private": true,
            "mainbranch": { "name": "trunk" },
            "links": {
                "html": { "href": "https://bitbucket.org/acme/widget" },
                "clone": [
                    { "name": "https", "href": "https://bitbucket.org/acme/widget.git" },
                    { "name": "ssh", "href": "git@bitbucket.org:acme/widget.git" },
                ]
            }
        });
        let repo = bitbucket_parse_repo(&json);
        assert_eq!(repo.name, "widget");
        assert!(repo.description.is_none());
        assert!(matches!(repo.visibility, Visibility::Private));
        assert_eq!(repo.default_branch, "trunk");
        assert_eq!(repo.ssh_url, "git@bitbucket.org:acme/widget.git");
        assert_eq!(repo.clone_url, "https://bitbucket.org/acme/widget.git");
    }

    #[test]
    fn bitbucket_list_follows_next() {
        let server = MockServer::start();
        let page2 = format!("{}/repositories?page=2", server.base_url());
        let p1 = server.mock(|when, then| {
            when.method(GET)
                .path("/repositories")
                .query_param("role", "member");
            then.status(200).json_body(serde_json::json!({
                "values": [{
                    "name": "one",
                    "is_private": false,
                    "links": {
                        "html": { "href": "https://bitbucket.org/acme/one" },
                        "clone": []
                    }
                }],
                "next": page2,
            }));
        });
        let p2 = server.mock(|when, then| {
            when.method(GET)
                .path("/repositories")
                .query_param("page", "2");
            then.status(200).json_body(serde_json::json!({
                "values": [{
                    "name": "two",
                    "is_private": false,
                    "links": {
                        "html": { "href": "https://bitbucket.org/acme/two" },
                        "clone": []
                    }
                }],
            }));
        });
        let repos = bitbucket_list_repos(&server.base_url(), "Bearer tok").unwrap();
        p1.assert();
        p2.assert();
        assert_eq!(
            repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["one", "two"]
        );
    }

    #[test]
    fn sourcehut_parse_repo_builds_clone_urls() {
        let json = serde_json::json!({
            "name": "widget",
            "description": "notes",
            "visibility": "unlisted",
            "owner": { "canonical_name": "~acme" },
        });
        let repo = sourcehut_parse_repo(&json, "~fallback");
        assert_eq!(repo.name, "widget");
        assert!(matches!(repo.visibility, Visibility::Internal));
        assert_eq!(repo.url, "https://git.sr.ht/~acme/widget");
        assert_eq!(repo.ssh_url, "git@git.sr.ht:~acme/widget");
        assert_eq!(repo.clone_url, "https://git.sr.ht/~acme/widget");
    }

    #[test]
    fn sourcehut_list_uses_whoami_when_owner_is_missing() {
        let git = MockServer::start();
        let meta = MockServer::start();
        let who = meta.mock(|when, then| {
            when.method(GET).path("/api/user");
            then.status(200)
                .json_body(serde_json::json!({ "canonical_name": "~acme" }));
        });
        let list = git.mock(|when, then| {
            when.method(GET).path("/api/repos");
            then.status(200).json_body(serde_json::json!({
                "results": [{ "name": "widget", "visibility": "public" }],
                "next": null,
            }));
        });
        let repos = sourcehut_list_repos(&git.base_url(), &meta.base_url(), "tok").unwrap();
        who.assert();
        list.assert();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].ssh_url, "git@git.sr.ht:~acme/widget");
    }

    #[test]
    fn azure_parse_repo_strips_git_from_the_web_url() {
        let json = serde_json::json!({
            "name": "widget",
            "webUrl": "https://dev.azure.com/org/proj/_git/widget",
            "sshUrl": "git@ssh.dev.azure.com:v3/org/proj/widget",
            "remoteUrl": "https://dev.azure.com/org/proj/_git/widget",
            "defaultBranch": "refs/heads/develop",
            "project": { "name": "proj", "visibility": "private" },
        });
        let repo = azure_parse_repo(&json);
        assert_eq!(repo.name, "widget");
        assert_eq!(repo.url, "https://dev.azure.com/org/proj/widget");
        assert_eq!(repo.default_branch, "develop");
        assert_eq!(repo.ssh_url, "git@ssh.dev.azure.com:v3/org/proj/widget");
    }

    #[test]
    fn azure_list_in_org_reads_the_value_array() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/acme/_apis/git/repositories")
                .query_param("api-version", "7.0");
            then.status(200).json_body(serde_json::json!({
                "value": [{
                    "name": "widget",
                    "webUrl": "https://dev.azure.com/acme/p/_git/widget",
                    "sshUrl": "git@ssh.dev.azure.com:v3/acme/p/widget",
                    "remoteUrl": "https://dev.azure.com/acme/p/_git/widget",
                    "project": { "name": "p", "visibility": "public" },
                }]
            }));
        });
        let repos = azure_list_repos_in_org(&server.base_url(), "acme", "Basic x").unwrap();
        m.assert();
        assert_eq!(repos.len(), 1);
        assert!(matches!(repos[0].visibility, Visibility::Public));
        assert_eq!(repos[0].url, "https://dev.azure.com/acme/p/widget");
    }

    #[test]
    fn azure_orgs_from_api_reads_account_names() {
        let server = MockServer::start();
        let me = server.mock(|when, then| {
            when.method(GET).path("/_apis/profile/profiles/me");
            then.status(200)
                .json_body(serde_json::json!({ "id": "guid-1" }));
        });
        let acc = server.mock(|when, then| {
            when.method(GET)
                .path("/_apis/accounts")
                .query_param("memberId", "guid-1");
            then.status(200).json_body(serde_json::json!({
                "value": [{ "accountName": "acme" }, { "accountName": "other" }]
            }));
        });
        let orgs = azure_orgs_from_api(&server.base_url(), "Basic x").unwrap();
        me.assert();
        acc.assert();
        assert_eq!(orgs, ["acme", "other"]);
    }

    #[test]
    fn radicle_parse_repo_reads_the_project_payload() {
        let json = serde_json::json!({
            "rid": "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5",
            "payload": {
                "xyz.radicle.project": {
                    "name": "heartwood",
                    "description": "the stack",
                    "defaultBranch": "master"
                }
            },
            "visibility": "public"
        });
        let repo = radicle_parse_repo(&json);
        assert_eq!(repo.name, "heartwood");
        assert_eq!(repo.description.as_deref(), Some("the stack"));
        assert_eq!(repo.default_branch, "master");
        assert!(matches!(repo.visibility, Visibility::Public));
        assert_eq!(repo.clone_url, "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5");
        assert_eq!(repo.ssh_url, repo.clone_url);
    }

    #[test]
    fn radicle_parse_repo_nested_doc_and_private() {
        let json = serde_json::json!({
            "id": "z42hL2jL4XNk6K8oHQaSWfMgCL7ji",
            "doc": {
                "payload": {
                    "xyz.radicle.project": { "name": "heartwood" }
                },
                "visibility": { "type": "private" }
            }
        });
        let repo = radicle_parse_repo(&json);
        assert_eq!(repo.name, "heartwood");
        assert_eq!(repo.clone_url, "rad:z42hL2jL4XNk6K8oHQaSWfMgCL7ji");
        assert!(matches!(repo.visibility, Visibility::Private));
    }

    #[test]
    fn radicle_ls_array_skips_rows_without_a_rid() {
        let json = serde_json::json!([
            { "rid": "rad:z111", "payload": { "xyz.radicle.project": { "name": "one" } } },
            { "noise": true },
            { "rid": "rad:z222", "name": "two" }
        ]);
        let repos = radicle_repos_from_ls(&json).unwrap();
        assert_eq!(
            repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["one", "two"]
        );
    }

    #[test]
    fn radicle_repo_from_inspect_payload() {
        let payload = serde_json::json!({
            "xyz.radicle.project": {
                "name": "heartwood",
                "description": "the stack",
                "defaultBranch": "master"
            }
        });
        let repo = radicle_repo_from_payload("rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5", &payload);
        assert_eq!(repo.name, "heartwood");
        assert_eq!(repo.clone_url, "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5");
    }
}
