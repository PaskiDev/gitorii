//! `torii mirror` — multi-platform mirrors.

use crate::duration::parse_duration;
use crate::mirror::{AccountType, MirrorManager, Protocol};
use crate::ssh::SshHelper;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum MirrorCommands {
    /// Add a mirror (replica by default; use --primary for the source of truth)
    Add {
        /// Platform (github, gitlab, bitbucket, codeberg)
        platform: String,

        /// Account type (user or org)
        account_type: String,

        /// Account name (username or organization)
        account: String,

        /// Repository name
        repo: String,

        /// Mark this mirror as the primary (source of truth). Default: replica.
        #[arg(long)]
        primary: bool,

        /// Protocol (ssh or https, defaults to ssh)
        #[arg(short, long)]
        protocol: Option<String>,
    },

    /// List all mirrors
    List,

    /// Sync to all replica mirrors
    Sync {
        /// Force sync
        #[arg(short, long)]
        force: bool,
    },

    /// Promote a mirror to primary (source of truth)
    Promote {
        /// Platform
        platform: String,

        /// Account name
        account: String,
    },

    /// Remove a mirror
    Remove {
        /// Platform
        platform: String,

        /// Account name
        account: String,
    },

    /// Configure autofetch (automatic fetch from mirrors)
    Autofetch {
        /// Enable autofetch
        #[arg(long)]
        enable: bool,

        /// Disable autofetch
        #[arg(long, conflicts_with = "enable")]
        disable: bool,

        /// Fetch interval (e.g., 10m, 30s, 2h, 1d)
        #[arg(long)]
        interval: Option<String>,

        /// Show current autofetch status
        #[arg(long, conflicts_with_all = ["enable", "disable", "interval"])]
        status: bool,
    },
}

fn parse_account_type(s: &str) -> Result<AccountType> {
    match s.to_lowercase().as_str() {
        "user" | "u" => Ok(AccountType::User),
        "org" | "organization" | "o" => Ok(AccountType::Organization),
        _ => Err(anyhow::anyhow!("Invalid account type. Use 'user' or 'org'")),
    }
}

fn parse_protocol(s: Option<&String>) -> Protocol {
    match s.map(|s| s.to_lowercase()) {
        Some(p) if p == "https" || p == "http" => Protocol::HTTPS,
        Some(p) if p == "ssh" => Protocol::SSH,
        None => {
            // Auto-detect: use SSH if keys available, otherwise HTTPS
            if SshHelper::has_ssh_keys() {
                Protocol::SSH
            } else {
                println!("⚠️  No SSH keys detected. Using HTTPS protocol.");
                println!("   Run 'torii config check-ssh' for SSH setup instructions.\n");
                Protocol::HTTPS
            }
        }
        _ => Protocol::SSH,
    }
}

pub(crate) fn run(action: &MirrorCommands) -> Result<()> {
    let mirror_mgr = MirrorManager::new(".")?;
    match action {
        MirrorCommands::Add {
            platform,
            account_type,
            account,
            repo,
            primary,
            protocol,
        } => {
            let acc_type = parse_account_type(account_type)?;
            let proto = parse_protocol(protocol.as_ref());
            mirror_mgr.add_mirror(platform, acc_type, account, repo, proto, *primary)?;
            let kind = if *primary { "Primary" } else { "Replica" };
            println!(
                "✅ {} mirror added: {}/{} on {}",
                kind, account, repo, platform
            );
        }
        MirrorCommands::List => {
            mirror_mgr.list_mirrors()?;
        }
        MirrorCommands::Sync { force } => {
            mirror_mgr.sync_all(*force)?;
        }
        MirrorCommands::Promote { platform, account } => {
            mirror_mgr.set_primary(platform, account)?;
            println!("✅ Promoted to primary: {}/{}", platform, account);
        }
        MirrorCommands::Remove { platform, account } => {
            mirror_mgr.remove_mirror_by_account(platform, account)?;
            println!("✅ Mirror removed: {}/{}", platform, account);
        }
        MirrorCommands::Autofetch {
            enable,
            disable,
            interval,
            status,
        } => {
            if *status {
                mirror_mgr.show_autofetch_status()?;
            } else if *enable {
                let interval_minutes = if let Some(interval_str) = interval {
                    Some(parse_duration(interval_str)?)
                } else {
                    None
                };
                mirror_mgr.configure_autofetch(true, interval_minutes)?;
            } else if *disable {
                mirror_mgr.configure_autofetch(false, None)?;
            } else {
                mirror_mgr.show_autofetch_status()?;
            }
        }
    }
    Ok(())
}
