//! `torii auth` — cloud key + per-platform tokens.

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum AuthCommands {
    /// Save a gitorii.com API key locally and validate it against the backend.
    Login {
        /// API key (gitorii_sk_…). If omitted, prompts on stdin.
        #[arg(long)]
        key: Option<String>,
        /// Custom API endpoint (default: https://api.gitorii.com).
        /// Useful for self-hosted / local dev.
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Show the org / plan / seats tied to the active gitorii.com key.
    Status,
    /// Alias of `status`.
    Whoami,
    /// Delete the local gitorii.com API key (env var TORII_API_KEY still wins if set).
    Logout,

    /// Save a platform token (github, gitlab, gitea, forgejo, codeberg,
    /// bitbucket, sourcehut, cargo). Goes into `~/.config/torii/auth.toml`
    /// (chmod 600); use `--local` for per-repo `.torii/auth.toml`.
    Set {
        /// Provider name. One of: github, gitlab, gitea, forgejo,
        /// codeberg, bitbucket, sourcehut, cargo.
        provider: String,
        /// Token value. Use `-` to read from stdin (recommended for CI
        /// so the token never lands in shell history).
        token: String,
        /// Save in the per-repo store instead of the global one.
        #[arg(long)]
        local: bool,
        /// Record an expiration: `30d`, `2h`, `7d12h`, etc. Stored in
        /// `~/.config/torii/auth.toml [token_expires]`; `torii auth
        /// doctor` warns when it's close. Pure metadata — torii does
        /// not auto-rotate.
        #[arg(long)]
        ttl: Option<String>,
    },

    /// Print a stored token, value masked (`ghp_xxxx****`).
    Get {
        provider: String,
        /// Show the raw token (you very rarely want this — it goes
        /// straight to stdout / shell history). Off by default.
        #[arg(long)]
        unsafe_show: bool,
    },

    /// Show every provider's token state (set / not set / from env)
    /// with masked values.
    List,

    /// Delete a stored token.
    Remove {
        provider: String,
        /// Remove from per-repo store instead of global.
        #[arg(long)]
        local: bool,
    },

    /// Diagnose where each provider's token comes from (env var name,
    /// local config, global config, or missing). Use this when "torii
    /// auth doesn't seem to use my token".
    Doctor,

    /// Run an OAuth device-flow login against a platform and save the
    /// resulting access token to `~/.config/torii/auth.toml`.
    ///
    /// Avoids having to create a Personal Access Token in the
    /// platform's web UI: you authorise torii in your browser, torii
    /// receives the token, done.
    ///
    /// Supported (0.7.20): github, gitlab, codeberg. Bitbucket and
    /// Azure DevOps use Authorization Code flow with a localhost
    /// callback — wired in a future release; for now `torii auth set
    /// bitbucket USERNAME:APP_PASSWORD` is still the path there.
    Oauth {
        /// Provider name. One of: github, gitlab, codeberg.
        provider: String,
        /// Save in the per-repo store instead of the global one.
        #[arg(long)]
        local: bool,
        /// Record an expiration: `30d`, `2h`, `7d12h`, etc. See
        /// `auth set --ttl`.
        #[arg(long)]
        ttl: Option<String>,
    },

    /// Rotate a stored token: obtain a fresh one, replace the saved
    /// value, and best-effort revoke the old one so a leaked copy
    /// stops working immediately.
    ///
    /// Default flow (OAuth): re-runs the device flow, swaps in the new
    /// access token, then POSTs to the platform's revoke endpoint. The
    /// old token stops working as soon as the revoke succeeds.
    ///
    /// `--pat` (GitLab only): uses the native
    /// `POST /personal_access_tokens/self/rotate` endpoint, which
    /// generates a new PAT with the same scopes and invalidates the
    /// old one atomically — no OAuth round-trip, no browser. Requires
    /// the current token to have the `api` scope.
    Rotate {
        /// Provider name. OAuth path: github, gitlab, codeberg.
        /// PAT path (`--pat`): gitlab.
        provider: String,
        /// Rotate the PAT in place via the platform's native rotate
        /// endpoint instead of running OAuth. GitLab only.
        #[arg(long)]
        pat: bool,
        /// Save in the per-repo store instead of the global one.
        #[arg(long)]
        local: bool,
        /// Record an expiration for the new token: `30d`, `2h`, etc.
        #[arg(long)]
        ttl: Option<String>,
    },
}

pub(crate) fn run_auth(action: &AuthCommands) -> Result<()> {
    use crate::auth;
    use crate::cloud::{whoami::whoami, CloudClient};

    match action {
        AuthCommands::Login { key, endpoint } => {
            let key_value = match key {
                Some(k) => k.clone(),
                None => {
                    use std::io::{BufRead, Write};
                    print!("API key (gitorii_sk_…): ");
                    std::io::stdout().flush().ok();
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line)?;
                    line.trim().to_string()
                }
            };
            if !key_value.starts_with("gitorii_sk_") {
                anyhow::bail!("API key must start with `gitorii_sk_`");
            }
            let endpoint = endpoint.clone().unwrap_or_else(auth::default_endpoint);
            // Validate before saving so we don't store a bogus key.
            let client = CloudClient::new(auth::ApiKey {
                key: key_value.clone(),
                endpoint: endpoint.clone(),
            });
            let me = whoami(&client)?;
            auth::save_cloud(&key_value, &endpoint)?;
            println!("✅ Logged in to {}", endpoint);
            println!("   org:  {} ({})", me.org_name, me.org_slug);
            println!("   plan: {}", me.plan);
        }
        AuthCommands::Status | AuthCommands::Whoami => {
            let key = auth::load().ok_or_else(|| {
                anyhow::anyhow!(
                    "no API key configured. Run `torii auth login` or set TORII_API_KEY."
                )
            })?;
            let client = CloudClient::new(key);
            let me = whoami(&client)?;
            println!("endpoint: {}", client.endpoint());
            println!(
                "org:      {} ({}) [{}]",
                me.org_name, me.org_slug, me.org_id
            );
            println!("plan:     {}", me.plan);
            println!("seats:    {}", me.seats);
            if me.suspended {
                println!("status:   ⚠️  suspended");
            }
        }
        AuthCommands::Logout => {
            auth::delete()?;
            println!("✅ Local API key deleted");
            if std::env::var("TORII_API_KEY").is_ok() {
                println!("⚠️  TORII_API_KEY env var still set — unset it to fully log out.");
            }
        }
        AuthCommands::Set {
            provider,
            token,
            local,
            ttl,
        } => {
            let resolved_token = if token == "-" {
                use std::io::{BufRead, Write};
                eprint!("Paste token (input hidden — Ctrl-D to end): ");
                std::io::stderr().flush().ok();
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line)?;
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    anyhow::bail!("Empty token from stdin.");
                }
                trimmed
            } else {
                token.clone()
            };
            let repo: Option<&std::path::Path> = if *local {
                Some(std::path::Path::new("."))
            } else {
                None
            };
            let expires_at = ttl_to_iso(ttl.as_deref())?;
            auth::set_token_with_expiry(provider, &resolved_token, expires_at.as_deref(), repo)?;
            let scope = if *local { "local" } else { "global" };
            println!("✅ {} token saved ({} store).", provider, scope);
            if let Some(iso) = &expires_at {
                println!("   expires: {}", iso);
            }
        }
        AuthCommands::Get {
            provider,
            unsafe_show,
        } => {
            let resolved = auth::resolve_token(provider, ".");
            match (&resolved.value, unsafe_show) {
                (Some(v), true) => println!("{}", v),
                (Some(v), false) => println!("{}", mask_token(v)),
                (None, _) => {
                    println!("(not set for '{}')", provider);
                }
            }
        }
        AuthCommands::List => {
            println!("🔑 Stored tokens:\n");
            for p in auth::PROVIDERS {
                let r = auth::resolve_token(p, ".");
                match r.value.as_deref() {
                    Some(v) => println!(
                        "  {:<10} {:<24} {}",
                        p,
                        mask_token(v),
                        describe_source(&r.source)
                    ),
                    None => println!("  {:<10} —", p),
                }
            }
        }
        AuthCommands::Remove { provider, local } => {
            let repo: Option<&std::path::Path> = if *local {
                Some(std::path::Path::new("."))
            } else {
                None
            };
            let removed = auth::remove_token(provider, repo)?;
            if removed {
                println!("✅ Removed {} token.", provider);
            } else {
                println!("(no {} token was set; nothing to remove)", provider);
            }
        }
        AuthCommands::Oauth {
            provider,
            local,
            ttl,
        } => {
            // Pick the right OAuth flow for the provider. Most platforms
            // support RFC 8628 Device Flow; a few (e.g. Bitbucket) only
            // support Authorization Code Grant and need a localhost
            // loopback server. Both end at the same point: an access
            // token saved under the same provider key.
            let token = if crate::oauth::device_flow_supported(provider) {
                crate::oauth::run_device_flow(provider)?
            } else if crate::oauth::auth_code_flow_supported(provider) {
                crate::oauth::run_auth_code_flow(provider)?
            } else {
                anyhow::bail!(
                    "OAuth flow not configured for `{}`. \
                     Device flow supports: github, gitlab, codeberg. \
                     Auth-code flow supports: bitbucket. \
                     For other providers, create a PAT manually and run `torii auth set {} ...`.",
                    provider,
                    provider
                );
            };
            let repo: Option<&std::path::Path> = if *local {
                Some(std::path::Path::new("."))
            } else {
                None
            };
            let expires_at = ttl_to_iso(ttl.as_deref())?;
            auth::set_token_with_expiry(provider, &token, expires_at.as_deref(), repo)?;
            let scope = if *local { "local" } else { "global" };
            println!("✅ {} token saved ({} store).", provider, scope);
            if let Some(iso) = &expires_at {
                println!("   expires: {}", iso);
            }
        }
        AuthCommands::Rotate {
            provider,
            pat,
            local,
            ttl,
        } => {
            // Snapshot the current token first — once we set the new
            // value the cached one in `auth::resolve_token` becomes
            // the new one, and there's no way to get the old back.
            let old = auth::resolve_token(provider, ".").value.ok_or_else(|| {
                anyhow::anyhow!(
                    "No `{}` token is currently stored. Use `torii auth oauth {}` \
                     or `torii auth set {} <token>` to set one first.",
                    provider,
                    provider,
                    provider
                )
            })?;

            let new_token = if *pat {
                // PAT-native rotate path. GitLab is the only platform
                // that exposes this today; for others, suggest the
                // OAuth path explicitly.
                if provider != "gitlab" {
                    anyhow::bail!(
                        "`--pat` rotate is only implemented for GitLab. \
                         For `{}`, drop `--pat` to use the OAuth flow.",
                        provider
                    );
                }
                println!("🔁 Rotating GitLab PAT via /personal_access_tokens/self/rotate…");
                crate::oauth::rotate_gitlab_pat(&old)?
            } else {
                // OAuth path: run the flow, then revoke the old token.
                println!(
                    "🔁 Rotating {} token via OAuth — re-authorise in your browser.\n",
                    provider
                );
                let t = if crate::oauth::device_flow_supported(provider) {
                    crate::oauth::run_device_flow(provider)?
                } else if crate::oauth::auth_code_flow_supported(provider) {
                    crate::oauth::run_auth_code_flow(provider)?
                } else {
                    anyhow::bail!(
                        "No OAuth flow wired for `{}`. Supported: github, gitlab, codeberg \
                         (device flow), bitbucket (auth-code+PKCE). For `{}` you'd need to \
                         rotate manually in the platform's web UI.",
                        provider,
                        provider
                    );
                };
                t
            };

            let repo: Option<&std::path::Path> = if *local {
                Some(std::path::Path::new("."))
            } else {
                None
            };
            let expires_at = ttl_to_iso(ttl.as_deref())?;
            auth::set_token_with_expiry(provider, &new_token, expires_at.as_deref(), repo)?;
            let scope = if *local { "local" } else { "global" };
            println!("✅ New {} token saved ({} store).", provider, scope);
            if let Some(iso) = &expires_at {
                println!("   expires: {}", iso);
            }

            // Best-effort revoke. PAT rotate already invalidated the
            // old one server-side, so we skip the revoke call there.
            if !*pat {
                match crate::oauth::revoke_token(provider, &old) {
                    Ok(true) => println!("✅ Old {} token revoked.", provider),
                    Ok(false) => {
                        let hint = crate::oauth::revoke_hint_url(provider)
                            .unwrap_or("the platform's settings page");
                        println!(
                            "⚠  No programmatic revoke for `{}` (or no client secret \
                                  available). Revoke the old token manually at:\n     {}",
                            provider, hint
                        );
                    }
                    Err(e) => {
                        let hint = crate::oauth::revoke_hint_url(provider)
                            .unwrap_or("the platform's settings page");
                        println!(
                            "⚠  Revoke failed: {}. Revoke manually at:\n     {}",
                            e, hint
                        );
                    }
                }
            }
        }
        AuthCommands::Doctor => {
            println!("🔍 Token resolution (env > local > global):\n");
            for p in auth::PROVIDERS {
                let r = auth::resolve_token(p, ".");
                let state = match &r.value {
                    Some(_) => "✓ resolved",
                    None => "— missing",
                };
                let source = describe_source(&r.source);
                let expiry = auth::token_expires_at(p)
                    .as_deref()
                    .and_then(describe_expiry)
                    .map(|s| format!("   {}", s))
                    .unwrap_or_default();
                println!("  {:<10} {:<14} {}{}", p, state, source, expiry);
            }
            // Also surface the legacy config.toml [auth] if it lingers.
            if let Some(cd) = dirs::config_dir() {
                let cfg = cd.join("torii").join("config.toml");
                if let Ok(t) = std::fs::read_to_string(&cfg) {
                    let has_legacy = t.lines().any(|l| {
                        let l = l.trim();
                        l == "[auth]" || l.starts_with("[auth]")
                    });
                    if has_legacy {
                        println!(
                            "\n⚠  Legacy [auth] block still present in {}.\n   \
                             Tokens have been migrated to auth.toml — you can delete that section now.",
                            cfg.display()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Render a token as `prefix6_xxxx****suffix4` so screenshots / logs
/// don't leak the secret. Tokens shorter than 12 chars are fully masked.
fn mask_token(t: &str) -> String {
    let chars: Vec<char> = t.chars().collect();
    if chars.len() < 12 {
        return "****".to_string();
    }
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{}…{}", head, tail)
}

fn describe_source(s: &crate::auth::TokenSource) -> String {
    match s {
        crate::auth::TokenSource::EnvVar(name) => format!("from env: ${}", name),
        crate::auth::TokenSource::EnvGeneric => "from env: $TORII_HTTPS_TOKEN".to_string(),
        crate::auth::TokenSource::Local => "local .torii/auth.toml".to_string(),
        crate::auth::TokenSource::Global => "global ~/.config/torii/auth.toml".to_string(),
        crate::auth::TokenSource::Missing => "(not set)".to_string(),
    }
}

/// Resolve a `--ttl <duration>` flag into an ISO-8601 timestamp at which
/// the token is considered expired. Returns `Ok(None)` when no TTL was
/// passed; the caller stores `None` as "no expiration tracked".
fn ttl_to_iso(ttl: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(s) = ttl else { return Ok(None) };
    let mins = crate::duration::parse_duration(s)
        .map_err(|e| anyhow::anyhow!("invalid --ttl `{}`: {}", s, e))?;
    let when = chrono::Utc::now() + chrono::Duration::minutes(mins as i64);
    Ok(Some(
        when.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    ))
}

/// One-line "expires in X" / "expired Y ago" string for the doctor and
/// list output. Returns None when the timestamp is missing or unparsable
/// — callers can decide whether to print nothing or a placeholder.
fn describe_expiry(iso: &str) -> Option<String> {
    let when = chrono::DateTime::parse_from_rfc3339(iso)
        .ok()?
        .with_timezone(&chrono::Utc);
    let now = chrono::Utc::now();
    let delta = when.signed_duration_since(now);
    let days = delta.num_days();
    let hours = delta.num_hours();
    if delta.num_seconds() < 0 {
        let past = -delta;
        if past.num_days() > 0 {
            Some(format!("⛔ expired {}d ago ({})", past.num_days(), iso))
        } else if past.num_hours() > 0 {
            Some(format!("⛔ expired {}h ago ({})", past.num_hours(), iso))
        } else {
            Some(format!("⛔ expired just now ({})", iso))
        }
    } else if days < 7 {
        // Warn band: less than a week. Rotate soon.
        if days >= 1 {
            Some(format!("⚠ expires in {}d ({})", days, iso))
        } else {
            Some(format!("⚠ expires in {}h ({})", hours.max(1), iso))
        }
    } else {
        Some(format!("⏳ expires in {}d ({})", days, iso))
    }
}
