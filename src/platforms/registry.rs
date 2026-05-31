//! 0.8.0 — registry of self-hosted platforms.
//!
//! `platforms.toml` lets the user declare instances that don't live
//! on the well-known SaaS domains: a self-hosted GitLab, a Gitea /
//! Forgejo instance, a GitHub Enterprise Server, a Bitbucket Data
//! Center. Each entry maps a `domain` (matched against the remote
//! URL) to the API + web base URLs the rest of torii's platform
//! clients should hit.
//!
//! Two on-disk locations:
//!
//! - Global: `~/.config/torii/platforms.toml`
//! - Per-repo: `<repo>/.torii/platforms.toml`
//!
//! Merge order: local overrides global, by `name`. Builtins
//! (github.com, gitlab.com, codeberg.org, etc.) live in code and
//! get returned at the end of `list()`; a user entry with the same
//! `name` shadows the builtin.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, ToriiError};

/// One platform entry. Mirrors the columns the `platforms list`
/// command surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformEntry {
    /// Short identifier, e.g. "work-gitlab", "ghe", "forge".
    pub name: String,
    /// Implementation kind. Drives which client we construct.
    /// Accepted: `gitlab`, `gitea`, `forgejo`, `codeberg`,
    /// `github`, `github_enterprise`, `bitbucket`,
    /// `bitbucket_data_center`. Codeberg/Forgejo route through the
    /// Gitea client.
    pub kind: String,
    /// Domain matched against the remote URL host. e.g.
    /// "gitlab.empresa.com", "ghe.work.io". Without scheme.
    pub domain: String,
    /// API base URL. e.g. `https://gitlab.empresa.com/api/v4` for
    /// GitLab, `https://ghe.work.io/api/v3` for GitHub Enterprise.
    pub api_base_url: String,
    /// Web base URL — what we show users and what OAuth flows hit.
    /// e.g. `https://gitlab.empresa.com`.
    pub web_base_url: String,
    /// OAuth `client_id` override. When set, supersedes the bundled
    /// client_id for OAuth flows against this instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// On-disk shape — TOML root carries a single `[[platform]]` array.
#[derive(Debug, Default, Deserialize, Serialize)]
struct OnDisk {
    #[serde(default, rename = "platform")]
    platforms: Vec<PlatformEntry>,
}

fn global_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("torii").join("platforms.toml"))
}

fn local_path<P: AsRef<Path>>(repo_path: P) -> PathBuf {
    repo_path.as_ref().join(".torii").join("platforms.toml")
}

fn load_file(path: &Path) -> Vec<PlatformEntry> {
    if !path.exists() {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    let Ok(parsed) = toml::from_str::<OnDisk>(&text) else { return Vec::new() };
    parsed.platforms
}

fn save_file(path: &Path, entries: &[PlatformEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| ToriiError::InvalidConfig(format!("mkdir {}: {}", parent.display(), e)))?;
    }
    let on_disk = OnDisk { platforms: entries.to_vec() };
    let text = toml::to_string_pretty(&on_disk)
        .map_err(|e| ToriiError::InvalidConfig(format!("serialise platforms.toml: {}", e)))?;
    fs::write(path, text)
        .map_err(|e| ToriiError::InvalidConfig(format!("write {}: {}", path.display(), e)))?;
    Ok(())
}

/// Read the global registry only.
pub fn load_global() -> Vec<PlatformEntry> {
    global_path().map(|p| load_file(&p)).unwrap_or_default()
}

/// Read the per-repo registry only.
pub fn load_local<P: AsRef<Path>>(repo_path: P) -> Vec<PlatformEntry> {
    load_file(&local_path(repo_path))
}

/// Merge per-repo over global by `name`. Returns the effective
/// registry torii's detection code reads from. Local entries win;
/// global entries that aren't shadowed survive.
pub fn merged<P: AsRef<Path>>(repo_path: P) -> Vec<PlatformEntry> {
    let mut by_name: BTreeMap<String, PlatformEntry> = BTreeMap::new();
    for e in load_global()       { by_name.insert(e.name.clone(), e); }
    for e in load_local(repo_path){ by_name.insert(e.name.clone(), e); }
    by_name.into_values().collect()
}

/// Builtins. Same domains the CLI has always recognised, exposed
/// here so `platforms list` can show them alongside customs. Edit
/// only when you add a new SaaS platform — self-hosted instances
/// belong in platforms.toml.
pub fn builtins() -> Vec<PlatformEntry> {
    vec![
        PlatformEntry {
            name: "github.com".into(),
            kind: "github".into(),
            domain: "github.com".into(),
            api_base_url: "https://api.github.com".into(),
            web_base_url: "https://github.com".into(),
            client_id: None,
        },
        PlatformEntry {
            name: "gitlab.com".into(),
            kind: "gitlab".into(),
            domain: "gitlab.com".into(),
            api_base_url: "https://gitlab.com/api/v4".into(),
            web_base_url: "https://gitlab.com".into(),
            client_id: None,
        },
        PlatformEntry {
            name: "codeberg.org".into(),
            kind: "codeberg".into(),
            domain: "codeberg.org".into(),
            api_base_url: "https://codeberg.org/api/v1".into(),
            web_base_url: "https://codeberg.org".into(),
            client_id: None,
        },
        PlatformEntry {
            name: "bitbucket.org".into(),
            kind: "bitbucket".into(),
            domain: "bitbucket.org".into(),
            api_base_url: "https://api.bitbucket.org/2.0".into(),
            web_base_url: "https://bitbucket.org".into(),
            client_id: None,
        },
    ]
}

/// All known platforms (builtins + merged). User entries with the
/// same name shadow builtins. Order: shadowed-out builtins drop,
/// custom entries first, surviving builtins last.
pub fn all<P: AsRef<Path>>(repo_path: P) -> Vec<PlatformEntry> {
    let user = merged(repo_path);
    let user_names: std::collections::BTreeSet<&str> =
        user.iter().map(|e| e.name.as_str()).collect();
    let mut out: Vec<PlatformEntry> = user.clone();
    for b in builtins() {
        if !user_names.contains(b.name.as_str()) {
            out.push(b);
        }
    }
    out
}

/// Resolve a remote URL host to the matching platform entry. Tries
/// custom first (most specific by domain length so subdomain
/// matches don't shadow longer matches), then builtins.
pub fn find_by_host<P: AsRef<Path>>(repo_path: P, host: &str) -> Option<PlatformEntry> {
    let mut candidates = all(repo_path);
    // Prefer longest-matching domain. Lets "gitea.work.io" beat a
    // catch-all "work.io" entry.
    candidates.sort_by_key(|e| std::cmp::Reverse(e.domain.len()));
    candidates.into_iter().find(|e| host == e.domain || host.ends_with(&format!(".{}", e.domain)))
}

/// Add an entry to the per-repo registry when `local`, otherwise
/// the global one. Replaces an existing entry with the same `name`.
pub fn add_entry<P: AsRef<Path>>(repo_path: P, entry: PlatformEntry, local: bool) -> Result<()> {
    let path = if local { local_path(&repo_path) } else { global_path()
        .ok_or_else(|| ToriiError::InvalidConfig("no config dir".into()))? };
    let mut entries = load_file(&path);
    entries.retain(|e| e.name != entry.name);
    entries.push(entry);
    save_file(&path, &entries)
}

/// Remove the entry whose `name` matches. Errors if the registry
/// doesn't carry one with that name (Ok(false)) — the caller can
/// still fall through to "maybe it was a builtin, those can't be
/// removed".
pub fn remove_entry<P: AsRef<Path>>(repo_path: P, name: &str, local: bool) -> Result<bool> {
    let path = if local { local_path(&repo_path) } else { global_path()
        .ok_or_else(|| ToriiError::InvalidConfig("no config dir".into()))? };
    let mut entries = load_file(&path);
    let before = entries.len();
    entries.retain(|e| e.name != name);
    if entries.len() == before {
        return Ok(false);
    }
    save_file(&path, &entries)?;
    Ok(true)
}
