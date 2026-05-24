use serde::{Deserialize, Serialize};
use reqwest::blocking::Client;
use crate::error::{Result, ToriiError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub author: String,
    pub url: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub created_at: String,
    pub comments: u64,
}

#[derive(Debug, Clone)]
pub struct CreateIssueOptions {
    pub title: String,
    pub body: Option<String>,
}

pub trait IssueClient: Send {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>>;
    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue>;
    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()>;
    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()>;
}

// ── GitHub ────────────────────────────────────────────────────────────────────

pub struct GitHubIssueClient {
    token: String,
}

impl GitHubIssueClient {
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

impl IssueClient for GitHubIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/issues?state={}&per_page=50",
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
        // filter out PRs (GitHub issues API returns PRs too)
        Ok(arr.iter().filter(|v| v["pull_request"].is_null()).filter_map(|v| parse_github_issue(v).ok()).collect())
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!("https://api.github.com/repos/{}/{}/issues", owner, repo);
        let body = serde_json::json!({
            "title": opts.title,
            "body":  opts.body.unwrap_or_default(),
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
        parse_github_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/issues/{}", owner, repo, number);
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

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/issues/{}/comments", owner, repo, number);
        let payload = serde_json::json!({ "body": body });
        let resp = self.client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&payload)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitHub API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("comment failed");
            return Err(ToriiError::InvalidConfig(format!("Comment failed: {}", msg)));
        }
        Ok(())
    }
}

fn parse_github_issue(json: &serde_json::Value) -> Result<Issue> {
    Ok(Issue {
        number:     json["number"].as_u64().unwrap_or(0),
        title:      json["title"].as_str().unwrap_or("").to_string(),
        body:       json["body"].as_str().map(|s| s.to_string()),
        state:      json["state"].as_str().unwrap_or("").to_string(),
        author:     json["user"]["login"].as_str().unwrap_or("").to_string(),
        url:        json["html_url"].as_str().unwrap_or("").to_string(),
        labels:     json["labels"].as_array().map(|a| {
            a.iter().filter_map(|l| l["name"].as_str().map(|s| s.to_string())).collect()
        }).unwrap_or_default(),
        assignees:  json["assignees"].as_array().map(|a| {
            a.iter().filter_map(|u| u["login"].as_str().map(|s| s.to_string())).collect()
        }).unwrap_or_default(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
        comments:   json["comments"].as_u64().unwrap_or(0),
    })
}

// ── GitLab ────────────────────────────────────────────────────────────────────

pub struct GitLabIssueClient {
    token: String,
    base_url: String,
}

impl GitLabIssueClient {
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

impl IssueClient for GitLabIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        let gl_state = match state {
            "open"   => "opened",
            "closed" => "closed",
            other    => other,
        };
        let url = format!(
            "{}/projects/{}/issues?state={}&per_page=50",
            self.base_url, Self::project_path(owner, repo), gl_state
        );
        let resp = self.client()
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        // Same pattern as pr.rs::list — check status before assuming the
        // body is an array of issues. The pre-0.7.5 code printed
        // "Unexpected GitLab response" for any non-200, swallowing the
        // real reason (bad token, 404 project path, etc.).
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
        arr.iter().map(|v| parse_gitlab_issue(v)).collect()
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!(
            "{}/projects/{}/issues",
            self.base_url, Self::project_path(owner, repo)
        );
        let body = serde_json::json!({
            "title":       opts.title,
            "description": opts.body.unwrap_or_default(),
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
        parse_gitlab_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/projects/{}/issues/{}",
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

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/issues/{}/notes",
            self.base_url, Self::project_path(owner, repo), number
        );
        let payload = serde_json::json!({ "body": body });
        let resp = self.client()
            .post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&payload)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("GitLab API error: {}", e)))?;
        if !resp.status().is_success() {
            let json: serde_json::Value = resp.json().unwrap_or_default();
            let msg = json["message"].as_str().unwrap_or("comment failed");
            return Err(ToriiError::InvalidConfig(format!("Comment failed: {}", msg)));
        }
        Ok(())
    }
}

fn parse_gitlab_issue(json: &serde_json::Value) -> Result<Issue> {
    Ok(Issue {
        number:     json["iid"].as_u64().unwrap_or(0),
        title:      json["title"].as_str().unwrap_or("").to_string(),
        body:       json["description"].as_str().map(|s| s.to_string()),
        state:      json["state"].as_str().unwrap_or("").to_string(),
        author:     json["author"]["username"].as_str().unwrap_or("").to_string(),
        url:        json["web_url"].as_str().unwrap_or("").to_string(),
        labels:     json["labels"].as_array().map(|a| {
            a.iter().filter_map(|l| l.as_str().map(|s| s.to_string())).collect()
        }).unwrap_or_default(),
        assignees:  json["assignees"].as_array().map(|a| {
            a.iter().filter_map(|u| u["username"].as_str().map(|s| s.to_string())).collect()
        }).unwrap_or_default(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
        comments:   json["user_notes_count"].as_u64().unwrap_or(0),
    })
}

// ── Gitea / Codeberg / Forgejo ────────────────────────────────────────────────
//
// Gitea's issues API mirrors GitHub's at `/api/v1/...` — same field
// names, same `number` identifier (per-repo), same auth header.

pub struct GiteaIssueClient {
    token: String,
    base_url: String,
}

impl GiteaIssueClient {
    pub fn new() -> Result<Self> {
        Self::new_with_host(crate::pr::gitea_base_url())
    }

    pub fn new_with_host(base_url: &str) -> Result<Self> {
        let token = crate::pr::resolve_gitea_token()?;
        Ok(Self { token, base_url: base_url.trim_end_matches('/').to_string() })
    }

    fn client(&self) -> Client { Client::builder().user_agent("gitorii-cli").build().unwrap() }
    fn auth(&self) -> String { format!("token {}", self.token) }
}

impl IssueClient for GiteaIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        // Gitea uses `type=issues` to exclude PRs from the listing.
        let url = format!(
            "{}/api/v1/repos/{}/{}/issues?state={}&type=issues&limit=50",
            self.base_url, owner, repo, state
        );
        let resp = self.client().get(&url)
            .header("Authorization", self.auth())
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("Gitea API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("Gitea API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str().unwrap_or("(no message)");
            return Err(ToriiError::InvalidConfig(format!(
                "Gitea API {}: {} (url: {})", status, msg, url
            )));
        }
        let arr = json.as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Gitea returned non-array for {}. Body: {}", url, json
            )))?;
        Ok(arr.iter().filter_map(|v| parse_gitea_issue(v).ok()).collect())
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!("{}/api/v1/repos/{}/{}/issues", self.base_url, owner, repo);
        let body = serde_json::json!({
            "title": opts.title,
            "body":  opts.body.unwrap_or_default(),
        });
        let resp = self.client().post(&url)
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("Gitea API error: {}", e)))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("Gitea API parse error: {}", e)))?;
        if !status.is_success() {
            let msg = json["message"].as_str().unwrap_or("(no message)");
            return Err(ToriiError::InvalidConfig(format!(
                "Gitea API {}: {} (url: {})", status, msg, url
            )));
        }
        parse_gitea_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/{}/issues/{}", self.base_url, owner, repo, number);
        let body = serde_json::json!({ "state": "closed" });
        let resp = self.client().patch(&url)
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("Gitea API error: {}", e)))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let txt = resp.text().unwrap_or_default();
            return Err(ToriiError::InvalidConfig(format!("Gitea API {} close failed: {}", s, txt)));
        }
        Ok(())
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/{}/issues/{}/comments", self.base_url, owner, repo, number);
        let payload = serde_json::json!({ "body": body });
        let resp = self.client().post(&url)
            .header("Authorization", self.auth())
            .json(&payload)
            .send()
            .map_err(|e| ToriiError::InvalidConfig(format!("Gitea API error: {}", e)))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let txt = resp.text().unwrap_or_default();
            return Err(ToriiError::InvalidConfig(format!("Gitea API {} comment failed: {}", s, txt)));
        }
        Ok(())
    }
}

fn parse_gitea_issue(json: &serde_json::Value) -> Result<Issue> {
    Ok(Issue {
        number:     json["number"].as_u64().unwrap_or(0),
        title:      json["title"].as_str().unwrap_or("").to_string(),
        body:       json["body"].as_str().map(|s| s.to_string()),
        state:      json["state"].as_str().unwrap_or("").to_string(),
        author:     json["user"]["login"].as_str().unwrap_or("").to_string(),
        url:        json["html_url"].as_str().unwrap_or("").to_string(),
        labels:     json["labels"].as_array().map(|a| {
            a.iter().filter_map(|l| l["name"].as_str().map(|s| s.to_string())).collect()
        }).unwrap_or_default(),
        assignees:  json["assignees"].as_array().map(|a| {
            a.iter().filter_map(|u| u["login"].as_str().map(|s| s.to_string())).collect()
        }).unwrap_or_default(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
        comments:   json["comments"].as_u64().unwrap_or(0),
    })
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub fn get_issue_client(platform: &str) -> Result<Box<dyn IssueClient>> {
    match platform.to_lowercase().as_str() {
        "github" => Ok(Box::new(GitHubIssueClient::new()?)),
        "gitlab" => Ok(Box::new(GitLabIssueClient::new()?)),
        "gitea"  => Ok(Box::new(GiteaIssueClient::new()?)),
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported: github, gitlab, gitea", other)
        )),
    }
}
