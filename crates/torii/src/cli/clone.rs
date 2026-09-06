//! `torii clone` — clone via platform shorthand or URL.

use crate::config::ToriiConfig;
use crate::core::GitRepo;
use crate::ssh::SshHelper;
use anyhow::Result;

/// True when the string looks like something `git clone` would accept as
/// a URL or local path, distinguishing it from a platform shorthand
/// (`github`, `gitlab`, …) used in `torii clone <plat> <user/repo>`.
///
/// Accepted shapes:
///   http://… https://… git://… ssh://… ftp(s)://… file://…
///   git@host:owner/repo.git           (scp-like SSH)
///   user@host:owner/repo.git          (any scp-like)
///   /absolute/path/to/repo            (Unix abs)
///   ./relative/path  ../sibling       (relative explicit)
///   C:\… or C:/…                      (Windows abs)
///   rad:z…  rad://z…                  (Radicle RID; cloned with `rad`)
fn looks_like_clone_url(s: &str) -> bool {
    // Explicit scheme — anything before `://` and at least one alphanum.
    if let Some(idx) = s.find("://") {
        if idx > 0
            && s[..idx]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        {
            return true;
        }
    }
    // Local paths.
    if s.starts_with('/') || s.starts_with("./") || s.starts_with("../") {
        return true;
    }
    // Windows drive (C:\ or C:/).
    let bytes = s.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
    {
        return true;
    }
    // scp-like: <user>@<host>:<path>. Requires '@' before ':' with both
    // sides non-empty. Excludes IPv6-ish patterns.
    if let Some(at) = s.find('@') {
        if at > 0 {
            if let Some(colon) = s[at + 1..].find(':') {
                let host = &s[at + 1..at + 1 + colon];
                let path = &s[at + 1 + colon + 1..];
                if !host.is_empty()
                    && !path.is_empty()
                    && !host.contains('/')
                    && !host.contains('\\')
                {
                    return true;
                }
            }
        }
    }
    // Radicle URN (`rad:z…`). No `://`, so the scheme check above
    // misses it; libgit2 cannot clone it either.
    if crate::radicle::is_radicle_url(s) {
        return true;
    }
    false
}

/// Host[:port] from an API base URL (`https://git.example.com` →
/// `git.example.com`). Empty or unparseable input yields `None`.
fn host_from_base_url(raw: &str) -> Option<String> {
    let s = raw.trim().trim_end_matches('/');
    if s.is_empty() {
        return None;
    }
    let rest = s.split_once("://").map(|(_, r)| r).unwrap_or(s);
    let rest = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
    let hostport = rest.split('/').next()?.trim();
    if hostport.is_empty() {
        None
    } else {
        Some(hostport.to_string())
    }
}

fn host_from_env(var: &str, default_host: &str) -> String {
    match std::env::var(var) {
        Ok(raw) if !raw.trim().is_empty() => {
            host_from_base_url(&raw).unwrap_or_else(|| default_host.to_string())
        }
        _ => default_host.to_string(),
    }
}

/// scp-like SSH cannot carry a port; strip `:digits` (and `[ipv6]:port`).
fn ssh_host(hostport: &str) -> &str {
    if let Some(end) = hostport.find(']') {
        if end >= 1 {
            return &hostport[1..end];
        }
    }
    match hostport.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => hostport,
    }
}

fn git_clone_url(hostport: &str, user_repo: &str, use_ssh: bool) -> String {
    if use_ssh {
        format!("git@{}:{}.git", ssh_host(hostport), user_repo)
    } else {
        format!("https://{}/{}.git", hostport, user_repo)
    }
}

/// Sourcehut owners are `~user`. Accept both `~user/repo` and `user/repo`.
fn sourcehut_slug(user_repo: &str) -> Result<String> {
    let (owner, repo) = user_repo
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("sourcehut shorthand is ~user/repo, got '{user_repo}'"))?;
    if owner.is_empty() || repo.is_empty() {
        anyhow::bail!("sourcehut shorthand is ~user/repo, got '{user_repo}'");
    }
    let owner = if owner.starts_with('~') {
        owner.to_string()
    } else {
        format!("~{owner}")
    };
    Ok(format!("{owner}/{repo}"))
}

/// Azure clone URLs are three-level: `org/project/repo`.
fn azure_parts(user_repo: &str) -> Result<(String, String, String)> {
    let mut parts: Vec<&str> = user_repo.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 3 {
        anyhow::bail!("Azure shorthand is org/project/repo, got '{user_repo}'");
    }
    let repo = parts.pop().unwrap().to_string();
    let org = parts.remove(0).to_string();
    let project = parts.join("/");
    Ok((org, project, repo))
}

fn clone_url_with(
    platform: &str,
    user_repo: &str,
    use_ssh: bool,
    gitea_host: &str,
    forgejo_host: &str,
) -> Result<String> {
    match platform {
        "github" => Ok(git_clone_url("github.com", user_repo, use_ssh)),
        "gitlab" => Ok(git_clone_url("gitlab.com", user_repo, use_ssh)),
        "codeberg" => Ok(git_clone_url("codeberg.org", user_repo, use_ssh)),
        "bitbucket" => Ok(git_clone_url("bitbucket.org", user_repo, use_ssh)),
        "gitea" => Ok(git_clone_url(gitea_host, user_repo, use_ssh)),
        "forgejo" => Ok(git_clone_url(forgejo_host, user_repo, use_ssh)),
        "sourcehut" => {
            let slug = sourcehut_slug(user_repo)?;
            if use_ssh {
                Ok(format!("git@git.sr.ht:{slug}"))
            } else {
                Ok(format!("https://git.sr.ht/{slug}"))
            }
        }
        "azure" => {
            let (org, project, repo) = azure_parts(user_repo)?;
            if use_ssh {
                Ok(format!("git@ssh.dev.azure.com:v3/{org}/{project}/{repo}"))
            } else {
                Ok(format!(
                    "https://dev.azure.com/{org}/{project}/_git/{repo}"
                ))
            }
        }
        "radicle" => crate::radicle::normalize_rid(user_repo)
            .map_err(|e| anyhow::anyhow!("{e}")),
        _ => anyhow::bail!(
            "Unknown platform '{}'. Supported: github, gitlab, codeberg, bitbucket, gitea, forgejo, sourcehut, azure, radicle",
            platform
        ),
    }
}

fn clone_url_for(platform: &str, user_repo: &str, use_ssh: bool) -> Result<String> {
    clone_url_with(
        platform,
        user_repo,
        use_ssh,
        &host_from_env("GITEA_URL", "gitea.com"),
        &host_from_env("FORGEJO_URL", "codeberg.org"),
    )
}

pub(crate) fn run(
    source: &String,
    args: &[String],
    directory: &Option<String>,
    protocol: &Option<String>,
) -> Result<()> {
    // Match git clone's positional shape:
    //   torii clone <platform> <user/repo> [<path>]
    //   torii clone <url> [<path>]
    // The trailing path arg silently used to be ignored, surprising
    // users coming from `git clone <url> <path>`.
    //
    // Disambiguation: if `source` already looks like a URL/path
    // (http(s)://, git://, ssh://, file://, /abs, ./rel,
    // user@host:path), treat the first positional `args[0]` as
    // the destination — NOT as user/repo. Without this guard,
    // `torii clone file:///tmp/foo dest` errored with
    // "Unknown platform 'file:///tmp/foo'".
    let source_is_url = looks_like_clone_url(source);

    let url = if !args.is_empty() && !source_is_url {
        // Shorthand: torii clone <platform> <user/repo>
        let platform = source;
        let user_repo = &args[0];

        // Protocol priority: --protocol flag > config > auto-detect
        let use_ssh = match protocol.as_deref() {
            Some("https") | Some("http") => false,
            Some("ssh") => true,
            _ => {
                let cfg = ToriiConfig::load_global().unwrap_or_default();
                if cfg.mirror.default_protocol == "https" {
                    false
                } else {
                    SshHelper::has_ssh_keys()
                }
            }
        };

        clone_url_for(platform, user_repo, use_ssh)?
    } else if looks_like_clone_url(source) {
        source.clone()
    } else {
        anyhow::bail!(
                        "Usage:\n  torii clone <platform> <user/repo>        e.g. torii clone github user/repo\n  torii clone <platform> <user/repo> --protocol https\n  torii clone <url>                          e.g. torii clone https://github.com/user/repo.git\n  torii clone <local-path-or-file:///url>    e.g. torii clone /tmp/source.git"
                    )
    };

    // Resolve destination. Precedence:
    //   1. -d / --directory flag
    //   2. trailing positional arg (git-style):
    //        torii clone <plat> <user/repo> <path>   → args[1]
    //        torii clone <url> <path>                → args[0]
    //   3. derive from URL (default)
    let positional_dest: Option<&str> = if source_is_url {
        // URL form: first positional after the URL is the dest.
        args.first().map(|s| s.as_str())
    } else if !args.is_empty() {
        // Shorthand: args[0] is user/repo, args[1] is dest.
        args.get(1).map(|s| s.as_str())
    } else {
        None
    };
    let target_dir = directory.as_deref().or(positional_dest);
    if crate::radicle::is_radicle_url(&url) {
        crate::radicle::rad_clone(&url, target_dir)?;
    } else {
        GitRepo::clone_repo(&url, target_dir)?;
    }

    let dir_name = target_dir.unwrap_or_else(|| {
        url.split('/')
            .next_back()
            .unwrap_or("repo")
            .trim_end_matches(".git")
    });
    println!("✅ Cloned repository to: {}", dir_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        azure_parts, clone_url_with, host_from_base_url, looks_like_clone_url, sourcehut_slug,
        ssh_host,
    };

    #[test]
    fn schemes_and_paths_are_urls() {
        for s in [
            "https://github.com/u/r.git",
            "http://gitlab.com/u/r",
            "git://host/repo.git",
            "ssh://git@host:2222/repo.git",
            "file:///tmp/repo",
            "/abs/path/repo",
            "./relative/repo",
            "../sibling/repo",
            "C:\\repos\\foo",
            "C:/repos/foo",
            "git@github.com:user/repo.git",
            "deploy@10.0.0.5:srv/app.git",
            "git@git.sr.ht:~user/repo",
            "https://git.sr.ht/~user/repo",
            "git@ssh.dev.azure.com:v3/org/proj/repo",
            "https://dev.azure.com/org/proj/_git/repo",
            "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5",
            "rad://z3gqcJUoA1n9HaHKufZs5FCSGazv5",
        ] {
            assert!(looks_like_clone_url(s), "should be URL-like: {s}");
        }
    }

    #[test]
    fn platform_shorthands_are_not_urls() {
        for s in [
            "github",
            "gitlab",
            "codeberg",
            "user/repo",
            "bitbucket",
            "gitea",
            "forgejo",
            "sourcehut",
            "azure",
            "radicle",
        ] {
            assert!(!looks_like_clone_url(s), "should be shorthand: {s}");
        }
    }

    #[test]
    fn scp_like_needs_host_and_path() {
        assert!(!looks_like_clone_url("@host:path")); // empty user
        assert!(!looks_like_clone_url("user@host:")); // empty path
        assert!(!looks_like_clone_url("user@ho/st:x")); // slash in host
    }

    #[test]
    fn host_from_base_url_strips_scheme_and_path() {
        assert_eq!(
            host_from_base_url("https://git.example.com").as_deref(),
            Some("git.example.com")
        );
        assert_eq!(
            host_from_base_url("https://git.example.com/").as_deref(),
            Some("git.example.com")
        );
        assert_eq!(
            host_from_base_url("https://git.example.com:3000/api/v1").as_deref(),
            Some("git.example.com:3000")
        );
        assert_eq!(
            host_from_base_url("git.example.com").as_deref(),
            Some("git.example.com")
        );
        assert_eq!(host_from_base_url("").as_deref(), None);
        assert_eq!(host_from_base_url("   /").as_deref(), None);
    }

    #[test]
    fn ssh_host_drops_a_numeric_port() {
        assert_eq!(ssh_host("git.example.com:3000"), "git.example.com");
        assert_eq!(ssh_host("git.example.com"), "git.example.com");
        assert_eq!(ssh_host("[::1]:3000"), "::1");
    }

    #[test]
    fn sourcehut_slug_adds_the_tilde() {
        assert_eq!(sourcehut_slug("user/repo").unwrap(), "~user/repo");
        assert_eq!(sourcehut_slug("~user/repo").unwrap(), "~user/repo");
        assert!(sourcehut_slug("nopath").is_err());
    }

    #[test]
    fn azure_parts_need_three_levels() {
        let (org, project, repo) = azure_parts("org/proj/repo").unwrap();
        assert_eq!(
            (org, project, repo),
            ("org".into(), "proj".into(), "repo".into())
        );
        let (org, project, repo) = azure_parts("org/team/proj/repo").unwrap();
        assert_eq!(
            (org, project, repo),
            ("org".into(), "team/proj".into(), "repo".into())
        );
        assert!(azure_parts("org/repo").is_err());
        assert!(azure_parts("repo").is_err());
    }

    #[test]
    fn shorthand_urls_match_the_host() {
        let url =
            |plat, slug, ssh| clone_url_with(plat, slug, ssh, "gitea.com", "codeberg.org").unwrap();
        assert_eq!(url("github", "u/r", true), "git@github.com:u/r.git");
        assert_eq!(url("github", "u/r", false), "https://github.com/u/r.git");
        assert_eq!(
            url("sourcehut", "user/repo", true),
            "git@git.sr.ht:~user/repo"
        );
        assert_eq!(
            url("sourcehut", "~user/repo", false),
            "https://git.sr.ht/~user/repo"
        );
        assert_eq!(
            url("azure", "org/proj/widget", true),
            "git@ssh.dev.azure.com:v3/org/proj/widget"
        );
        assert_eq!(
            url("azure", "org/proj/widget", false),
            "https://dev.azure.com/org/proj/_git/widget"
        );
        assert_eq!(url("forgejo", "u/r", false), "https://codeberg.org/u/r.git");
        assert_eq!(
            clone_url_with("forgejo", "u/r", true, "gitea.com", "git.example.com:3000").unwrap(),
            "git@git.example.com:u/r.git"
        );
        assert_eq!(
            clone_url_with("forgejo", "u/r", false, "gitea.com", "git.example.com:3000").unwrap(),
            "https://git.example.com:3000/u/r.git"
        );
        assert_eq!(
            clone_url_with("gitea", "u/r", false, "git.example.com", "codeberg.org").unwrap(),
            "https://git.example.com/u/r.git"
        );
        assert_eq!(
            url("radicle", "z3gqcJUoA1n9HaHKufZs5FCSGazv5", true),
            "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5"
        );
        assert_eq!(
            url("radicle", "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5", false),
            "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5"
        );
        let err = clone_url_with("nope", "u/r", true, "gitea.com", "codeberg.org")
            .unwrap_err()
            .to_string();
        assert!(err.contains("sourcehut"), "{err}");
        assert!(err.contains("azure"), "{err}");
        assert!(err.contains("radicle"), "{err}");
    }
}
