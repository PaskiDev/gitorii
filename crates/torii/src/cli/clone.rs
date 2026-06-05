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
    false
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

        let (ssh_host, https_host) = match platform.as_str() {
                        "github"    => ("github.com", "github.com"),
                        "gitlab"    => ("gitlab.com", "gitlab.com"),
                        "codeberg"  => ("codeberg.org", "codeberg.org"),
                        "bitbucket" => ("bitbucket.org", "bitbucket.org"),
                        "gitea"     => ("gitea.com", "gitea.com"),
                        "forgejo"   => ("codeberg.org", "codeberg.org"),
                        _ => anyhow::bail!(
                            "Unknown platform '{}'. Supported: github, gitlab, codeberg, bitbucket, gitea, forgejo",
                            platform
                        ),
                    };

        if use_ssh {
            format!("git@{}:{}.git", ssh_host, user_repo)
        } else {
            format!("https://{}/{}.git", https_host, user_repo)
        }
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
    GitRepo::clone_repo(&url, target_dir)?;

    let dir_name = target_dir.unwrap_or_else(|| {
        url.split('/')
            .last()
            .unwrap_or("repo")
            .trim_end_matches(".git")
    });
    println!("✅ Cloned repository to: {}", dir_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::looks_like_clone_url;

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
        ] {
            assert!(looks_like_clone_url(s), "should be URL-like: {s}");
        }
    }

    #[test]
    fn platform_shorthands_are_not_urls() {
        for s in ["github", "gitlab", "codeberg", "user/repo", "bitbucket"] {
            assert!(!looks_like_clone_url(s), "should be shorthand: {s}");
        }
    }

    #[test]
    fn scp_like_needs_host_and_path() {
        assert!(!looks_like_clone_url("@host:path")); // empty user
        assert!(!looks_like_clone_url("user@host:")); // empty path
        assert!(!looks_like_clone_url("user@ho/st:x")); // slash in host
    }
}
