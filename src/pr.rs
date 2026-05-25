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
        crate::http::make_client()
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
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        let json = crate::http::send_json(req, "GitHub create PR")?;
        parse_github_pr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/pulls?state={}&per_page=50",
            owner, repo, state
        );
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json");
        let json = crate::http::send_json(req, &format!("GitHub (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_github_pr).collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls/{}", owner, repo, number);
        let req = self.client().get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json");
        let json = crate::http::send_json(req, &format!("GitHub PR #{}", number))?;
        parse_github_pr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls/{}/merge", owner, repo, number);
        let body = serde_json::json!({ "merge_method": method.to_string() });
        let req = self.client().put(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        crate::http::send_empty(req, "GitHub merge PR")
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls/{}", owner, repo, number);
        let body = serde_json::json!({ "state": "closed" });
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&body);
        crate::http::send_empty(req, "GitHub close PR")
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/pulls/{}", owner, repo, number);
        let mut body = serde_json::Map::new();
        if let Some(t) = opts.title { body.insert("title".into(), t.into()); }
        if let Some(b) = opts.body  { body.insert("body".into(), b.into()); }
        if let Some(b) = opts.base  { body.insert("base".into(), b.into()); }
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json")
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "GitHub update PR")
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/{}/git/refs/heads/{}", owner, repo, branch);
        let req = self.client().delete(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/vnd.github.v3+json");
        crate::http::send_empty(req, "GitHub delete branch")
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
        crate::http::make_client()
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
        let req = self.client().post(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body);
        let json = crate::http::send_json(req, "GitLab create MR")?;
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
        let req = self.client().get(&url).header("PRIVATE-TOKEN", &self.token);
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_gitlab_mr).collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.base_url, Self::project_path(owner, repo), number
        );
        let req = self.client().get(&url).header("PRIVATE-TOKEN", &self.token);
        let json = crate::http::send_json(req, &format!("GitLab MR !{}", number))?;
        parse_gitlab_mr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/merge",
            self.base_url, Self::project_path(owner, repo), number
        );
        let squash = matches!(method, MergeMethod::Squash);
        let body = serde_json::json!({ "squash": squash });
        let req = self.client().put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body);
        crate::http::send_empty(req, "GitLab merge MR")
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.base_url, Self::project_path(owner, repo), number
        );
        let body = serde_json::json!({ "state_event": "close" });
        let req = self.client().put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&body);
        crate::http::send_empty(req, "GitLab close MR")
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
        let req = self.client().put(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "GitLab update MR")
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!(
            "{}/projects/{}/repository/branches/{}",
            self.base_url, Self::project_path(owner, repo),
            crate::url::encode(branch)
        );
        let req = self.client().delete(&url).header("PRIVATE-TOKEN", &self.token);
        crate::http::send_empty(req, "GitLab delete branch")
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
// Gitea / Codeberg / Forgejo
// ============================================================================
//
// Gitea's pulls API is GitHub-shaped at `/api/v1/...`. Same `number`
// identifier, same `head`/`base`/`draft` fields, same `merge_method`
// values for merge — auth header is `token <token>` like GitHub.
// `mergeable` is exposed as a boolean rather than GitHub's null/true
// while-computing dance, so we surface it directly.

pub struct GiteaPrClient {
    token: String,
    base_url: String,
}

impl GiteaPrClient {
    pub fn new() -> Result<Self> {
        Self::new_with_host(gitea_base_url())
    }

    pub fn new_with_host(base_url: &str) -> Result<Self> {
        let token = resolve_gitea_token()?;
        Ok(Self { token, base_url: base_url.trim_end_matches('/').to_string() })
    }

    fn client(&self) -> Client { crate::http::make_client() }
    fn auth(&self) -> String { format!("token {}", self.token) }
}

impl PrClient for GiteaPrClient {
    fn create(&self, owner: &str, repo: &str, opts: CreatePrOptions) -> Result<PullRequest> {
        let url = format!("{}/api/v1/repos/{}/{}/pulls", self.base_url, owner, repo);
        let mut title = opts.title.clone();
        // Gitea has no draft flag — convention is `WIP:` prefix.
        if opts.draft && !title.to_lowercase().starts_with("wip:") {
            title = format!("WIP: {}", title);
        }
        let body = serde_json::json!({
            "title": title,
            "body":  opts.body.unwrap_or_default(),
            "head":  opts.head,
            "base":  opts.base,
        });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .json(&body);
        let json = crate::http::send_json(req, "Gitea create PR")?;
        parse_gitea_pr(&json)
    }

    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<PullRequest>> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/pulls?state={}&limit=50",
            self.base_url, owner, repo, state
        );
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Gitea (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter().map(parse_gitea_pr).collect()
    }

    fn get(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        let url = format!("{}/api/v1/repos/{}/{}/pulls/{}", self.base_url, owner, repo, number);
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("Gitea PR #{}", number))?;
        parse_gitea_pr(&json)
    }

    fn merge(&self, owner: &str, repo: &str, number: u64, method: MergeMethod) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/{}/pulls/{}/merge", self.base_url, owner, repo, number);
        let do_param = match method {
            MergeMethod::Merge  => "merge",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        };
        let body = serde_json::json!({ "Do": do_param });
        let req = self.client().post(&url)
            .header("Authorization", self.auth())
            .json(&body);
        crate::http::send_empty(req, "Gitea merge PR")
    }

    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/{}/pulls/{}", self.base_url, owner, repo, number);
        let body = serde_json::json!({ "state": "closed" });
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .json(&body);
        crate::http::send_empty(req, "Gitea close PR")
    }

    fn update(&self, owner: &str, repo: &str, number: u64, opts: UpdatePrOptions) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/{}/pulls/{}", self.base_url, owner, repo, number);
        let mut body = serde_json::Map::new();
        if let Some(t) = opts.title { body.insert("title".into(), serde_json::Value::String(t)); }
        if let Some(b) = opts.body  { body.insert("body".into(),  serde_json::Value::String(b)); }
        if let Some(base) = opts.base { body.insert("base".into(), serde_json::Value::String(base)); }
        if body.is_empty() { return Ok(()); }
        let req = self.client().patch(&url)
            .header("Authorization", self.auth())
            .json(&serde_json::Value::Object(body));
        crate::http::send_empty(req, "Gitea update PR")
    }

    fn delete_branch(&self, owner: &str, repo: &str, branch: &str) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/{}/branches/{}", self.base_url, owner, repo, branch);
        let req = self.client().delete(&url).header("Authorization", self.auth());
        crate::http::send_empty(req, "Gitea delete branch")
    }

    fn checkout_branch(&self, pr: &PullRequest) -> String {
        pr.head.clone()
    }
}

fn parse_gitea_pr(json: &serde_json::Value) -> Result<PullRequest> {
    Ok(PullRequest {
        number:     json["number"].as_u64().unwrap_or(0),
        title:      json["title"].as_str().unwrap_or("").to_string(),
        body:       json["body"].as_str().map(|s| s.to_string()),
        state:      json["state"].as_str().unwrap_or("").to_string(),
        head:       json["head"]["ref"].as_str().unwrap_or("").to_string(),
        base:       json["base"]["ref"].as_str().unwrap_or("").to_string(),
        author:     json["user"]["login"].as_str().unwrap_or("").to_string(),
        url:        json["html_url"].as_str().unwrap_or("").to_string(),
        // Gitea convention: WIP: prefix marks drafts (no native flag pre-1.19).
        draft:      json["title"].as_str().map(|s| {
                        let l = s.to_lowercase();
                        l.starts_with("wip:") || l.starts_with("[wip]") || l.starts_with("draft:")
                    }).unwrap_or(false),
        mergeable:  json["mergeable"].as_bool(),
        created_at: json["created_at"].as_str().unwrap_or("").to_string(),
    })
}

// ============================================================================
// Sourcehut (paradigm mismatch — patches go through mailing lists)
// ============================================================================
//
// Sourcehut's contribution model is **email-based patches sent to
// `~user/repo-devel@lists.sr.ht`**, not a server-side merge-request
// object. There is no REST endpoint to "create a PR" the way GitHub
// or GitLab expose one — the patch lives on the mailing list, the
// maintainer applies it locally with `torii patch apply`, then pushes.
//
// We expose a stub client that returns a clear error explaining the
// workflow, so the four CLI commands (`torii pr list/create/view/...`)
// fail with guidance instead of silently working on a wrong endpoint.
// `torii patch export <range>` + mailing the resulting `.patch` files
// is the supported flow.

pub struct SourcehutPrClient;

impl SourcehutPrClient {
    pub fn new() -> Result<Self> { Ok(Self) }
}

fn srht_pr_unsupported() -> ToriiError {
    ToriiError::InvalidConfig(
        "Sourcehut doesn't have server-side pull requests — \
         contributions are sent as `git format-patch` style emails to \
         the project's `*-devel@lists.sr.ht` mailing list. Use \
         `torii patch export <range>` to produce the .patch files and \
         mail them with `git send-email` (or your MUA). The maintainer \
         applies them with `torii patch apply`.".to_string()
    )
}

impl PrClient for SourcehutPrClient {
    fn create(&self, _o: &str, _r: &str, _opts: CreatePrOptions) -> Result<PullRequest> { Err(srht_pr_unsupported()) }
    fn list(&self, _o: &str, _r: &str, _state: &str) -> Result<Vec<PullRequest>> { Err(srht_pr_unsupported()) }
    fn get(&self, _o: &str, _r: &str, _n: u64) -> Result<PullRequest> { Err(srht_pr_unsupported()) }
    fn merge(&self, _o: &str, _r: &str, _n: u64, _m: MergeMethod) -> Result<()> { Err(srht_pr_unsupported()) }
    fn close(&self, _o: &str, _r: &str, _n: u64) -> Result<()> { Err(srht_pr_unsupported()) }
    fn update(&self, _o: &str, _r: &str, _n: u64, _opts: UpdatePrOptions) -> Result<()> { Err(srht_pr_unsupported()) }
    fn delete_branch(&self, _o: &str, _r: &str, _b: &str) -> Result<()> { Err(srht_pr_unsupported()) }
    fn checkout_branch(&self, pr: &PullRequest) -> String { pr.head.clone() }
}

// ============================================================================
// Factory
// ============================================================================

pub fn get_pr_client(platform: &str) -> Result<Box<dyn PrClient>> {
    match platform.to_lowercase().as_str() {
        "github"    => Ok(Box::new(GitHubPrClient::new()?)),
        "gitlab"    => Ok(Box::new(GitLabPrClient::new()?)),
        "gitea"     => Ok(Box::new(GiteaPrClient::new()?)),
        "sourcehut" => Ok(Box::new(SourcehutPrClient::new()?)),
        other => Err(ToriiError::InvalidConfig(
            format!("Unsupported platform: {}. Supported: github, gitlab, gitea, sourcehut", other)
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

    // 0.7.13: Codeberg (Forgejo-based) detected as "gitea" — they share
    // the same API surface. Self-hosted Gitea/Forgejo instances need
    // explicit declaration via ~/.config/torii/platforms.toml (coming
    // in 0.8.0); for now they fall through to None.
    // 0.7.15: git.sr.ht detected as "sourcehut" — issues + builds
    // supported, PR / release / package have no equivalent there.
    let platform = if url.contains("github.com") { "github" }
        else if url.contains("gitlab.com") { "gitlab" }
        else if url.contains("codeberg.org") { "gitea" }
        else if url.contains("git.sr.ht") { "sourcehut" }
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

/// Map a "gitea" platform value to its base URL. Today this is always
/// `https://codeberg.org`; in 0.8.0 with `platforms.toml` support, the
/// caller will be able to resolve self-hosted instances per-remote.
///
/// Centralised here so adding a per-host map later only touches one site.
pub fn gitea_base_url() -> &'static str {
    "https://codeberg.org"
}

/// Resolve the Gitea / Codeberg / Forgejo token. The auth subsystem
/// accepts all three names as distinct providers (because users like
/// to call them by their brand), but the API surface is the same — so
/// we try all three in order and return the first one set.
///
/// Used by every Gitea* client (release / issue / pr / pipeline) so
/// `torii auth set codeberg YOUR_TOKEN` works without forcing the user
/// to learn that "the canonical provider is gitea".
pub fn resolve_gitea_token() -> Result<String> {
    for provider in ["gitea", "codeberg", "forgejo"] {
        if let Some(t) = crate::auth::resolve_token(provider, ".").value {
            return Ok(t);
        }
    }
    Err(ToriiError::InvalidConfig(
        "Gitea / Codeberg / Forgejo token not found. Run: torii auth set codeberg YOUR_TOKEN".to_string()
    ))
}
