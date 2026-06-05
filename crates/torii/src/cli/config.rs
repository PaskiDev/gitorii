//! `torii config` — configuration management.

use crate::config::ToriiConfig;
use crate::ssh::SshHelper;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum ConfigCommands {
    /// Set a configuration value
    Set {
        /// Configuration key (e.g., user.name, snapshot.auto_enabled)
        key: String,

        /// Configuration value
        value: String,

        /// Set in local repository config instead of global
        #[arg(long)]
        local: bool,
    },

    /// Get a configuration value
    Get {
        /// Configuration key (e.g., user.name, snapshot.auto_enabled)
        key: String,

        /// Get from local repository config
        #[arg(long)]
        local: bool,
    },

    /// List all configuration values
    List {
        /// Show local repository config
        #[arg(long)]
        local: bool,
    },

    /// Edit configuration file in editor
    Edit {
        /// Edit local repository config instead of global
        #[arg(long)]
        local: bool,
    },

    /// Reset configuration to defaults
    Reset {
        /// Reset local repository config instead of global
        #[arg(long)]
        local: bool,
    },

    /// Check SSH configuration and show setup instructions
    #[command(name = "check-ssh")]
    CheckSsh,
}

pub(crate) fn run(action: &ConfigCommands) -> Result<()> {
    match action {
        ConfigCommands::Set { key, value, local } => {
            // Auth tokens migrated to `torii auth` in 0.7.1.
            // Redirect transparently so old scripts keep
            // working but the user is steered to the new home.
            if let Some(provider_token) = key.strip_prefix("auth.") {
                if let Some(provider) = provider_token.strip_suffix("_token") {
                    let repo: Option<&std::path::Path> = if *local {
                        Some(std::path::Path::new("."))
                    } else {
                        None
                    };
                    crate::auth::set_token(provider, value, repo)?;
                    eprintln!(
                                    "⚠  `torii config set auth.{p}_token` is deprecated and will be removed in 0.8.\n   \
                                     Saved via the new path: `torii auth set {p} …` (which is what you want next time).",
                                    p = provider
                                );
                    let scope = if *local { "local" } else { "global" };
                    println!("✅ {} token saved ({} store).", provider, scope);
                    return Ok(());
                }
            }

            if *local {
                let mut config = ToriiConfig::load_local(".")?;
                config.set(key, value)?;
                config.save_local(".")?;
                println!("✅ Local config updated: {} = {}", key, value);
            } else {
                let mut config = ToriiConfig::load_global()?;
                config.set(key, value)?;
                config.save_global()?;
                println!("✅ Global config updated: {} = {}", key, value);
            }
        }
        ConfigCommands::Get { key, local } => {
            let config = if *local {
                ToriiConfig::load_local(".")?
            } else {
                ToriiConfig::load_global()?
            };

            if let Some(value) = config.get(key) {
                println!("{}", value);
            } else {
                println!("❌ Config key not found: {}", key);
            }
        }
        ConfigCommands::List { local } => {
            let config = if *local {
                ToriiConfig::load_local(".")?
            } else {
                ToriiConfig::load_global()?
            };

            let scope = if *local { "Local" } else { "Global" };
            println!("⚙️  {} Configuration:\n", scope);

            for (key, value) in config.list() {
                println!("  {} = {}", key, value);
            }
        }
        ConfigCommands::Edit { local } => {
            let config_path = if *local {
                std::path::PathBuf::from(".")
                    .join(".torii")
                    .join("config.toml")
            } else {
                dirs::config_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
                    .join("torii")
                    .join("config.toml")
            };

            // Ensure config exists
            if *local {
                let config = ToriiConfig::load_local(".")?;
                config.save_local(".")?;
            } else {
                let config = ToriiConfig::load_global()?;
                config.save_global()?;
            }

            // Get editor
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());

            // Open editor
            let status = std::process::Command::new(&editor)
                .arg(&config_path)
                .status()?;

            if status.success() {
                println!("✅ Configuration edited");
            } else {
                println!("❌ Editor exited with error");
            }
        }
        ConfigCommands::Reset { local } => {
            let config = ToriiConfig::default();

            if *local {
                config.save_local(".")?;
                println!("✅ Local configuration reset to defaults");
            } else {
                config.save_global()?;
                println!("✅ Global configuration reset to defaults");
            }
        }
        ConfigCommands::CheckSsh => {
            run_ssh_check();
        }
    }
    Ok(())
}

fn run_ssh_check() {
    println!("🔐 SSH Configuration Check\n");

    if SshHelper::has_ssh_keys() {
        println!("✅ SSH keys found!\n");

        let keys = SshHelper::list_keys();
        if !keys.is_empty() {
            println!("Available keys:");
            for key in &keys {
                println!("  • {}", key);
            }
        }

        println!("\n💡 Recommendation: Use SSH protocol (default)");
    } else {
        println!("❌ No SSH keys found");
        println!("\n💡 To set up SSH keys:");
        println!("   1. Generate a new key:");
        println!("      ssh-keygen -t ed25519 -C \"your_email@example.com\"");
        println!("   2. Start the SSH agent:");
        println!("      eval \"$(ssh-agent -s)\"");
        println!("   3. Add your key:");
        println!("      ssh-add ~/.ssh/id_ed25519");
        println!("   4. Copy your public key:");
        println!("      cat ~/.ssh/id_ed25519.pub");
        println!("   5. Add it to your Git hosting service");
    }
}
