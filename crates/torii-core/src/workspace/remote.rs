use serde::{Deserialize, Serialize};
use crate::error::{Result, ToriiError};

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
    fn create_repo(&self, name: &str, description: Option<&str>, visibility: Visibility, namespace: Option<&str>) -> Result<RemoteRepo>;
    
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
        crate::auth::resolve_token("github", ".").value
            .ok_or_else(|| ToriiError::Auth { provider: "github".into(), message: "GitHub token not found. Run: torii auth set github YOUR_TOKEN".to_string() })
    }
}

impl PlatformClient for GitHubClient {
    fn create_repo(&self, name: &str, description: Option<&str>, visibility: Visibility, namespace: Option<&str>) -> Result<RemoteRepo> {
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
            .map_err(|e| ToriiError::Network { provider: "github".into(), message: e.to_string() })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let msg = resp.text().unwrap_or_default();
            return Err(ToriiError::PlatformApi { provider: "github".into(), status, message: msg });
        }

        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::MalformedResponse { provider: "github".into(), message: format!("Failed to parse GitHub response: {}", e) })?;

        let repo_name = json["name"].as_str().unwrap_or(name).to_string();
        let owner = json["owner"]["login"].as_str().unwrap_or("unknown").to_string();

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
            .map_err(|e| ToriiError::Network { provider: "github".into(), message: e.to_string() })?;

        match resp.status().as_u16() {
            204 => {
                println!("✅ Repository deleted from GitHub");
                Ok(())
            }
            403 => Err(ToriiError::Auth { provider: "github".into(), message: format!(
                "GitHub refused the delete (HTTP 403). Token needs the `delete_repo` scope; \
                 add it at https://github.com/settings/tokens or use a fine-grained token \
                 with `Administration: write` on `{}/{}`.", owner, repo
            ) }),
            404 => Err(ToriiError::Auth { provider: "github".into(), message: format!(
                "GitHub returned 404 for `{}/{}` — repo doesn't exist or token can't see it.",
                owner, repo
            ) }),
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
        let repo_name = format!("{}/{}", owner, repo);
        let mut args = vec!["repo", "edit", &repo_name];
        
        let mut temp_args = Vec::new();
        
        if let Some(desc) = &settings.description {
            temp_args.push("--description".to_string());
            temp_args.push(desc.clone());
        }
        
        if let Some(homepage) = &settings.homepage {
            temp_args.push("--homepage".to_string());
            temp_args.push(homepage.clone());
        }
        
        if let Some(vis) = &settings.visibility {
            match vis {
                Visibility::Public => temp_args.push("--visibility=public".to_string()),
                Visibility::Private => temp_args.push("--visibility=private".to_string()),
                Visibility::Internal => temp_args.push("--visibility=private".to_string()),
            }
        }
        
        if let Some(branch) = &settings.default_branch {
            temp_args.push("--default-branch".to_string());
            temp_args.push(branch.clone());
        }
        
        // Convert temp_args to string slices
        let arg_refs: Vec<&str> = temp_args.iter().map(|s| s.as_str()).collect();
        args.extend(arg_refs);
        
        let output = std::process::Command::new("gh")
            .args(&args)
            .output();
        
        match output {
            Ok(out) if out.status.success() => {
                println!("✅ Repository settings updated");
                self.get_repo(owner, repo)
            }
            _ => {
                Err(ToriiError::Subprocess {
                    tool: "gh".into(),
                    message: "Failed to update repository settings".to_string(),
                })
            }
        }
    }
    
    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo> {
        let repo_name = format!("{}/{}", owner, repo);
        let output = std::process::Command::new("gh")
            .args(&["repo", "view", &repo_name, "--json", "name,description,visibility,defaultBranchRef,url,sshUrl"])
            .output();
        
        match output {
            Ok(out) if out.status.success() => {
                // Parse JSON output (simplified)
                Ok(RemoteRepo {
                    name: repo.to_string(),
                    description: None,
                    visibility: Visibility::Private,
                    default_branch: "main".to_string(),
                    url: format!("https://github.com/{}/{}", owner, repo),
                    ssh_url: format!("git@github.com:{}/{}.git", owner, repo),
                    clone_url: format!("https://github.com/{}/{}.git", owner, repo),
                })
            }
            _ => {
                Err(ToriiError::Subprocess {
                    tool: "gh".into(),
                    message: "Failed to get repository information".to_string(),
                })
            }
        }
    }
    
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        let output = std::process::Command::new("gh")
            .args(&["repo", "list", "--json", "name,description,visibility", "--limit", "100"])
            .output();
        
        match output {
            Ok(out) if out.status.success() => {
                // Return empty list for now (would parse JSON in full implementation)
                Ok(Vec::new())
            }
            _ => {
                Err(ToriiError::Subprocess {
                    tool: "gh".into(),
                    message: "Failed to list repositories".to_string(),
                })
            }
        }
    }
    
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        let mut settings = RepoSettings::default();
        settings.visibility = Some(visibility);
        self.update_repo(owner, repo, settings)?;
        Ok(())
    }
    
    fn configure_features(&self, owner: &str, repo: &str, features: RepoFeatures) -> Result<()> {
        let repo_name = format!("{}/{}", owner, repo);
        let mut args = vec!["repo", "edit", &repo_name];
        
        let mut temp_args = Vec::new();
        
        if let Some(issues) = features.issues {
            temp_args.push(if issues { "--enable-issues".to_string() } else { "--disable-issues".to_string() });
        }
        
        if let Some(wiki) = features.wiki {
            temp_args.push(if wiki { "--enable-wiki".to_string() } else { "--disable-wiki".to_string() });
        }
        
        if let Some(projects) = features.projects {
            temp_args.push(if projects { "--enable-projects".to_string() } else { "--disable-projects".to_string() });
        }
        
        let arg_refs: Vec<&str> = temp_args.iter().map(|s| s.as_str()).collect();
        args.extend(arg_refs);
        
        let output = std::process::Command::new("gh")
            .args(&args)
            .output();
        
        match output {
            Ok(out) if out.status.success() => {
                println!("✅ Repository features configured");
                Ok(())
            }
            _ => {
                Err(ToriiError::Subprocess {
                    tool: "gh".into(),
                    message: "Failed to configure repository features".to_string(),
                })
            }
        }
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
    fn create_repo(&self, name: &str, description: Option<&str>, visibility: Visibility, namespace: Option<&str>) -> Result<RemoteRepo> {
        let token = self.token.as_ref()
            .ok_or_else(|| ToriiError::Auth { provider: "gitlab".into(), message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string() })?;

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
                .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("group lookup failed: {}", e) })?;

            let ns_id = if group_resp.status().is_success() {
                let group: serde_json::Value = group_resp.json()
                    .map_err(|e| ToriiError::MalformedResponse { provider: "gitlab".into(), message: format!("GitLab group parse: {}", e) })?;
                group["id"].as_i64().ok_or_else(|| ToriiError::MalformedResponse { provider: "gitlab".into(), message: format!("GitLab group `{}` returned no id", ns) })?
            } else if group_resp.status().as_u16() == 404 {
                // Try as a user. /users?username=… returns an array.
                let user_url = format!("{}/users?username={}", self.base_url, ns_encoded);
                let user_resp = client
                    .get(&user_url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send()
                    .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("user lookup failed: {}", e) })?;
                if !user_resp.status().is_success() {
                    return Err(ToriiError::MalformedResponse {
                        provider: "gitlab".into(),
                        message: format!("namespace `{}` is neither a group nor a user", ns),
                    });
                }
                let users: serde_json::Value = user_resp.json()
                    .map_err(|e| ToriiError::MalformedResponse { provider: "gitlab".into(), message: format!("GitLab user parse: {}", e) })?;
                let user = users.as_array()
                    .and_then(|a| a.first())
                    .ok_or_else(|| ToriiError::Usage(
                        format!("GitLab namespace `{}` not found", ns)
                    ))?;
                user["namespace_id"].as_i64()
                    .or_else(|| user["id"].as_i64())
                    .ok_or_else(|| ToriiError::MalformedResponse { provider: "gitlab".into(), message: format!("GitLab user `{}` returned no namespace_id", ns) })?
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
            .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("GitLab API request failed: {}", e) })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        let project: serde_json::Value = response.json()
            .map_err(|e| ToriiError::MalformedResponse { provider: "gitlab".into(), message: format!("Failed to parse GitLab response: {}", e) })?;

        Ok(RemoteRepo {
            name: project["name"].as_str().unwrap_or(name).to_string(),
            description: project["description"].as_str().map(|s| s.to_string()),
            visibility,
            default_branch: project["default_branch"].as_str().unwrap_or("main").to_string(),
            url: project["web_url"].as_str().unwrap_or("").to_string(),
            ssh_url: project["ssh_url_to_repo"].as_str().unwrap_or("").to_string(),
            clone_url: project["http_url_to_repo"].as_str().unwrap_or("").to_string(),
        })
    }
    
    fn delete_repo(&self, owner: &str, repo: &str) -> Result<()> {
        let token = self.token.as_ref()
            .ok_or_else(|| ToriiError::Auth { provider: "gitlab".into(), message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string() })?;

        let path_str = format!("{}/{}", owner, repo);
        let project_path = crate::url::encode(&path_str);
        let client = reqwest::blocking::Client::new();
        let response = client
            .delete(format!("{}/projects/{}", self.base_url, project_path))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("GitLab API request failed: {}", e) })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        Ok(())
    }
    
    fn update_repo(&self, owner: &str, repo: &str, settings: RepoSettings) -> Result<RemoteRepo> {
        let token = self.token.as_ref()
            .ok_or_else(|| ToriiError::Auth { provider: "gitlab".into(), message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string() })?;

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
            .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("GitLab API request failed: {}", e) })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        let project: serde_json::Value = response.json()
            .map_err(|e| ToriiError::MalformedResponse { provider: "gitlab".into(), message: format!("Failed to parse GitLab response: {}", e) })?;

        let visibility = match project["visibility"].as_str() {
            Some("public") => Visibility::Public,
            Some("internal") => Visibility::Internal,
            _ => Visibility::Private,
        };

        Ok(RemoteRepo {
            name: project["name"].as_str().unwrap_or(repo).to_string(),
            description: project["description"].as_str().map(|s| s.to_string()),
            visibility,
            default_branch: project["default_branch"].as_str().unwrap_or("main").to_string(),
            url: project["web_url"].as_str().unwrap_or("").to_string(),
            ssh_url: project["ssh_url_to_repo"].as_str().unwrap_or("").to_string(),
            clone_url: project["http_url_to_repo"].as_str().unwrap_or("").to_string(),
        })
    }
    
    fn get_repo(&self, owner: &str, repo: &str) -> Result<RemoteRepo> {
        let token = self.token.as_ref()
            .ok_or_else(|| ToriiError::Auth { provider: "gitlab".into(), message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string() })?;

        let path_str = format!("{}/{}", owner, repo);
        let project_path = crate::url::encode(&path_str);
        let client = reqwest::blocking::Client::new();
        let response = client
            .get(format!("{}/projects/{}", self.base_url, project_path))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("GitLab API request failed: {}", e) })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        let project: serde_json::Value = response.json()
            .map_err(|e| ToriiError::MalformedResponse { provider: "gitlab".into(), message: format!("Failed to parse GitLab response: {}", e) })?;

        let visibility = match project["visibility"].as_str() {
            Some("public") => Visibility::Public,
            Some("internal") => Visibility::Internal,
            _ => Visibility::Private,
        };

        Ok(RemoteRepo {
            name: project["name"].as_str().unwrap_or(repo).to_string(),
            description: project["description"].as_str().map(|s| s.to_string()),
            visibility,
            default_branch: project["default_branch"].as_str().unwrap_or("main").to_string(),
            url: project["web_url"].as_str().unwrap_or("").to_string(),
            ssh_url: project["ssh_url_to_repo"].as_str().unwrap_or("").to_string(),
            clone_url: project["http_url_to_repo"].as_str().unwrap_or("").to_string(),
        })
    }
    
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        let token = self.token.as_ref()
            .ok_or_else(|| ToriiError::Auth { provider: "gitlab".into(), message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string() })?;

        let client = reqwest::blocking::Client::new();
        let response = client
            .get(format!("{}/projects?membership=true&per_page=100", self.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("GitLab API request failed: {}", e) })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        let projects: Vec<serde_json::Value> = response.json()
            .map_err(|e| ToriiError::MalformedResponse { provider: "gitlab".into(), message: format!("Failed to parse GitLab response: {}", e) })?;

        Ok(projects.iter().map(|project| {
            let visibility = match project["visibility"].as_str() {
                Some("public") => Visibility::Public,
                Some("internal") => Visibility::Internal,
                _ => Visibility::Private,
            };

            RemoteRepo {
                name: project["name"].as_str().unwrap_or("").to_string(),
                description: project["description"].as_str().map(|s| s.to_string()),
                visibility,
                default_branch: project["default_branch"].as_str().unwrap_or("main").to_string(),
                url: project["web_url"].as_str().unwrap_or("").to_string(),
                ssh_url: project["ssh_url_to_repo"].as_str().unwrap_or("").to_string(),
                clone_url: project["http_url_to_repo"].as_str().unwrap_or("").to_string(),
            }
        }).collect())
    }
    
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        let token = self.token.as_ref()
            .ok_or_else(|| ToriiError::Auth { provider: "gitlab".into(), message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string() })?;

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
            .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("GitLab API request failed: {}", e) })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ToriiError::PlatformApi {
                provider: "gitlab".into(),
                status,
                message: error_text,
            });
        }

        Ok(())
    }
    
    fn configure_features(&self, owner: &str, repo: &str, features: RepoFeatures) -> Result<()> {
        let token = self.token.as_ref()
            .ok_or_else(|| ToriiError::Auth { provider: "gitlab".into(), message: "GitLab token not found. Set GITLAB_TOKEN environment variable".to_string() })?;

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
            .map_err(|e| ToriiError::Network { provider: "gitlab".into(), message: format!("GitLab API request failed: {}", e) })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error_text = response.text().unwrap_or_else(|_| "Unknown error".to_string());
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
            Ok(Box::new(AzureClient::new()))
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
}

impl CodebergClient {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }
}

// Placeholder implementations - will be completed with API calls
// ──────────────────────────────────────────────────────────────────────────────
// Gitea / Forgejo / Codeberg — all three share the Gitea API.
//
// We implement the shared bits as free functions and delegate from each
// client. Only the base URL differs (gitea.com / codeberg.org / a
// self-hosted Forgejo instance via FORGEJO_URL / GITEA_URL env vars).
//
// For 0.7.19 we wire `set_visibility` end-to-end; create / delete /
// update / list still return clear "not implemented yet" errors —
// follow-up work tracked in ROADMAP.

fn gitea_token<'a>(label: &str, token: &'a Option<String>) -> Result<&'a String> {
    token.as_ref().ok_or_else(|| ToriiError::Auth {
        provider: label.to_lowercase(),
        message: format!("{label} token not found. Run: torii auth set {} YOUR_TOKEN", label.to_lowercase()),
    })
}

fn gitea_set_visibility(base_url: &str, token: &Option<String>, owner: &str, repo: &str, visibility: Visibility, label: &str) -> Result<()> {
    // Gitea visibility is just a `private` boolean. "Internal" doesn't
    // exist on Gitea — collapse to private.
    let private = !matches!(visibility, Visibility::Public);
    let tok = gitea_token(label, token)?;
    let url = format!("{}/api/v1/repos/{}/{}", base_url.trim_end_matches('/'), owner, repo);
    let body = serde_json::json!({ "private": private });
    let req = crate::http::make_client().patch(&url)
        .header("Authorization", format!("token {}", tok))
        .header("Accept", "application/json")
        .json(&body);
    crate::http::send_empty(req, &format!("{} set visibility", label))
}

impl PlatformClient for GiteaClient {
    fn create_repo(&self, _name: &str, _description: Option<&str>, _visibility: Visibility, _namespace: Option<&str>) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Gitea create_repo not yet wired (use the web UI). `torii remote visibility` does work.".to_string()))
    }
    fn delete_repo(&self, _owner: &str, _repo: &str) -> Result<()> {
        Err(ToriiError::Unsupported("Gitea delete_repo not yet wired".to_string()))
    }
    fn update_repo(&self, _owner: &str, _repo: &str, _settings: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Gitea update_repo not yet wired".to_string()))
    }
    fn get_repo(&self, _owner: &str, _repo: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Gitea get_repo not yet wired".to_string()))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        Err(ToriiError::Unsupported("Gitea list_repos not yet wired".to_string()))
    }
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        gitea_set_visibility(&self.base_url, &self.token, owner, repo, visibility, "Gitea")
    }
    fn configure_features(&self, _owner: &str, _repo: &str, _features: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported("Gitea configure_features not yet wired".to_string()))
    }
}

impl PlatformClient for ForgejoClient {
    fn create_repo(&self, _name: &str, _description: Option<&str>, _visibility: Visibility, _namespace: Option<&str>) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Forgejo create_repo not yet wired (use the web UI). `torii remote visibility` does work.".to_string()))
    }
    fn delete_repo(&self, _owner: &str, _repo: &str) -> Result<()> {
        Err(ToriiError::Unsupported("Forgejo delete_repo not yet wired".to_string()))
    }
    fn update_repo(&self, _owner: &str, _repo: &str, _settings: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Forgejo update_repo not yet wired".to_string()))
    }
    fn get_repo(&self, _owner: &str, _repo: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Forgejo get_repo not yet wired".to_string()))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        Err(ToriiError::Unsupported("Forgejo list_repos not yet wired".to_string()))
    }
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        gitea_set_visibility(&self.base_url, &self.token, owner, repo, visibility, "Forgejo")
    }
    fn configure_features(&self, _owner: &str, _repo: &str, _features: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported("Forgejo configure_features not yet wired".to_string()))
    }
}

impl PlatformClient for CodebergClient {
    fn create_repo(&self, _name: &str, _description: Option<&str>, _visibility: Visibility, _namespace: Option<&str>) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Codeberg create_repo not yet wired (use the web UI). `torii remote visibility` does work.".to_string()))
    }
    fn delete_repo(&self, _owner: &str, _repo: &str) -> Result<()> {
        Err(ToriiError::Unsupported("Codeberg delete_repo not yet wired".to_string()))
    }
    fn update_repo(&self, _owner: &str, _repo: &str, _settings: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Codeberg update_repo not yet wired".to_string()))
    }
    fn get_repo(&self, _owner: &str, _repo: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Codeberg get_repo not yet wired".to_string()))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        Err(ToriiError::Unsupported("Codeberg list_repos not yet wired".to_string()))
    }
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        // Codeberg is just a Forgejo instance pinned to codeberg.org.
        gitea_set_visibility("https://codeberg.org", &self.token, owner, repo, visibility, "Codeberg")
    }
    fn configure_features(&self, _owner: &str, _repo: &str, _features: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported("Codeberg configure_features not yet wired".to_string()))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Bitbucket Cloud — `PUT /2.0/repositories/{ws}/{repo}` with `is_private`.

#[allow(dead_code)]
pub struct BitbucketClient { token: Option<String> }

impl BitbucketClient {
    pub fn new(token: Option<String>) -> Self { Self { token } }

    fn auth(&self) -> Result<String> {
        let tok = self.token.as_ref().ok_or_else(|| ToriiError::Auth { provider: "bitbucket".into(), message: "Bitbucket token not found. Run: torii auth set bitbucket USERNAME:APP_PASSWORD".to_string() })?;
        if tok.contains(':') {
            use base64::Engine;
            Ok(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(tok)))
        } else {
            Ok(format!("Bearer {}", tok))
        }
    }
}

impl PlatformClient for BitbucketClient {
    fn create_repo(&self, _n: &str, _d: Option<&str>, _v: Visibility, _ns: Option<&str>) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Bitbucket create_repo not yet wired".to_string()))
    }
    fn delete_repo(&self, _o: &str, _r: &str) -> Result<()> {
        Err(ToriiError::Unsupported("Bitbucket delete_repo not yet wired".to_string()))
    }
    fn update_repo(&self, _o: &str, _r: &str, _s: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Bitbucket update_repo not yet wired".to_string()))
    }
    fn get_repo(&self, _o: &str, _r: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Bitbucket get_repo not yet wired".to_string()))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        Err(ToriiError::Unsupported("Bitbucket list_repos not yet wired".to_string()))
    }
    fn set_visibility(&self, owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        // Bitbucket: PUT /2.0/repositories/{ws}/{repo} with `is_private`.
        // "Internal" doesn't exist — collapse to private.
        let is_private = !matches!(visibility, Visibility::Public);
        let url = format!("https://api.bitbucket.org/2.0/repositories/{}/{}", owner, repo);
        let body = serde_json::json!({ "is_private": is_private });
        let req = crate::http::make_client().put(&url)
            .header("Authorization", self.auth()?)
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Bitbucket set visibility")
    }
    fn configure_features(&self, _o: &str, _r: &str, _f: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported("Bitbucket configure_features not yet wired".to_string()))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Sourcehut — `meta.sr.ht` GraphQL endpoint for visibility updates.

#[allow(dead_code)]
pub struct SourcehutClient { token: Option<String> }

impl SourcehutClient {
    pub fn new(token: Option<String>) -> Self { Self { token } }
}

impl PlatformClient for SourcehutClient {
    fn create_repo(&self, _n: &str, _d: Option<&str>, _v: Visibility, _ns: Option<&str>) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Sourcehut create_repo not yet wired".to_string()))
    }
    fn delete_repo(&self, _o: &str, _r: &str) -> Result<()> {
        Err(ToriiError::Unsupported("Sourcehut delete_repo not yet wired".to_string()))
    }
    fn update_repo(&self, _o: &str, _r: &str, _s: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Sourcehut update_repo not yet wired".to_string()))
    }
    fn get_repo(&self, _o: &str, _r: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Sourcehut get_repo not yet wired".to_string()))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        Err(ToriiError::Unsupported("Sourcehut list_repos not yet wired".to_string()))
    }
    fn set_visibility(&self, _owner: &str, repo: &str, visibility: Visibility) -> Result<()> {
        // git.sr.ht exposes visibility via a GraphQL mutation. Three
        // values: PUBLIC, UNLISTED, PRIVATE. We collapse torii's
        // (Public, Private, Internal) → (PUBLIC, PRIVATE, UNLISTED).
        let tok = self.token.as_ref().ok_or_else(|| ToriiError::Auth { provider: "sourcehut".into(), message: "Sourcehut token not found. Run: torii auth set sourcehut YOUR_TOKEN".to_string() })?;
        let target = match visibility {
            Visibility::Public   => "PUBLIC",
            Visibility::Private  => "PRIVATE",
            Visibility::Internal => "UNLISTED",
        };
        // git.sr.ht GraphQL is at https://git.sr.ht/query
        let query = serde_json::json!({
            "query": "mutation Update($name: String!, $visibility: Visibility!) { \
                       updateRepository(name: $name, input: { visibility: $visibility }) { id } }",
            "variables": { "name": repo, "visibility": target }
        });
        let req = crate::http::make_client().post("https://git.sr.ht/query")
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
                    message: format!("GraphQL errors: {}", serde_json::to_string(errs).unwrap_or_default()),
                });
            }
        }
        Ok(())
    }
    fn configure_features(&self, _o: &str, _r: &str, _f: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported("Sourcehut configure_features not yet wired".to_string()))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Azure DevOps — visibility lives at the *project* level on Azure, not
// per-repo. We surface that clearly so the user knows where to go.

pub struct AzureClient;

impl AzureClient { pub fn new() -> Self { Self } }

fn azure_visibility_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Azure DevOps repo visibility is controlled at the *project* level, not \
         per-repo. Change it from `https://dev.azure.com/{org}/{project}/_settings/` \
         → Overview → Visibility. (Individual repos can be disabled but not made \
         public independently of their parent project.)".to_string()
    )
}

impl PlatformClient for AzureClient {
    fn create_repo(&self, _n: &str, _d: Option<&str>, _v: Visibility, _ns: Option<&str>) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Azure DevOps create_repo not yet wired".to_string()))
    }
    fn delete_repo(&self, _o: &str, _r: &str) -> Result<()> {
        Err(ToriiError::Unsupported("Azure DevOps delete_repo not yet wired".to_string()))
    }
    fn update_repo(&self, _o: &str, _r: &str, _s: RepoSettings) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Azure DevOps update_repo not yet wired".to_string()))
    }
    fn get_repo(&self, _o: &str, _r: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Azure DevOps get_repo not yet wired".to_string()))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        Err(ToriiError::Unsupported("Azure DevOps list_repos not yet wired".to_string()))
    }
    fn set_visibility(&self, _o: &str, _r: &str, _v: Visibility) -> Result<()> { Err(azure_visibility_unsupported()) }
    fn configure_features(&self, _o: &str, _r: &str, _f: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported("Azure DevOps configure_features not yet wired".to_string()))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Radicle — peer-to-peer, no central visibility setting. Replication
// is governed by who seeds the project, not by a host-side flag.

pub struct RadicleClient;

impl RadicleClient { pub fn new() -> Self { Self } }

fn radicle_visibility_unsupported() -> ToriiError {
    ToriiError::Unsupported(
        "Radicle is peer-to-peer and has no central visibility setting. \
         Reachability is governed by who seeds the project — make a project \
         less discoverable by removing it from seed nodes, not by toggling a flag. \
         See `rad node` and `rad inspect`.".to_string()
    )
}

impl PlatformClient for RadicleClient {
    fn create_repo(&self, _n: &str, _d: Option<&str>, _v: Visibility, _ns: Option<&str>) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Radicle uses `rad init` to create projects locally, not a REST API.".to_string()))
    }
    fn delete_repo(&self, _o: &str, _r: &str) -> Result<()> {
        Err(ToriiError::Unsupported("Radicle has no remote-delete — projects exist as long as someone seeds them.".to_string()))
    }
    fn update_repo(&self, _o: &str, _r: &str, _s: RepoSettings) -> Result<RemoteRepo> { Err(radicle_visibility_unsupported()) }
    fn get_repo(&self, _o: &str, _r: &str) -> Result<RemoteRepo> {
        Err(ToriiError::Unsupported("Radicle get_repo not yet wired — use `rad inspect <RID>` directly.".to_string()))
    }
    fn list_repos(&self) -> Result<Vec<RemoteRepo>> {
        Err(ToriiError::Unsupported("Radicle list_repos not yet wired — use `rad ls` directly.".to_string()))
    }
    fn set_visibility(&self, _o: &str, _r: &str, _v: Visibility) -> Result<()> { Err(radicle_visibility_unsupported()) }
    fn configure_features(&self, _o: &str, _r: &str, _f: RepoFeatures) -> Result<()> {
        Err(ToriiError::Unsupported("Radicle has no per-repo features knob.".to_string()))
    }
}
