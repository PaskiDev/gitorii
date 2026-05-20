use serde::{Deserialize, Serialize};
use reqwest::blocking::Client;
use crate::error::{Result, ToriiError};

// ============================================================================
// Shared types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub head: String,
    pub base: String,
    pub author: String,
    pub url: String,
    pub draft: bool,
    pub mergeable: Option<bool>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct CreatePrOptions {
    pub title: String,
    pub body: Option<String>,
    pub head: String,
    pub base: String,
    pub draft: bool,
}

#[derive(Debug, Clone)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl std::fmt::Display for MergeMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeMethod::Merge  => write!(f, "merge"),
            MergeMethod::Squash => write!(f, "squash"),
            MergeMethod::Rebase => write!(f, "rebase"),
        }
    }
}

// ============================================================================
// Trait
// ============================================================================

pub struct UpdatePrOptions {
    pub title: Option<String>,
    pub body: Option<String>,
    pub base: Option<String>,
}

#[allow(dead_code)]
pub trait PrClient: Send {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest>;
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>>;
    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest>;
    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()>;
    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()>;
    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()>;
    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()>;
    fn checkout_branch(&self, pr: &PullRequest) -> String;
}

// ============================================================================
// GitHub
// ============================================================================

pub struct GitHubPrClient {
    token: String,
}

impl GitHubPrClient {
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

    fn auth(&self) -> String {
        format!("token {}", self.token)
    }
}

impl PrClient for GitHubPrClient {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls", owner, repo);
        let body = serde_json::json!({
            "title": opts.title,
            "body":  opts.body.unwrap_or_default(),
            "head":  opts.head,
            "base":  opts.base,
            "draft": opts.draft,
        });
        let resp = self.client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str().unwrap_or("unknown error");
            return Err(ToriiError::InvalidConfig(format!("GitHub API {}: {}", status, msg)));
        }
        parse_github_pr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/pulls?state={}&per_page=50",
            owner, repo, state
        );
        let resp = self.client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str().unwrap_or("(no message in response)");
            return Err(ToriiError::InvalidConfig(format!(
                "GitHub API {}: {} (url: {})", status, msg, url
            )));
        }
        let arr = json.as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitHub returned a non-array body for {}. Body: {}",
                url, json
            )))?;
        arr.iter().map(parse_github_pr).collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls/{}", owner, repo, number);
        let resp = self.client()
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API parse error: {}", e)))?;
        parse_github_pr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls/{}/merge", owner, repo, number);
        let body = serde_json::json!({ "merge_method": method.to_string() });
        let resp = self.client()
            .put(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("merge failed");
            return Err(ToriiError::InvalidConfig(format!("Merge failed: {}", msg)));
        }
        Ok(())
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls/{}", owner, repo, number);
        let body = serde_json::json!({ "state": "closed" });
        let resp = self.client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("close failed");
            return Err(ToriiError::InvalidConfig(format!("Close failed: {}", msg)));
        }
        Ok(())
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls/{}", owner, repo, number);
        let mut body = serde_json::Map::new();
        if let Some(t) = opts.title { body.insert("title".into(), t.into()); }
        if let Some(b) = opts.body  { body.insert("body".into(), b.into()); }
        if let Some(b) = opts.base  { body.insert("base".into(), b.into()); }
        let resp = self.client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&serde_json::Value::Object(body))
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("update failed");
            return Err(ToriiError::InvalidConfig(format!("Update failed: {}", msg)));
        }
        Ok(())
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/git/refs/heads/{}", owner, repo, branch);
        let resp = self.client()
            .delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("delete branch failed");
            return Err(ToriiError::InvalidConfig(format!("Delete branch failed: {}", msg)));
        }
        Ok(())
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_github_pr(json: &serde_json::Value) -> Result<PullRequest> {
    Ok(PullRequest {
        number:     json["number"].as_u64().unwrap_or(0),
        title:      json["title"].as_str().unwrap_or("").to_string(),
        body:       json["body"].as_str().map(|s| s.to_string()),
        state:      json["state"].as_str().unwrap_or("").to_string(),
        head:       json["head"]["ref"].as_str().unwrap_or("").to_string(),
        base:       json["base"]["ref"].as_str().unwrap_or("").to_string(),
        author:     json["user"]["login"].as_str().unwrap_or("").to_string(),
        url:        json["html_url"].as_str().unwrap_or("").to_string(),
        draft:      json["draft"].as_bool().unwrap_or(false),
        mergeable:  json["mergeable"].as_bool(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// GitLab (Merge Requests)
// ============================================================================

pub struct GitLabPrClient {
    token: String,
    base_url: String,
}

impl GitLabPrClient {
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

impl PrClient for GitLabPrClient {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        let url = format!(
            "{}/projects/{}/merge_requests",
            self.base_url, Self::project_path(owner, repo)
        );
        let body = serde_json::json!({
            "title":         opts.title,
            "description":   opts.body.unwrap_or_default(),
            "source_branch": opts.head,
            "target_branch": opts.base,
            "draft":         opts.draft,
        });
        let resp = self.client()
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str()
                .or_else(|| json["error"].as_str())
                .unwrap_or("unknown error");
            return Err(ToriiError::InvalidConfig(format!("GitLab API {}: {}", status, msg)));
        }
        parse_gitlab_mr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let gl_state = match state {
            "open"   => "opened",
            "closed" => "closed",
            "merged" => "merged",
            other    => other,
        };
        let url = format!(
            "{}/projects/{}/merge_requests?state={}&per_page=50",
            self.base_url, Self::project_path(owner, repo), gl_state
        );
        let resp = self.client()
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        // Capture the HTTP status before consuming the response, so we
        // can surface "GitLab API 401: 401 Unauthorized" instead of the
        // generic "Unexpected GitLab response" the old code printed when
        // the body wasn't an array. The body shape on error is
        // `{"message": "..."}` (or `{"error": "..."}` on some
        // endpoints); on success it's an array of MRs.
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str()
                .or_else(|| json["error"].as_str())
                .unwrap_or("(no message in response)");
            return Err(ToriiError::InvalidConfig(format!(
                "GitLab API {}: {} (url: {})", status, msg, url
            )));
        }
        let arr = json.as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "GitLab returned a non-array body for {}. Body: {}",
                url, json
            )))?;
        arr.iter().map(parse_gitlab_mr).collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.base_url, Self::project_path(owner, repo), number
        );
        let resp = self.client()
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API parse error: {}", e)))?;
        parse_gitlab_mr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/merge",
            self.base_url, Self::project_path(owner, repo), number
        );
        let squash = matches!(method, MergeMethod::Squash);
        let body = serde_json::json!({ "squash": squash });
        let resp = self.client()
            .put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("merge failed");
            return Err(ToriiError::InvalidConfig(format!("Merge failed: {}", msg)));
        }
        Ok(())
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.base_url, Self::project_path(owner, repo), number
        );
        let body = serde_json::json!({ "state_event": "close" });
        let resp = self.client()
            .put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("close failed");
            return Err(ToriiError::InvalidConfig(format!("Close failed: {}", msg)));
        }
        Ok(())
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.base_url, Self::project_path(owner, repo), number
        );
        let mut body = serde_json::Map::new();
        if let Some(t) = opts.title { body.insert("title".into(), t.into()); }
        if let Some(b) = opts.body  { body.insert("description".into(), b.into()); }
        if let Some(b) = opts.base  { body.insert("target_branch".into(), b.into()); }
        let resp = self.client()
            .put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::Value::Object(body))
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("update failed");
            return Err(ToriiError::InvalidConfig(format!("Update failed: {}", msg)));
        }
        Ok(())
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/repository/branches/{}",
            self.base_url, Self::project_path(owner, repo),
            crate::url::encode(branch)
        );
        let resp = self.client()
            .delete(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("delete branch failed");
            return Err(ToriiError::InvalidConfig(format!("Delete branch failed: {}", msg)));
        }
        Ok(())
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_gitlab_mr(json: &serde_json::Value) -> Result<PullRequest> {
    Ok(PullRequest {
        number:     json["iid"].as_u64().unwrap_or(0),
        title:      json["title"].as_str().unwrap_or("").to_string(),
        body:       json["description"].as_str().map(|s| s.to_string()),
        state:      json["state"].as_str().unwrap_or("").to_string(),
        head:       json["source_branch"].as_str().unwrap_or("").to_string(),
        base:       json["target_branch"].as_str().unwrap_or("").to_string(),
        author:     json["author"]["username"].as_str().unwrap_or("").to_string(),
        url:        json["web_url"].as_str().unwrap_or("").to_string(),
        draft:      json["draft"].as_bool().unwrap_or(false),
        mergeable:  json["merge_status"].as_str().map(|s| s == "can_be_merged"),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Factory
// ============================================================================

pub fn get_pr_client(platform: &str) -> Result<Box<dyn PrClient>> {
    match platform.to_lowercase().as_str() {
        "github" => Ok(Box::new(GitHubPrClient::new()?)),
        "gitlab" => Ok(Box::new(GitLabPrClient::new()?)),
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported: github, gitlab", other)
        )),
    }
}

/// Detect platform + owner/repo from the `origin` remote URL.
/// Convenience wrapper around `detect_platform_from_remote_named` for
/// callers that don't need to choose which remote to inspect.
pub fn detect_platform_from_remote(repo_path: &str) -> Option<(String, String, String)> {
    detect_platform_from_remote_named(repo_path, "origin")
}

/// Same as `detect_platform_from_remote` but takes the remote name
/// explicitly. Used by the platform-management commands
/// (`pipeline`, `job`, `package`, `release`) to support managing a
/// project mirrored across multiple platforms — e.g. gitorii itself
/// has `origin → gitlab` and `github-paskidev → github`, and a user
/// may want to query either via `--remote NAME`.
pub fn detect_platform_from_remote_named(repo_path: &str, remote_name: &str) -> Option<(String, String, String)> {
    let repo = git2::Repository::discover(repo_path).ok()?;
    let remote = repo.find_remote(remote_name).ok()?;
    let url = remote.url()?.to_string();

    let platform = if url.contains("github.com") { "github" }
        else if url.contains("gitlab.com") { "gitlab" }
        else { return None; };

    let path = if url.contains('@') {
        url.splitn(2, ':').nth(1)?
    } else {
        url.trim_start_matches("https://")
            .trim_start_matches("http://")
            .splitn(2, '/').nth(1)?
    };

    let path = path.trim_end_matches(".git");
    let mut parts = path.splitn(2, '/');
    let owner = parts.next()?.to_string();
    let repo_name = parts.next()?.to_string();

    Some((platform.to_string(), owner, repo_name))
}
