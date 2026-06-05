//! `torii platforms` — self-hosted platform registry.

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum PlatformsCommands {
    /// Show every known platform (builtins + custom). Custom entries
    /// are marked with a `*`.
    List,
    /// Add or overwrite a platform entry.
    Add {
        /// Short identifier. Reused everywhere else.
        name: String,
        /// Implementation kind. Accepted: `gitlab`, `gitea`,
        /// `forgejo`, `codeberg`, `github`, `github_enterprise`,
        /// `bitbucket`, `bitbucket_data_center`.
        #[arg(long)]
        kind: String,
        /// Host the remote URLs use. Without scheme. e.g.
        /// `gitlab.work.io`.
        #[arg(long)]
        domain: String,
        /// API base URL. e.g. `https://gitlab.work.io/api/v4` for
        /// GitLab, `https://ghe.work.io/api/v3` for GitHub Enterprise.
        #[arg(long = "api")]
        api_base_url: String,
        /// Web base URL — what we show users and where OAuth lands.
        /// e.g. `https://gitlab.work.io`.
        #[arg(long = "web")]
        web_base_url: String,
        /// OAuth client_id override for self-hosted GitLab / Gitea
        /// instances that registered their own OAuth app.
        #[arg(long)]
        client_id: Option<String>,
        /// Write into the per-repo `.torii/platforms.toml` instead
        /// of the global registry.
        #[arg(long)]
        local: bool,
    },
    /// Remove a custom entry by name. Builtins can't be removed —
    /// shadow them with `add` if you need different URLs.
    Remove {
        name: String,
        #[arg(long)]
        local: bool,
    },
    /// Ping the API root with the stored token to verify the entry
    /// is reachable. Exit code 0 on a 2xx; 1 otherwise.
    Test { name: String },
}

pub(crate) fn run(action: &PlatformsCommands) -> Result<()> {
    use crate::platforms_registry as reg;
    match action {
        PlatformsCommands::List => {
            let user = reg::merged(".");
            let user_names: std::collections::BTreeSet<&str> =
                user.iter().map(|e| e.name.as_str()).collect();
            println!("{:<22} {:<22} {:<26} {}", "NAME", "KIND", "DOMAIN", "API");
            for e in &user {
                println!(
                    "* {:<20} {:<22} {:<26} {}",
                    e.name, e.kind, e.domain, e.api_base_url
                );
            }
            for b in reg::builtins() {
                if user_names.contains(b.name.as_str()) {
                    continue;
                }
                println!(
                    "  {:<20} {:<22} {:<26} {}",
                    b.name, b.kind, b.domain, b.api_base_url
                );
            }
            println!();
            println!(
                "(* = custom · `torii platforms add` to add; `torii platforms remove` to drop)"
            );
        }
        PlatformsCommands::Add {
            name,
            kind,
            domain,
            api_base_url,
            web_base_url,
            client_id,
            local,
        } => {
            const KINDS: &[&str] = &[
                "gitlab",
                "gitea",
                "forgejo",
                "codeberg",
                "github",
                "github_enterprise",
                "bitbucket",
                "bitbucket_data_center",
            ];
            if !KINDS.contains(&kind.as_str()) {
                anyhow::bail!("unknown kind `{}`. Known: {}", kind, KINDS.join(", "));
            }
            let entry = reg::PlatformEntry {
                name: name.clone(),
                kind: kind.clone(),
                domain: domain.clone(),
                api_base_url: api_base_url.clone(),
                web_base_url: web_base_url.clone(),
                client_id: client_id.clone(),
            };
            reg::add_entry(".", entry, *local)?;
            let scope = if *local { "local" } else { "global" };
            println!("✓ platform `{}` saved ({} registry)", name, scope);
        }
        PlatformsCommands::Remove { name, local } => {
            let removed = reg::remove_entry(".", name, *local)?;
            if removed {
                println!("✓ removed `{}`", name);
            } else {
                // Tell the user whether it was a builtin
                // they were trying to drop.
                if reg::builtins().iter().any(|b| &b.name == name) {
                    println!("(`{}` is a builtin — shadow it with `torii platforms add` if you need different URLs.)", name);
                } else {
                    println!(
                        "(no platform named `{}` in the {} registry)",
                        name,
                        if *local { "local" } else { "global" }
                    );
                }
            }
        }
        PlatformsCommands::Test { name } => {
            // Find the entry (custom > builtin).
            let entry = reg::all(".")
                .into_iter()
                .find(|e| &e.name == name)
                .ok_or_else(|| {
                    anyhow::anyhow!("no platform named `{}`. Run `torii platforms list`.", name)
                })?;
            // Resolve the provider key for the auth token.
            let provider = match entry.kind.as_str() {
                "github" | "github_enterprise" => "github",
                "gitlab" => "gitlab",
                "gitea" | "forgejo" | "codeberg" => "gitea",
                "bitbucket" | "bitbucket_data_center" => "bitbucket",
                _ => &entry.kind,
            };
            let token = crate::auth::resolve_token(provider, ".")
                .value
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "no `{}` token stored. Run `torii auth set {} <TOKEN>` first.",
                        provider,
                        provider
                    )
                })?;
            let client = crate::http::make_client();
            let mut req = client.get(&entry.api_base_url);
            req = match entry.kind.as_str() {
                "github" | "github_enterprise" => req
                    .header("Authorization", format!("token {}", token))
                    .header("Accept", "application/vnd.github+json"),
                _ => req.header("Authorization", format!("Bearer {}", token)),
            };
            let resp = req
                .send()
                .map_err(|e| anyhow::anyhow!("ping {}: {}", entry.api_base_url, e))?;
            let status = resp.status().as_u16();
            if (200..300).contains(&status) {
                println!(
                    "✓ {} → {} {}",
                    entry.api_base_url,
                    status,
                    resp.status().canonical_reason().unwrap_or("")
                );
            } else {
                let body = resp.text().unwrap_or_default();
                anyhow::bail!(
                    "✗ {} → {}: {}",
                    entry.api_base_url,
                    status,
                    body.chars().take(200).collect::<String>()
                );
            }
        }
    }
    Ok(())
}
