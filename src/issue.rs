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
        crate::http::make_client()
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
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        // filter out PRs (GitHub issues API returns PRs too)
        Ok(crate::http::extract_array(&json, &url)?
            .iter()
            .filter(|v| v["pull_request"].is_null())
            .filter_map(|v| parse_github_issue(v).ok())
            .collect())
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!("https://api.github.com/repos/{}/{}/issues", owner, repo);
        let body = serde_json::json!({
            "title": opts.title,
            "body":  opts.body.unwrap_or_default(),
        });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        let json = crate::http::send_json(req, "GitHub create issue")?;
        parse_github_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/issues/{}", owner, repo, number);
        let body = serde_json::json!({ "state": "closed" });
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        crate::http::send_empty(req, "GitHub close issue")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/issues/{}/comments", owner, repo, number);
        let payload = serde_json::json!({ "body": body });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&payload);
        crate::http::send_empty(req, "GitHub comment issue")
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
        crate::http::make_client()
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
        let req = self.client().get(&url).header("PRIVATE-TOKEN", &self.token);
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_gitlab_issue).collect()
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
        let req = self.client().post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body);
        let json = crate::http::send_json(req, "GitLab create issue")?;
        parse_gitlab_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/projects/{}/issues/{}",
            self.base_url, Self::project_path(owner, repo), number
        );
        let body = serde_json::json!({ "state_event": "close" });
        let req = self.client().put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body);
        crate::http::send_empty(req, "GitLab close issue")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/issues/{}/notes",
            self.base_url, Self::project_path(owner, repo), number
        );
        let payload = serde_json::json!({ "body": body });
        let req = self.client().post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&payload);
        crate::http::send_empty(req, "GitLab comment issue")
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

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth(&self) -> String { format!("token {}", self.token) }
}

impl IssueClient for GiteaIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        // Gitea uses `type=issues` to exclude PRs from the listing.
        let url = format!(
            "{}/api/v1/repos/{}/{}/issues?state={}&type=issues&limit=50",
            self.base_url, owner, repo, state
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Gitea (url: {})", url))?;
        Ok(crate::http::extract_array(&json, &url)?
            .iter().filter_map(|v| parse_gitea_issue(v).ok()).collect())
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!("{}/api/v1/repos/{}/{}/issues", self.base_url, owner, repo);
        let body = serde_json::json!({
            "title": opts.title,
            "body":  opts.body.unwrap_or_default(),
        });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .json(&body);
        let json = crate::http::send_json(req, "Gitea create issue")?;
        parse_gitea_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/{}/issues/{}", self.base_url, owner, repo, number);
        let body = serde_json::json!({ "state": "closed" });
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .json(&body);
        crate::http::send_empty(req, "Gitea close issue")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/{}/issues/{}/comments", self.base_url, owner, repo, number);
        let payload = serde_json::json!({ "body": body });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .json(&payload);
        crate::http::send_empty(req, "Gitea comment issue")
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
