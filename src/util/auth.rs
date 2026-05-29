//! Credential storage for torii — both the gitorii.com cloud API key
//! and the per-platform tokens (GitHub, GitLab, Gitea, Forgejo,
//! Codeberg, crates.io) that the HTTPS transport and platform APIs use.
//!
//! Storage layout (`~/.config/torii/auth.toml`, chmod 600 on Unix):
//!
//! ```toml
//! [cloud]
//! key = "gitorii_sk_…"
//! endpoint = "https://api.gitorii.com"
//!
//! [tokens]
//! github = "ghp_…"
//! gitlab = "glpat-…"
//! gitea = "…"
//! forgejo = "…"
//! codeberg = "…"
//! cargo = "cio_…"
//! ```
//!
//! For backwards compatibility we also read the legacy formats:
//!
//! - `auth.toml` with `key = …` / `endpoint = …` at the top level (the
//!   pre-0.7.1 cloud-only format) — auto-rewrites to the new sectioned
//!   format on the next save.
//! - `config.toml`'s `[auth]` block (where platform tokens used to live)
//!   — also auto-migrated to `auth.toml [tokens]` on the next mutating
//!   call.
//!
//! Token precedence (resolved by [`resolve_token`]):
//!
//! 1. Provider-specific env var (`GITHUB_TOKEN`/`GH_TOKEN`,
//!    `GITLAB_TOKEN`, `CARGO_REGISTRY_TOKEN`, …)
//! 2. Generic env var `TORII_HTTPS_TOKEN`
//! 3. Local repo config (`<repo>/.torii/auth.toml`, same schema as
//!    global)
//! 4. Global config (`~/.config/torii/auth.toml`)
//!
//! Env var `TORII_API_KEY` overrides the cloud key the same way.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Result, ToriiError};

const CLOUD_ENV_VAR: &str = "TORII_API_KEY";
const FILE_NAME: &str = "auth.toml";

// -- Public types -----------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ApiKey {
    pub key: String,
    pub endpoint: String,
}

/// Every credential torii knows about, in one struct. `tokens` is a
/// map rather than fixed fields so `auth.toml` can keep older or
/// newer entries without parser breakage when we add providers.
#[derive(Debug, Clone, Default)]
pub struct AuthStore {
    pub cloud: Option<ApiKey>,
    pub tokens: BTreeMap<String, String>,
}

/// Recognised provider names. The CLI accepts these; readers ask by
/// the same string. Add new entries here only — every other module
/// looks them up by name.
pub const PROVIDERS: &[&str] = &[
    "github",
    "gitlab",
    "gitea",
    "forgejo",
    "codeberg",
    "bitbucket",
    "sourcehut",
    "azure",      // 0.7.18 — Azure DevOps Repos / Pipelines / Releases / Artifacts
    "cargo",
];

// -- Default endpoint for cloud --------------------------------------------

pub fn default_endpoint() -> String {
    std::env::var("TORII_API_ENDPOINT")
        .unwrap_or_else(|_| "https://api.gitorii.com".to_string())
}

// -- Paths -----------------------------------------------------------------

fn global_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("torii").join(FILE_NAME))
}

fn local_path<P: AsRef<Path>>(repo_path: P) -> PathBuf {
    repo_path.as_ref().join(".torii").join(FILE_NAME)
}

// -- Load -------------------------------------------------------------------

/// Read the cloud API key (env wins).
pub fn load() -> Option<ApiKey> {
    if let Ok(env_key) = std::env::var(CLOUD_ENV_VAR) {
        if !env_key.is_empty() {
            return Some(ApiKey {
                key: env_key,
                endpoint: default_endpoint(),
            });
        }
    }
    load_global().cloud
}

/// Read the whole global store from disk (no env override applied —
/// that's [`load`] / [`resolve_token`]'s job).
pub fn load_global() -> AuthStore {
    let Some(path) = global_path() else {
        return AuthStore::default();
    };
    if !path.exists() {
        // Fallback: try migrating the legacy `[auth]` block from
        // `config.toml` so the user doesn't lose tokens after upgrade.
        return migrate_from_config_toml().unwrap_or_default();
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return AuthStore::default(),
    };
    parse(&text)
}

/// Read a local (per-repo) store. Returns empty if the repo has no
/// `.torii/auth.toml`. **Never** falls back to global — that's the
/// merge step's job ([`resolve_token`]).
pub fn load_local_raw<P: AsRef<Path>>(repo_path: P) -> AuthStore {
    let path = local_path(repo_path);
    if !path.exists() {
        return AuthStore::default();
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return AuthStore::default(),
    };
    parse(&text)
}

// -- Save -------------------------------------------------------------------

/// Persist a store to disk with chmod 600 on Unix.
fn save_to(path: &Path, store: &AuthStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| ToriiError::InvalidConfig(format!("create dir: {}", e)))?;
    }
    let mut out = String::new();
    out.push_str("# torii credentials — managed by 'torii auth …'. Do not share.\n\n");
    if let Some(cloud) = &store.cloud {
        out.push_str("[cloud]\n");
        out.push_str(&format!("key = \"{}\"\n", cloud.key));
        out.push_str(&format!("endpoint = \"{}\"\n\n", cloud.endpoint));
    }
    if !store.tokens.is_empty() {
        out.push_str("[tokens]\n");
        for (k, v) in &store.tokens {
            out.push_str(&format!("{} = \"{}\"\n", k, v));
        }
    }
    fs::write(path, out)
        .map_err(|e| ToriiError::InvalidConfig(format!("write {}: {}", path.display(), e)))?;
    restrict_permissions(path);
    Ok(())
}

pub fn save_global(store: &AuthStore) -> Result<()> {
    let path = global_path()
        .ok_or_else(|| ToriiError::InvalidConfig("could not resolve config dir".to_string()))?;
    save_to(&path, store)
}

pub fn save_local<P: AsRef<Path>>(repo_path: P, store: &AuthStore) -> Result<()> {
    let path = local_path(repo_path);
    save_to(&path, store)
}

/// Persist a cloud key — kept as a back-compat shim for `torii auth login`
/// callers that don't know about the wider store yet.
pub fn save_cloud(key: &str, endpoint: &str) -> Result<()> {
    let mut store = load_global();
    store.cloud = Some(ApiKey {
        key: key.to_string(),
        endpoint: endpoint.to_string(),
    });
    save_global(&store)
}

/// Delete just the cloud entry (preserves platform tokens).
pub fn delete() -> Result<()> {
    let mut store = load_global();
    store.cloud = None;
    if store.tokens.is_empty() {
        // Whole file was just the cloud key — remove it entirely.
        if let Some(path) = global_path() {
            if path.exists() {
                fs::remove_file(&path).map_err(|e| {
                    ToriiError::InvalidConfig(format!("remove {}: {}", path.display(), e))
                })?;
            }
        }
        return Ok(());
    }
    save_global(&store)
}

// -- Token mutation ---------------------------------------------------------

/// Validate the provider name against the known list. Case-insensitive
/// match; returns the canonical lowercase form.
pub fn normalise_provider(name: &str) -> Result<String> {
    let lc = name.to_lowercase();
    if PROVIDERS.iter().any(|p| **p == lc) {
        Ok(lc)
    } else {
        Err(ToriiError::InvalidConfig(format!(
            "unknown provider '{}'. Known: {}",
            name,
            PROVIDERS.join(", ")
        )))
    }
}

pub fn set_token(provider: &str, token: &str, local: Option<&Path>) -> Result<()> {
    let provider = normalise_provider(provider)?;
    let result = if let Some(repo) = local {
        let mut store = load_local_raw(repo);
        store.tokens.insert(provider, token.to_string());
        save_local(repo, &store)
    } else {
        let mut store = load_global();
        store.tokens.insert(provider, token.to_string());
        save_global(&store)
    };
    // Invalidate the resolve_token cache so the next call picks up
    // the new value instead of returning the pre-set state.
    invalidate_token_cache();
    result
}

pub fn remove_token(provider: &str, local: Option<&Path>) -> Result<bool> {
    let provider = normalise_provider(provider)?;
    let removed = if let Some(repo) = local {
        let mut store = load_local_raw(repo);
        let r = store.tokens.remove(&provider).is_some();
        save_local(repo, &store)?;
        r
    } else {
        let mut store = load_global();
        let r = store.tokens.remove(&provider).is_some();
        save_global(&store)?;
        r
    };
    invalidate_token_cache();
    Ok(removed)
}

// -- The big one: token resolution -----------------------------------------

/// Where a token came from, surfaced by `torii auth doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    EnvVar(&'static str),
    EnvGeneric,
    Local,
    Global,
    Missing,
}

#[derive(Debug, Clone)]
pub struct ResolvedToken {
    /// Provider this token resolves for. Currently informational only —
    /// kept on the public type so consumers (CLI doctor output, future
    /// audit logs) can use it without forcing a recompile.
    #[allow(dead_code)]
    pub provider: String,
    pub value: Option<String>,
    pub source: TokenSource,
}

/// Look up a token using the documented precedence:
/// env-per-host > env generic > local config > global config > none.
///
// In-process cache for resolve_token results. Many CLI flows resolve
// the same token N times (workspace status across M repos × P
// platforms = N*M*P file reads). Caching is safe because a single
// `torii ...` invocation is short-lived and config rarely changes
// mid-run; the few mutating commands (`set_token`, `remove_token`)
// invalidate the cache below.
fn token_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, ResolvedToken>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, ResolvedToken>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn invalidate_token_cache() {
    if let Ok(mut g) = token_cache().lock() {
        g.clear();
    }
}

/// `repo_path` is the path to the repo (`.` is usually fine); pass it
/// even when you don't expect a local override, the local lookup is
/// cheap when the file doesn't exist.
///
/// Results are cached in-process per `(provider, repo_path)` to avoid
/// re-reading the global / local TOML on every platform-client
/// construction. `set_token` and `remove_token` invalidate the cache.
pub fn resolve_token<P: AsRef<Path>>(provider: &str, repo_path: P) -> ResolvedToken {
    let key = format!("{}|{}", provider.to_lowercase(), repo_path.as_ref().display());
    if let Ok(g) = token_cache().lock() {
        if let Some(hit) = g.get(&key) {
            return hit.clone();
        }
    }
    let result = resolve_token_uncached(provider, repo_path);
    if let Ok(mut g) = token_cache().lock() {
        g.insert(key, result.clone());
    }
    result
}

fn resolve_token_uncached<P: AsRef<Path>>(provider: &str, repo_path: P) -> ResolvedToken {
    let provider_lc = provider.to_lowercase();

    // 1. Per-provider env vars.
    for env_name in env_vars_for(&provider_lc) {
        if let Ok(v) = std::env::var(env_name) {
            if !v.is_empty() {
                return ResolvedToken {
                    provider: provider_lc,
                    value: Some(v),
                    source: TokenSource::EnvVar(env_name),
                };
            }
        }
    }

    // 2. Generic env var (TORII_HTTPS_TOKEN) — matches existing transport.
    if let Ok(v) = std::env::var("TORII_HTTPS_TOKEN") {
        if !v.is_empty() {
            return ResolvedToken {
                provider: provider_lc,
                value: Some(v),
                source: TokenSource::EnvGeneric,
            };
        }
    }

    // 3. Local (per-repo) store.
    let local = load_local_raw(repo_path);
    if let Some(v) = local.tokens.get(&provider_lc) {
        if !v.is_empty() {
            return ResolvedToken {
                provider: provider_lc,
                value: Some(v.clone()),
                source: TokenSource::Local,
            };
        }
    }

    // 4. Global store.
    let global = load_global();
    if let Some(v) = global.tokens.get(&provider_lc) {
        if !v.is_empty() {
            return ResolvedToken {
                provider: provider_lc,
                value: Some(v.clone()),
                source: TokenSource::Global,
            };
        }
    }

    ResolvedToken {
        provider: provider_lc,
        value: None,
        source: TokenSource::Missing,
    }
}

/// Env var names checked for each provider, in order. Order matters
/// because `gh` and GitHub's own CI use `GH_TOKEN` interchangeably with
/// `GITHUB_TOKEN`; we accept both.
fn env_vars_for(provider: &str) -> &'static [&'static str] {
    match provider {
        "github" => &["GITHUB_TOKEN", "GH_TOKEN"],
        "gitlab" => &["GITLAB_TOKEN", "GL_TOKEN"],
        "gitea" => &["GITEA_TOKEN"],
        "forgejo" => &["FORGEJO_TOKEN"],
        "codeberg" => &["CODEBERG_TOKEN"],
        "bitbucket" => &["BITBUCKET_TOKEN"],
        "azure"     => &["AZURE_DEVOPS_TOKEN", "AZURE_DEVOPS_EXT_PAT", "AZDO_TOKEN"],
        "sourcehut" => &["SOURCEHUT_TOKEN", "SRHT_TOKEN"],
        "cargo" => &["CARGO_REGISTRY_TOKEN"],
        _ => &[],
    }
}

// -- Parser -----------------------------------------------------------------

/// Parse the on-disk format. Accepts both the new sectioned form and
/// the pre-0.7.1 cloud-only form (bare `key = …`/`endpoint = …`).
fn parse(text: &str) -> AuthStore {
    enum Section {
        TopLevel,
        Cloud,
        Tokens,
    }
    let mut section = Section::TopLevel;
    let mut cloud_key = String::new();
    let mut cloud_endpoint = default_endpoint();
    let mut have_cloud = false;
    let mut tokens = BTreeMap::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = &line[1..line.len() - 1];
            section = match name.trim() {
                "cloud" => Section::Cloud,
                "tokens" => Section::Tokens,
                _ => Section::TopLevel, // unknown section: tolerate, ignore lines
            };
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim().trim_matches('"').to_string();
        match section {
            Section::Cloud | Section::TopLevel => match k {
                "key" => {
                    cloud_key = v;
                    have_cloud = true;
                }
                "endpoint" => {
                    cloud_endpoint = v;
                }
                _ => {}
            },
            Section::Tokens => {
                if !v.is_empty() {
                    tokens.insert(k.to_string(), v);
                }
            }
        }
    }

    AuthStore {
        cloud: if have_cloud && !cloud_key.is_empty() {
            Some(ApiKey {
                key: cloud_key,
                endpoint: cloud_endpoint,
            })
        } else {
            None
        },
        tokens,
    }
}

// -- Migration --------------------------------------------------------------

/// Back-compat: read `~/.config/torii/config.toml`, pull `[auth]` out of
/// it into an `AuthStore`, and on success write it into `auth.toml` (so
/// next time we use the canonical location). Idempotent — silently
/// returns None when there's nothing to migrate.
fn migrate_from_config_toml() -> Option<AuthStore> {
    let config_path = dirs::config_dir()?.join("torii").join("config.toml");
    if !config_path.exists() {
        return None;
    }
    let text = fs::read_to_string(&config_path).ok()?;

    let mut tokens = BTreeMap::new();
    let mut in_auth = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_auth = line.trim_start_matches('[').trim_end_matches(']').trim() == "auth";
            continue;
        }
        if !in_auth {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let value = v.trim().trim_matches('"').to_string();
        if value.is_empty() {
            continue;
        }
        // Field name in config.toml was `<provider>_token`; in auth.toml
        // we drop the `_token` suffix to match the CLI argument.
        if let Some(provider) = key.strip_suffix("_token") {
            tokens.insert(provider.to_string(), value);
        }
    }
    if tokens.is_empty() {
        return None;
    }
    let store = AuthStore {
        cloud: None,
        tokens,
    };
    let _ = save_global(&store);
    Some(store)
}

// -- Unix permissions -------------------------------------------------------

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_: &std::path::Path) {}

// -- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_legacy_top_level_cloud() {
        let s = parse("key = \"gitorii_sk_abc\"");
        assert_eq!(s.cloud.as_ref().unwrap().key, "gitorii_sk_abc");
        assert!(s.tokens.is_empty());
    }

    #[test]
    fn parse_new_sectioned_cloud_only() {
        let s = parse("[cloud]\nkey = \"x\"\nendpoint = \"http://h\"\n");
        let c = s.cloud.unwrap();
        assert_eq!(c.key, "x");
        assert_eq!(c.endpoint, "http://h");
    }

    #[test]
    fn parse_tokens_only() {
        let s = parse("[tokens]\ngithub = \"ghp_x\"\ngitlab = \"glp_y\"\n");
        assert_eq!(s.tokens["github"], "ghp_x");
        assert_eq!(s.tokens["gitlab"], "glp_y");
        assert!(s.cloud.is_none());
    }

    #[test]
    fn parse_both_sections() {
        let s = parse("[cloud]\nkey = \"k\"\n[tokens]\ncargo = \"cio\"\n");
        assert_eq!(s.cloud.unwrap().key, "k");
        assert_eq!(s.tokens["cargo"], "cio");
    }

    #[test]
    fn parse_empty_tokens_are_dropped() {
        let s = parse("[tokens]\ngithub = \"\"\ngitlab = \"x\"\n");
        assert!(!s.tokens.contains_key("github"));
        assert!(s.tokens.contains_key("gitlab"));
    }

    #[test]
    fn normalise_provider_accepts_known() {
        assert_eq!(normalise_provider("GitHub").unwrap(), "github");
        assert_eq!(normalise_provider("cargo").unwrap(), "cargo");
    }

    #[test]
    fn normalise_provider_rejects_unknown() {
        assert!(normalise_provider("hackernews").is_err());
    }
}
