use serde::{Deserialize, Serialize};
use serde_json::Value;
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
        let req = self.client().get(&url).header("Authorization", format!("Bearer {}", self.token));
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
            .header("Authorization", format!("Bearer {}", self.token))
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
            .header("Authorization", format!("Bearer {}", self.token))
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
            .header("Authorization", format!("Bearer {}", self.token))
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

// ── Sourcehut (todo.sr.ht) ───────────────────────────────────────────────────
//
// Sourcehut's bug tracker lives on a separate subdomain from the git
// host. The convention is `~user/<tracker-name>` where tracker name is
// usually the same as the repo (but not enforced) — projects sometimes
// have multiple trackers (e.g. `-bugs`, `-features`). We assume
// `tracker_name == repo_name`; if the user uses a different naming
// scheme they can pass `--remote` to a remote whose URL points at the
// correct tracker (in 0.8.0 with platforms.toml this will be
// configurable per-host).

pub struct SourcehutIssueClient {
    token: String,
}

impl SourcehutIssueClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("sourcehut", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "Sourcehut token not found. Generate one at https://meta.sr.ht/oauth and run: \
                 torii auth set sourcehut YOUR_TOKEN".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth(&self) -> String { format!("token {}", self.token) }
}

impl IssueClient for SourcehutIssueClient {
    fn list(&self, owner: &str, repo: &str, _state: &str) -> Result<Vec<Issue>> {
        // todo.sr.ht doesn't support per-state filtering on the list
        // endpoint — we fetch then filter client-side. The owner already
        // includes the `~` prefix from the URL parser.
        let url = format!(
            "https://todo.sr.ht/api/trackers/{}/{}/tickets",
            owner, repo
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Sourcehut todo (url: {})", url))?;
        let arr = json["results"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Sourcehut returned no `results` array. Body: {}", json
            )))?;
        Ok(arr.iter().filter_map(|v| parse_sourcehut_issue(v).ok()).collect())
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!(
            "https://todo.sr.ht/api/trackers/{}/{}/tickets",
            owner, repo
        );
        let body = serde_json::json!({
            "title":       opts.title,
            "description": opts.body.unwrap_or_default(),
        });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .json(&body);
        let json = crate::http::send_json(req, "Sourcehut create ticket")?;
        parse_sourcehut_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        // todo.sr.ht ticket updates go through `PUT /tickets/{id}` with
        // a `status: "resolved"` and `resolution: "fixed"` body.
        let url = format!(
            "https://todo.sr.ht/api/trackers/{}/{}/tickets/{}",
            owner, repo, number
        );
        let body = serde_json::json!({
            "status":     "resolved",
            "resolution": "fixed",
        });
        let req = self.client().put(&url)
            .header("Authorization", self.auth())
            .json(&body);
        crate::http::send_empty(req, "Sourcehut close ticket")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "https://todo.sr.ht/api/trackers/{}/{}/tickets/{}/events",
            owner, repo, number
        );
        let payload = serde_json::json!({ "comment": body });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .json(&payload);
        crate::http::send_empty(req, "Sourcehut comment ticket")
    }
}

fn parse_sourcehut_issue(json: &serde_json::Value) -> Result<Issue> {
    let number = json["id"].as_u64().unwrap_or(0);
    let owner = json["tracker"]["owner"]["canonical_name"].as_str().unwrap_or("");
    let tracker = json["tracker"]["name"].as_str().unwrap_or("");
    Ok(Issue {
        number,
        title:      json["title"].as_str().unwrap_or("").to_string(),
        body:       json["description"].as_str().map(String::from),
        // todo.sr.ht uses "reported" (open) and "resolved" (closed).
        state:      match json["status"].as_str().unwrap_or("") {
            "reported" => "open".to_string(),
            "resolved" => "closed".to_string(),
            other      => other.to_string(),
        },
        author:     json["submitter"]["canonical_name"].as_str().unwrap_or("").to_string(),
        url:        format!("https://todo.sr.ht/{}/{}/{}", owner, tracker, number),
        labels:     json["labels"].as_array().map(|a| {
            a.iter().filter_map(|l| l["name"].as_str().map(String::from)).collect()
        }).unwrap_or_default(),
        assignees:  json["assignees"].as_array().map(|a| {
            a.iter().filter_map(|u| u["canonical_name"].as_str().map(String::from)).collect()
        }).unwrap_or_default(),
        created_at: json["created"].as_str().unwrap_or("").to_string(),
        comments:   0, // todo.sr.ht doesn't expose a count on the list endpoint
    })
}

// ── Radicle (peer-to-peer, via `rad` CLI) ────────────────────────────────────
//
// Radicle stores issues in special refs inside the project itself —
// no central server. We drive the `rad issue` subcommand directly.
// owner/repo args from the URL parser hold the RID and (empty) repo
// part; `rad` resolves the project from the current working dir's
// `.git` config, so we don't need to pass the RID per call.

pub struct RadicleIssueClient;

impl RadicleIssueClient {
    pub fn new() -> Result<Self> { Ok(Self) }
}

impl IssueClient for RadicleIssueClient {
    fn list(&self, _o: &str, _r: &str, state: &str) -> Result<Vec<Issue>> {
        // `rad issue list --state open|closed|all`. We translate the
        // platform-agnostic "open" / "closed" / "all" into rad's
        // matching state names.
        let st = match state { "open" => "open", "closed" => "closed", _ => "all" };
        let json = crate::radicle::run_rad_json(&["issue", "list", "--state", st])?;
        let arr = json.as_array()
            .ok_or_else(|| ToriiError::InvalidConfig("rad issue list: expected array".into()))?;
        Ok(arr.iter().filter_map(|v| parse_radicle_issue(v).ok()).collect())
    }

    fn create(&self, _o: &str, _r: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let body = opts.body.unwrap_or_default();
        let stdout = crate::radicle::run_rad(&[
            "issue", "open",
            "--title", &opts.title,
            "--description", &body,
        ])?;
        // `rad issue open` prints the new issue id on the last line.
        let id = stdout.trim().lines().last().unwrap_or("").trim().to_string();
        Ok(Issue {
            number:     0, // Radicle issues are identified by hash, not number
            title:      opts.title,
            body:       Some(body),
            state:      "open".to_string(),
            author:     String::new(),
            url:        format!("rad:{}", id),
            labels:     vec![],
            assignees:  vec![],
            created_at: String::new(),
            comments:   0,
        })
    }

    fn close(&self, _o: &str, _r: &str, number: u64) -> Result<()> {
        // Radicle uses string ids, not numbers — torii's IssueClient
        // signature takes u64 so we can't address a real radicle issue
        // through this method. Surface a clear error pointing at the
        // CLI direct path.
        Err(ToriiError::InvalidConfig(format!(
            "Radicle issues are identified by hash, not number. `torii issue close {}` \
             cannot be mapped 1:1 — use `rad issue state <id> --closed` directly until \
             torii's IssueClient trait grows a string-id variant.",
            number
        )))
    }

    fn comment(&self, _o: &str, _r: &str, number: u64, _body: &str) -> Result<()> {
        Err(ToriiError::InvalidConfig(format!(
            "Radicle issues are identified by hash, not number. `torii issue comment {}` \
             can't address a hash-id issue — use `rad issue comment <id>` directly.",
            number
        )))
    }
}

fn parse_radicle_issue(v: &Value) -> Result<Issue> {
    let id = v["id"].as_str().unwrap_or("");
    Ok(Issue {
        number:     0,
        title:      v["title"].as_str().unwrap_or("").to_string(),
        body:       v["description"].as_str().map(String::from),
        state:      v["state"]["status"].as_str().unwrap_or("open").to_string(),
        author:     v["author"]["alias"].as_str()
                        .or_else(|| v["author"]["id"].as_str())
                        .unwrap_or("").to_string(),
        url:        format!("rad:{}", id),
        labels:     v["labels"].as_array().map(|a| {
            a.iter().filter_map(|l| l.as_str().map(String::from)).collect()
        }).unwrap_or_default(),
        assignees:  v["assignees"].as_array().map(|a| {
            a.iter().filter_map(|u| u.as_str().map(String::from)).collect()
        }).unwrap_or_default(),
        created_at: v["timestamp"].as_str().unwrap_or("").to_string(),
        comments:   v["comments"].as_u64().unwrap_or(0),
    })
}

// ── Bitbucket Cloud (issues — deprecated but still works if enabled) ────────
//
// Bitbucket Cloud's issue tracker is technically deprecated in favour
// of third-party trackers (Jira), but the REST endpoint still works
// for repos that have issues enabled. On repos without issues
// enabled, the API returns 404 — we surface that with a hint.

pub struct BitbucketIssueClient {
    token: String,
}

impl BitbucketIssueClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("bitbucket", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "Bitbucket token not found. Create an app password at \
                 https://bitbucket.org/account/settings/app-passwords/ \
                 and run: torii auth set bitbucket USERNAME:APP_PASSWORD".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth(&self) -> String {
        if self.token.contains(':') {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&self.token);
            format!("Basic {}", b64)
        } else {
            format!("Bearer {}", self.token)
        }
    }
}

impl IssueClient for BitbucketIssueClient {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>> {
        // Bitbucket issue states: new/open/resolved/on hold/invalid/
        // duplicate/wontfix/closed. We collapse "open" to the two
        // active ones and "closed" to the terminal set. Q-syntax is
        // Bitbucket-specific (`state="new" OR state="open"`).
        let q = match state {
            "open"   => r#"state="new" OR state="open""#.to_string(),
            "closed" => r#"state="resolved" OR state="closed" OR state="invalid" OR state="duplicate" OR state="wontfix""#.to_string(),
            _        => String::new(),
        };
        let mut url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/issues?pagelen=50",
            owner, repo
        );
        if !q.is_empty() {
            url.push_str(&format!("&q={}", crate::url::encode(&q)));
        }
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let json = crate::http::send_json(req, &format!("Bitbucket (url: {})", url))?;
        let arr = json["values"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Bitbucket returned no `values` array — does the repo have issues enabled? \
                 Body: {}", json
            )))?;
        arr.iter().map(parse_bitbucket_issue).collect()
    }

    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/issues",
            owner, repo
        );
        let body = serde_json::json!({
            "title":   opts.title,
            "content": {
                "raw":    opts.body.unwrap_or_default(),
                "markup": "markdown",
            },
        });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        let json = crate::http::send_json(req, "Bitbucket create issue")?;
        parse_bitbucket_issue(&json)
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/issues/{}",
            owner, repo, number
        );
        let body = serde_json::json!({ "state": "resolved" });
        let req = self.client().put(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Bitbucket close issue")
    }

    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()> {
        let url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/issues/{}/comments",
            owner, repo, number
        );
        let payload = serde_json::json!({
            "content": { "raw": body, "markup": "markdown" },
        });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&payload);
        crate::http::send_empty(req, "Bitbucket comment issue")
    }
}

fn parse_bitbucket_issue(json: &serde_json::Value) -> Result<Issue> {
    Ok(Issue {
        number:     json["id"].as_u64().unwrap_or(0),
        title:      json["title"].as_str().unwrap_or("").to_string(),
        body:       json["content"]["raw"].as_str().map(String::from),
        state:      match json["state"].as_str().unwrap_or("") {
            "new" | "open"                                              => "open".to_string(),
            "resolved" | "closed" | "invalid" | "duplicate" | "wontfix" => "closed".to_string(),
            other                                                       => other.to_string(),
        },
        author:     json["reporter"]["display_name"].as_str()
                        .or_else(|| json["reporter"]["username"].as_str())
                        .unwrap_or("").to_string(),
        url:        json["links"]["html"]["href"].as_str().unwrap_or("").to_string(),
        labels:     json["kind"].as_str().map(|k| vec![k.to_string()]).unwrap_or_default(),
        assignees:  json["assignee"]["display_name"].as_str()
                        .or_else(|| json["assignee"]["username"].as_str())
                        .map(|s| vec![s.to_string()])
                        .unwrap_or_default(),
        created_at: json["created_on"].as_str().unwrap_or("").to_string(),
        comments:   0,
    })
}

// ── Azure DevOps (Work Items via WIQL) ──────────────────────────────────────
//
// Azure's "issues" are Work Items. The list endpoint takes a WIQL
// (Work Item Query Language, SQL-like) query that returns IDs, then a
// second call fetches the full records by id. We bundle that into a
// single torii `list` call.
//
// Work item types depend on the project's process template (Agile /
// Scrum / Basic / CMMI). The Basic process uses `Issue`; Agile uses
// `User Story` and `Bug`. We default to `Issue` for create — projects
// on a non-Basic process will need to extend the create flow later.

pub struct AzureIssueClient {
    token: String,
}

impl AzureIssueClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("azure", ".").value
            .ok_or_else(|| ToriiError::InvalidConfig(
                "Azure DevOps PAT not found. Create at https://dev.azure.com/{org}/_usersSettings/tokens \
                 with `Work Items (read/write)` scope, then: torii auth set azure YOUR_PAT".to_string()
            ))?;
        Ok(Self { token })
    }

    fn client(&self) -> Client { crate::http::make_client() }

    fn auth(&self) -> String {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(format!(":{}", self.token));
        format!("Basic {}", b64)
    }
}

impl IssueClient for AzureIssueClient {
    fn list(&self, owner: &str, _repo: &str, state: &str) -> Result<Vec<Issue>> {
        // Azure Issues are project-scoped, not repo-scoped — we ignore
        // `repo` here. The WIQL query filters by State.
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let state_filter = match state {
            "open"   => r#"[System.State] <> 'Closed' AND [System.State] <> 'Resolved' AND [System.State] <> 'Done' AND [System.State] <> 'Removed'"#,
            "closed" => r#"([System.State] = 'Closed' OR [System.State] = 'Resolved' OR [System.State] = 'Done')"#,
            _        => "[System.Id] > 0", // dummy always-true
        };
        let query = format!(
            "SELECT [System.Id] FROM workitems WHERE [System.TeamProject] = '{}' AND {} ORDER BY [System.Id] DESC",
            project, state_filter
        );

        // Step 1: WIQL query → list of IDs.
        let wiql_url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/wiql?api-version=7.0&$top=50",
            org, project
        );
        let wiql_req = self.client().post(&wiql_url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&serde_json::json!({ "query": query }));
        let wiql_json = crate::http::send_json(wiql_req, "Azure WIQL")?;
        let ids: Vec<u64> = wiql_json["workItems"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v["id"].as_u64()).collect())
            .unwrap_or_default();
        if ids.is_empty() { return Ok(vec![]); }

        // Step 2: batch GET work items by id.
        let ids_csv = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let fields = "System.Id,System.Title,System.Description,System.State,\
                      System.CreatedBy,System.CreatedDate,System.AssignedTo,System.Tags";
        let wi_url = format!(
            "https://dev.azure.com/{}/_apis/wit/workitems?ids={}&fields={}&api-version=7.0",
            org, ids_csv, fields
        );
        let wi_req = self.client().get(&wi_url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let wi_json = crate::http::send_json(wi_req, "Azure get work items")?;
        let arr = wi_json["value"].as_array()
            .ok_or_else(|| ToriiError::InvalidConfig(format!(
                "Azure returned no `value` array. Body: {}", wi_json
            )))?;
        let org_for_url = org.clone();
        Ok(arr.iter().filter_map(|v| parse_azure_work_item(v, &org_for_url).ok()).collect())
    }

    fn create(&self, owner: &str, _repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        // POST body is JSON-Patch — the Content-Type matters.
        let mut ops = vec![
            serde_json::json!({ "op": "add", "path": "/fields/System.Title", "value": opts.title }),
        ];
        if let Some(b) = opts.body {
            ops.push(serde_json::json!({ "op": "add", "path": "/fields/System.Description", "value": b }));
        }
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/workitems/$Issue?api-version=7.0",
            org, project
        );
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .header("Content-Type", "application/json-patch+json")
            .header("Accept", "application/json")
            .json(&serde_json::Value::Array(ops));
        let json = crate::http::send_json(req, "Azure create work item")?;
        parse_azure_work_item(&json, &org)
    }

    fn close(&self, owner: &str, _repo: &str, number: u64) -> Result<()> {
        let (org, _project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/_apis/wit/workitems/{}?api-version=7.0",
            org, number
        );
        let body = serde_json::json!([
            { "op": "add", "path": "/fields/System.State", "value": "Closed" }
        ]);
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .header("Content-Type", "application/json-patch+json")
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Azure close work item")
    }

    fn comment(&self, owner: &str, _repo: &str, number: u64, body: &str) -> Result<()> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        // Comments endpoint is still preview as of api-version 7.1.
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/workitems/{}/comments?api-version=7.1-preview.3",
            org, project, number
        );
        let payload = serde_json::json!({ "text": body });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&payload);
        crate::http::send_empty(req, "Azure comment work item")
    }
}

fn parse_azure_work_item(json: &serde_json::Value, org: &str) -> Result<Issue> {
    let id = json["id"].as_u64().unwrap_or(0);
    let fields = &json["fields"];
    let state_raw = fields["System.State"].as_str().unwrap_or("");
    let project = fields["System.TeamProject"].as_str().unwrap_or("");
    Ok(Issue {
        number:     id,
        title:      fields["System.Title"].as_str().unwrap_or("").to_string(),
        body:       fields["System.Description"].as_str().map(String::from),
        state:      match state_raw {
            "New" | "Active" | "Open" | "Approved" | "To Do" | "Committed" | "In Progress" =>
                "open".to_string(),
            "Closed" | "Resolved" | "Done" | "Removed" =>
                "closed".to_string(),
            other => other.to_string(),
        },
        author:     fields["System.CreatedBy"]["displayName"].as_str()
                        .or_else(|| fields["System.CreatedBy"].as_str())
                        .unwrap_or("").to_string(),
        url:        if !project.is_empty() {
            format!("https://dev.azure.com/{}/{}/_workitems/edit/{}", org, project, id)
        } else {
            json["url"].as_str().unwrap_or("").to_string()
        },
        labels:     fields["System.Tags"].as_str()
                        .map(|s| s.split(';').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
                        .unwrap_or_default(),
        assignees:  fields["System.AssignedTo"]["displayName"].as_str()
                        .or_else(|| fields["System.AssignedTo"].as_str())
                        .map(|s| vec![s.to_string()])
                        .unwrap_or_default(),
        created_at: fields["System.CreatedDate"].as_str().unwrap_or("").to_string(),
        comments:   0,
    })
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub fn get_issue_client(platform: &str) -> Result<Box<dyn IssueClient>> {
    match platform.to_lowercase().as_str() {
        "github"    => Ok(Box::new(GitHubIssueClient::new()?)),
        "gitlab"    => Ok(Box::new(GitLabIssueClient::new()?)),
        "gitea"     => Ok(Box::new(GiteaIssueClient::new()?)),
        "sourcehut" => Ok(Box::new(SourcehutIssueClient::new()?)),
        "radicle"   => Ok(Box::new(RadicleIssueClient::new()?)),
        "bitbucket" => Ok(Box::new(BitbucketIssueClient::new()?)),
        "azure"     => Ok(Box::new(AzureIssueClient::new()?)),
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported: github, gitlab, gitea, sourcehut, radicle, bitbucket, azure", other)
        )),
    }
}
