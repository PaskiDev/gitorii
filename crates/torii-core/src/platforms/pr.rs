use crate::error::{Result, ToriiError};
use serde::{Deserialize, Serialize};
// Platform-specific URL/token helpers re-exported so historical
// `crate::pr::…` paths keep working after the per-platform split.
pub(crate) use super::azure::pr::{parse_azure_url, split_azure_owner};
use super::azure::AzurePrClient;
use super::bitbucket::BitbucketPrClient;
pub use super::gitea::pr::{gitea_base_url, resolve_gitea_token};
use super::gitea::GiteaPrClient;
use super::github::GitHubPrClient;
use super::gitlab::GitLabPrClient;
use super::radicle::RadiclePrClient;
use super::sourcehut::SourcehutPrClient;

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
            MergeMethod::Merge => write!(f, "merge"),
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

pub fn get_pr_client(platform: &str) -> Result<Box<dyn PrClient>> {
    match platform.to_lowercase().as_str() {
        "github"    => Ok(Box::new(GitHubPrClient::new()?)),
        "gitlab"    => Ok(Box::new(GitLabPrClient::new()?)),
        "gitea"     => Ok(Box::new(GiteaPrClient::new()?)),
        "sourcehut" => Ok(Box::new(SourcehutPrClient::new()?)),
        "radicle"   => Ok(Box::new(RadiclePrClient::new()?)),
        "bitbucket" => Ok(Box::new(BitbucketPrClient::new()?)),
        "azure"     => Ok(Box::new(AzurePrClient::new()?)),
        other => Err(ToriiError::Unsupported(format!("Unsupported platform: {}. Supported: github, gitlab, gitea, sourcehut, radicle, bitbucket, azure", other))),
    }
}

/// Detect platform + owner/repo from the `origin` remote URL.
/// Convenience wrapper around `detect_platform_from_remote_named` for
/// callers that don't need to choose which remote to inspect.
pub fn detect_platform_from_remote(repo_path: &str) -> Option<(String, String, String)> {
    detect_platform_from_remote_named(repo_path, "origin")
}

/// 0.8.0 — fuller detect that also returns the API base URL the
/// rest of torii should hit. Tries the platforms.toml registry
/// first (so self-hosted instances get their custom URL); falls
/// back to the builtin per-platform default. Returns None if the
/// remote doesn't resolve to any known platform.
pub fn detect_platform_full(
    repo_path: &str,
    remote_name: &str,
) -> Option<(String, String, String, String)> {
    let (platform, owner, repo) = detect_platform_from_remote_named(repo_path, remote_name)?;
    let api_base_url = resolve_api_base_url(repo_path, remote_name, &platform);
    Some((platform, owner, repo, api_base_url))
}

fn resolve_api_base_url(repo_path: &str, remote_name: &str, platform: &str) -> String {
    // Try the registry first.
    if let Ok(repo) = git2::Repository::discover(repo_path) {
        if let Ok(rem) = repo.find_remote(remote_name) {
            if let Some(url) = rem.url() {
                if let Some(host) = extract_host(url) {
                    if let Some(entry) = crate::platforms_registry::find_by_host(repo_path, &host) {
                        return entry.api_base_url;
                    }
                }
            }
        }
    }
    // Fall back to the builtin default for the resolved platform.
    match platform {
        "github" => "https://api.github.com".to_string(),
        "gitlab" => "https://gitlab.com/api/v4".to_string(),
        "gitea" => "https://codeberg.org/api/v1".to_string(),
        "bitbucket" => "https://api.bitbucket.org/2.0".to_string(),
        _ => String::new(),
    }
}

/// 0.8.0 — extract the host from a git remote URL. Handles the four
/// shapes the rest of torii deals with: `https://host/...`,
/// `git@host:owner/repo.git`, `ssh://git@host:22/owner/repo.git`,
/// and the rare `host/owner/repo` shorthand we tolerate.
fn extract_host(url: &str) -> Option<String> {
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        let host = rest.split(['/', ':']).next()?;
        return Some(host.to_string());
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        // Skip optional `user@`.
        let after_user = rest.split('@').next_back()?;
        let host = after_user.split([':', '/']).next()?;
        return Some(host.to_string());
    }
    if let Some(at) = url.find('@') {
        if let Some(colon) = url[at + 1..].find(':') {
            return Some(url[at + 1..at + 1 + colon].to_string());
        }
    }
    None
}

/// Pull owner / repo out of a git remote URL. The path after the
/// host is split on `/`; the last two non-empty segments are the
/// owner + repo (with `.git` trimmed). Subgroups (GitLab) collapse
/// into the owner field — that's already how the GitLab client
/// expects it.
fn extract_owner_repo(url: &str) -> Option<(String, String)> {
    let path_part: String = if let Some(at) = url.find('@') {
        // git@host:owner/repo.git → owner/repo.git
        url[at + 1..].split_once(':')?.1.to_string()
    } else if let Some(after_scheme) = url.split("://").nth(1) {
        // https://host/owner/repo.git → owner/repo.git
        after_scheme.split_once('/')?.1.to_string()
    } else {
        url.to_string()
    };
    let cleaned = path_part.trim_end_matches('/').trim_end_matches(".git");
    let segments: Vec<&str> = cleaned.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    let repo = segments.last()?.to_string();
    let owner_segments = &segments[..segments.len() - 1];
    let owner = owner_segments.join("/");
    Some((owner, repo))
}

/// Same as `detect_platform_from_remote` but takes the remote name
/// explicitly. Used by the platform-management commands
/// (`pipeline`, `job`, `package`, `release`) to support managing a
/// project mirrored across multiple platforms — e.g. gitorii itself
/// has `origin → gitlab` and `github-paskidev → github`, and a user
/// may want to query either via `--remote NAME`.
pub fn detect_platform_from_remote_named(
    repo_path: &str,
    remote_name: &str,
) -> Option<(String, String, String)> {
    let repo = git2::Repository::discover(repo_path).ok()?;
    let remote = repo.find_remote(remote_name).ok()?;
    let url = remote.url()?.to_string();

    // 0.7.13: Codeberg (Forgejo-based) detected as "gitea" — they share
    // the same API surface. Self-hosted Gitea/Forgejo instances need
    // explicit declaration via ~/.config/torii/platforms.toml (coming
    // in 0.8.0); for now they fall through to None.
    // 0.7.15: git.sr.ht detected as "sourcehut" — issues + builds
    // supported, PR / release / package have no equivalent there.
    // 0.7.16: rad:// URLs detected as "radicle" — fully peer-to-peer,
    // all ops drive the local `rad` CLI. owner is the RID; repo is
    // unused (Radicle is per-project, not per-repo-within-org).
    // 0.7.17: bitbucket.org detected as "bitbucket". Bitbucket Cloud
    // only — self-hosted Bitbucket Data Center has a different URL
    // shape and API surface, falls through to None for now.
    // 0.7.18: Azure DevOps detected from `dev.azure.com`,
    // `ssh.dev.azure.com`, or the legacy `*.visualstudio.com`. Azure
    // uses a 3-level path (org/project/repo) which doesn't fit the
    // owner/repo shape — we pack `org/project` into `owner` and let
    // the AzureClient split it back. See parser below.
    let platform = if url.contains("github.com") {
        "github"
    } else if url.contains("gitlab.com") {
        "gitlab"
    } else if url.contains("codeberg.org") {
        "gitea"
    } else if url.contains("git.sr.ht") {
        "sourcehut"
    } else if url.starts_with("rad://") || url.starts_with("rad@") {
        "radicle"
    } else if url.contains("bitbucket.org") {
        "bitbucket"
    } else if url.contains("dev.azure.com") || url.contains(".visualstudio.com") {
        "azure"
    } else {
        // 0.8.0 — self-hosted lookup via platforms.toml registry.
        // We parse the URL host out of the remote (ssh / https
        // both work) and ask the registry for a matching entry.
        // Codeberg / Forgejo route through the Gitea client, so
        // their `kind` strings map onto the platform string the
        // rest of torii's switch tables key on.
        if let Some(host) = extract_host(&url) {
            if let Some(entry) = crate::platforms_registry::find_by_host(repo_path, &host) {
                // Map registry `kind` strings onto the platform
                // discriminators the rest of torii uses.
                let mapped: &str = match entry.kind.as_str() {
                    "gitlab" => "gitlab",
                    "github" | "github_enterprise" => "github",
                    "gitea" | "forgejo" | "codeberg" => "gitea",
                    "bitbucket" | "bitbucket_data_center" => "bitbucket",
                    other => other,
                };
                // Static-lifetime requirement of the local `platform`
                // binding above is satisfied by the matched arms;
                // for an unknown kind we bail rather than guess.
                let static_kind: &'static str = match mapped {
                    "gitlab" => "gitlab",
                    "github" => "github",
                    "gitea" => "gitea",
                    "bitbucket" => "bitbucket",
                    _ => return None,
                };
                let owner_repo = extract_owner_repo(&url)?;
                return Some((static_kind.to_string(), owner_repo.0, owner_repo.1));
            }
        }
        return None;
    };

    // Radicle URLs are `rad://<seed-host>/<RID>` — there's no
    // owner/repo split, the RID identifies the project globally. We
    // shove the RID into `owner` and leave `repo` empty so callers
    // have a non-empty key to work with.
    if platform == "radicle" {
        let rid = url
            .trim_start_matches("rad://")
            .trim_start_matches("rad@")
            .split('/')
            .next_back()?
            .trim_end_matches(".git")
            .to_string();
        return Some((platform.to_string(), rid, String::new()));
    }

    // Azure DevOps URL shapes:
    //   HTTPS modern:  https://dev.azure.com/{org}/{project}/_git/{repo}
    //   HTTPS legacy:  https://{org}.visualstudio.com/{project}/_git/{repo}
    //   SSH modern:    git@ssh.dev.azure.com:v3/{org}/{project}/{repo}
    // We pack `org/project` into the `owner` slot — the AzureClient
    // splits it on `/` at call time so api-url construction has all
    // three parts.
    if platform == "azure" {
        let (org, project, repo_name) = parse_azure_url(&url)?;
        return Some((
            platform.to_string(),
            format!("{}/{}", org, project),
            repo_name,
        ));
    }

    let path = if url.contains('@') {
        url.split_once(':')?.1
    } else {
        url.trim_start_matches("https://")
            .trim_start_matches("http://")
            .split_once('/')?
            .1
    };

    let path = path.trim_end_matches(".git");
    let mut parts = path.splitn(2, '/');
    let owner = parts.next()?.to_string();
    let repo_name = parts.next()?.to_string();

    Some((platform.to_string(), owner, repo_name))
}
