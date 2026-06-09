use crate::core::GitRepo;
use crate::duration::format_duration;
use crate::error::{Result, ToriiError};
use dirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Clone, PartialEq)]
pub enum MirrorType {
    Primary,
    Replica,
}

impl<'de> serde::Deserialize<'de> for MirrorType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "Primary" | "Master" => Ok(MirrorType::Primary),
            "Replica" | "Slave" => Ok(MirrorType::Replica),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["Primary", "Replica"],
            )),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum AccountType {
    User,
    Organization,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Protocol {
    SSH,
    HTTPS,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Mirror {
    pub name: String,
    pub platform: String,
    pub account_type: AccountType,
    pub account_name: String,
    pub repo_name: String,
    pub url: String,
    pub protocol: Protocol,
    pub mirror_type: MirrorType,
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct MirrorConfig {
    mirrors: Vec<Mirror>,
    #[serde(default)]
    autofetch_enabled: bool,
    #[serde(default = "default_autofetch_interval")]
    autofetch_interval_minutes: u32,
}

fn default_autofetch_interval() -> u32 {
    30 // Default: 30 minutes
}

impl Mirror {
    /// Generate URL based on platform, account info, and protocol
    pub fn generate_url(
        platform: &str,
        _account_type: &AccountType,
        account_name: &str,
        repo_name: &str,
        protocol: &Protocol,
    ) -> String {
        match protocol {
            Protocol::SSH => match platform.to_lowercase().as_str() {
                "github" => format!("git@github.com:{}/{}.git", account_name, repo_name),
                "gitlab" => format!("git@gitlab.com:{}/{}.git", account_name, repo_name),
                "bitbucket" => format!("git@bitbucket.org:{}/{}.git", account_name, repo_name),
                "codeberg" => format!("git@codeberg.org:{}/{}.git", account_name, repo_name),
                "gitea" => format!("git@gitea.com:{}/{}.git", account_name, repo_name),
                "forgejo" => format!("git@codeberg.org:{}/{}.git", account_name, repo_name),
                "sourcehut" | "srht" => format!("git@git.sr.ht:~{}/{}", account_name, repo_name),
                "sourceforge" => format!("git@git.code.sf.net:p/{}/{}", account_name, repo_name),
                _ => format!("git@{}:{}/{}.git", platform, account_name, repo_name),
            },
            Protocol::HTTPS => match platform.to_lowercase().as_str() {
                "github" => format!("https://github.com/{}/{}.git", account_name, repo_name),
                "gitlab" => format!("https://gitlab.com/{}/{}.git", account_name, repo_name),
                "bitbucket" => format!("https://bitbucket.org/{}/{}.git", account_name, repo_name),
                "codeberg" => format!("https://codeberg.org/{}/{}.git", account_name, repo_name),
                "gitea" => format!("https://gitea.com/{}/{}.git", account_name, repo_name),
                "forgejo" => format!("https://codeberg.org/{}/{}.git", account_name, repo_name),
                "sourcehut" | "srht" => {
                    format!("https://git.sr.ht/~{}/{}", account_name, repo_name)
                }
                "sourceforge" => {
                    format!("https://git.code.sf.net/p/{}/{}", account_name, repo_name)
                }
                _ => format!("https://{}/{}/{}.git", platform, account_name, repo_name),
            },
        }
    }

    /// Get display name for the mirror
    #[allow(dead_code)]
    pub fn display_name(&self) -> String {
        format!("{}/{}", self.account_name, self.repo_name)
    }
}

pub struct MirrorManager {
    repo_path: PathBuf,
    config_path: PathBuf,
}

impl MirrorManager {
    pub fn new<P: AsRef<Path>>(repo_path: P) -> Result<Self> {
        let repo_path = repo_path.as_ref().to_path_buf();
        let torii_dir = repo_path.join(".torii");
        fs::create_dir_all(&torii_dir)?;

        let config_path = torii_dir.join("mirrors.json");

        Ok(Self {
            repo_path,
            config_path,
        })
    }

    /// Load mirror configuration
    fn load_config(&self) -> Result<MirrorConfig> {
        if !self.config_path.exists() {
            return Ok(MirrorConfig {
                mirrors: vec![],
                autofetch_enabled: false,
                autofetch_interval_minutes: 30,
            });
        }

        let config_str = fs::read_to_string(&self.config_path)?;
        let config: MirrorConfig = serde_json::from_str(&config_str)?;

        Ok(config)
    }

    /// Save mirror configuration
    fn save_config(&self, config: &MirrorConfig) -> Result<()> {
        let config_str = serde_json::to_string_pretty(config)?;
        fs::write(&self.config_path, config_str)?;
        Ok(())
    }

    /// Add a new mirror with simplified interface
    pub fn add_mirror(
        &self,
        platform: &str,
        account_type: AccountType,
        account_name: &str,
        repo_name: &str,
        protocol: Protocol,
        is_primary: bool,
    ) -> Result<()> {
        let mut config = self.load_config()?;

        // Check if master already exists
        if is_primary
            && config
                .mirrors
                .iter()
                .any(|m| m.mirror_type == MirrorType::Primary)
        {
            return Err(ToriiError::Mirror(
                "A primary mirror already exists. Use 'torii mirror promote' to change it."
                    .to_string(),
            ));
        }

        // Generate URL automatically
        let url = Mirror::generate_url(platform, &account_type, account_name, repo_name, &protocol);

        // Generate remote name
        let remote_name = if is_primary {
            "origin".to_string()
        } else {
            format!("{}-{}", platform, account_name)
        };

        let mirror = Mirror {
            name: remote_name.clone(),
            platform: platform.to_string(),
            account_type,
            account_name: account_name.to_string(),
            repo_name: repo_name.to_string(),
            url: url.clone(),
            protocol,
            mirror_type: if is_primary {
                MirrorType::Primary
            } else {
                MirrorType::Replica
            },
            enabled: true,
        };

        // Add the git remote first — if that fails (invalid URL, libgit2
        // error) we haven't yet polluted mirrors.json. Then persist the
        // config; if THAT fails (disk full, perms), roll the remote back
        // so disk state matches mirrors.json.
        let repo = GitRepo::open(&self.repo_path)?;
        self.add_git_remote(&repo, &remote_name, &url)?;

        config.mirrors.push(mirror);
        if let Err(e) = self.save_config(&config) {
            let _ = repo.repository().remote_delete(&remote_name);
            return Err(e);
        }

        Ok(())
    }

    /// Set a mirror as master
    pub fn set_primary(&self, platform: &str, account_name: &str) -> Result<()> {
        let mut config = self.load_config()?;

        // Find the mirror
        let mirror_index = config
            .mirrors
            .iter()
            .position(|m| m.platform == platform && m.account_name == account_name)
            .ok_or_else(|| ToriiError::Mirror("Mirror not found".to_string()))?;

        // Set all to replica
        for mirror in &mut config.mirrors {
            mirror.mirror_type = MirrorType::Replica;
        }

        // Set selected as primary
        config.mirrors[mirror_index].mirror_type = MirrorType::Primary;

        self.save_config(&config)?;
        Ok(())
    }

    /// Add git remote
    fn add_git_remote(&self, repo: &GitRepo, name: &str, url: &str) -> Result<()> {
        repo.repository().remote(name, url)?;
        Ok(())
    }

    /// List all mirrors
    pub fn list_mirrors(&self) -> Result<()> {
        let config = self.load_config()?;

        if config.mirrors.is_empty() {
            println!("No mirrors configured");
            println!();
            println!("💡 Add a primary mirror first:");
            println!("   torii mirror add <platform> <user|org> <account> <repo> --primary");
            return Ok(());
        }

        println!("🪞 Configured Mirrors:");
        println!();

        // Show primary first
        for mirror in config
            .mirrors
            .iter()
            .filter(|m| m.mirror_type == MirrorType::Primary)
        {
            let status = if mirror.enabled { "✅" } else { "❌" };
            let account_type = match mirror.account_type {
                AccountType::User => "👤",
                AccountType::Organization => "🏢",
            };
            let protocol_icon = match mirror.protocol {
                Protocol::SSH => "🔑",
                Protocol::HTTPS => "🌐",
            };
            println!(
                "  {} 👑 PRIMARY - {} {} {} {}/{}",
                status,
                protocol_icon,
                account_type,
                mirror.platform,
                mirror.account_name,
                mirror.repo_name
            );
            println!("     {}", mirror.url);
            println!();
        }

        // Show replicas
        let replicas: Vec<_> = config
            .mirrors
            .iter()
            .filter(|m| m.mirror_type == MirrorType::Replica)
            .collect();

        if !replicas.is_empty() {
            println!("  Replica Mirrors:");
            for mirror in replicas {
                let status = if mirror.enabled { "✅" } else { "❌" };
                let account_type = match mirror.account_type {
                    AccountType::User => "👤",
                    AccountType::Organization => "🏢",
                };
                let protocol_icon = match mirror.protocol {
                    Protocol::SSH => "🔑",
                    Protocol::HTTPS => "🌐",
                };
                println!(
                    "    {} {} {} {} {}/{}",
                    status,
                    protocol_icon,
                    account_type,
                    mirror.platform,
                    mirror.account_name,
                    mirror.repo_name
                );
                println!("       {}", mirror.url);
            }
        }

        Ok(())
    }

    /// Sync replicas silently if any are configured — called automatically by `torii sync`
    pub fn sync_replicas_if_any(&self, force: bool) -> Result<()> {
        let config = self.load_config()?;
        let replicas: Vec<_> = config
            .mirrors
            .iter()
            .filter(|m| m.mirror_type == MirrorType::Replica && m.enabled)
            .collect();
        if replicas.is_empty() {
            return Ok(());
        }
        let repo = GitRepo::open(&self.repo_path)?;
        let mut failed = vec![];
        for mirror in &replicas {
            if let Err(e) = self.sync_to_mirror(&repo, mirror, force) {
                failed.push(format!(
                    "{}/{}: {}",
                    mirror.platform, mirror.account_name, e
                ));
            }
        }
        let ok = replicas.len() - failed.len();
        if ok > 0 {
            println!("🪞 Mirrors synced: {}/{}", ok, replicas.len());
        }
        for f in &failed {
            eprintln!("⚠️  Mirror sync failed: {}", f);
        }
        Ok(())
    }

    /// Sync to all replica mirrors (push from master)
    pub fn sync_all(&self, force: bool) -> Result<()> {
        let config = self.load_config()?;
        let repo = GitRepo::open(&self.repo_path)?;

        // Find primary mirror
        let primary = config
            .mirrors
            .iter()
            .find(|m| m.mirror_type == MirrorType::Primary);

        if primary.is_none() {
            println!("⚠️  No primary mirror configured. Add one with:");
            println!("   torii mirror add-primary <platform> <user|org> <account> <repo>");
            return Ok(());
        }

        // Get replica mirrors
        let replicas: Vec<_> = config
            .mirrors
            .iter()
            .filter(|m| m.mirror_type == MirrorType::Replica && m.enabled)
            .collect();

        if replicas.is_empty() {
            println!("ℹ️  No replica mirrors configured. Add one with:");
            println!("   torii mirror add-replica <platform> <user|org> <account> <repo>");
            return Ok(());
        }

        println!(
            "📤 Syncing from primary to {} replica mirror(s)...\n",
            replicas.len()
        );

        let mut success_count = 0;
        let mut fail_count = 0;

        for mirror in replicas {
            println!(
                "🔄 Syncing to {} {}/{} ...",
                mirror.platform, mirror.account_name, mirror.repo_name
            );

            match self.sync_to_mirror(&repo, mirror, force) {
                Ok(_) => {
                    println!("  ✅ Synced successfully\n");
                    success_count += 1;
                }
                Err(e) => {
                    eprintln!("  ❌ Failed: {}\n", e);
                    fail_count += 1;
                    if !force {
                        return Err(e);
                    }
                }
            }
        }

        println!(
            "📊 Summary: {} succeeded, {} failed",
            success_count, fail_count
        );
        Ok(())
    }

    /// Sync to a specific mirror
    fn sync_to_mirror(&self, repo: &GitRepo, mirror: &Mirror, force: bool) -> Result<()> {
        let mut remote = repo.repository().find_remote(&mirror.name)?;
        let branch = repo.get_current_branch()?;

        let refspec = if force {
            format!("+refs/heads/{}:refs/heads/{}", branch, branch)
        } else {
            format!("refs/heads/{}:refs/heads/{}", branch, branch)
        };

        // Setup SSH callbacks — ed25519, then rsa, then agent
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, _allowed_types| {
            let username = username_from_url.unwrap_or("git");
            let home = dirs::home_dir().unwrap_or_default();
            let ed25519 = home.join(".ssh").join("id_ed25519");
            let rsa = home.join(".ssh").join("id_rsa");
            if ed25519.exists() {
                git2::Cred::ssh_key(username, None, &ed25519, None)
            } else if rsa.exists() {
                git2::Cred::ssh_key(username, None, &rsa, None)
            } else {
                git2::Cred::ssh_key_from_agent(username)
            }
        });

        let mut push_options = git2::PushOptions::new();
        push_options.remote_callbacks(callbacks);

        remote.push(&[&refspec], Some(&mut push_options))?;

        // Push tags via git2 — but only the ones whose OID differs from
        // what the mirror already has. Pre-0.7.8 this re-pushed every
        // local tag on every replica sync, which made GitLab retrigger
        // its workflow:rules for every historical tag (see the matching
        // comment in `core.rs::push_all_tags`). One extra
        // ls-remote round-trip per replica saves N stale pipelines.
        let tags = repo.repository().tag_names(None)?;
        if !tags.is_empty() {
            let local: std::collections::HashMap<String, git2::Oid> = tags
                .iter()
                .flatten()
                .filter_map(|t| {
                    let refname = format!("refs/tags/{}", t);
                    repo.repository()
                        .refname_to_id(&refname)
                        .ok()
                        .map(|oid| (t.to_string(), oid))
                })
                .collect();

            let mut tag_remote = repo.repository().find_remote(&mirror.name)?;

            let make_ssh_callbacks = || {
                let mut cb = git2::RemoteCallbacks::new();
                cb.credentials(|_url, username_from_url, _allowed_types| {
                    let username = username_from_url.unwrap_or("git");
                    let home = dirs::home_dir().unwrap_or_default();
                    let ed25519 = home.join(".ssh").join("id_ed25519");
                    let rsa = home.join(".ssh").join("id_rsa");
                    if ed25519.exists() {
                        git2::Cred::ssh_key(username, None, &ed25519, None)
                    } else if rsa.exists() {
                        git2::Cred::ssh_key(username, None, &rsa, None)
                    } else {
                        git2::Cred::ssh_key_from_agent(username)
                    }
                });
                cb
            };

            // ls-remote on the mirror to learn what tags are already there.
            let remote_tags: std::collections::HashMap<String, git2::Oid> = {
                let cb = make_ssh_callbacks();
                tag_remote.connect_auth(git2::Direction::Fetch, Some(cb), None)?;
                let list = tag_remote.list()?;
                let map = list
                    .iter()
                    .filter_map(|h| {
                        h.name()
                            .strip_prefix("refs/tags/")
                            .filter(|n| !n.ends_with("^{}"))
                            .map(|n| (n.to_string(), h.oid()))
                    })
                    .collect::<std::collections::HashMap<_, _>>();
                let _ = tag_remote.disconnect();
                map
            };

            let refspecs: Vec<String> = local
                .iter()
                .filter(|(name, oid)| remote_tags.get(*name) != Some(oid))
                .map(|(t, _)| {
                    let r = format!("refs/tags/{}:refs/tags/{}", t, t);
                    if force {
                        format!("+{}", r)
                    } else {
                        r
                    }
                })
                .collect();

            if !refspecs.is_empty() {
                let refspec_refs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();
                let mut tag_push_opts = git2::PushOptions::new();
                tag_push_opts.remote_callbacks(make_ssh_callbacks());
                let _ = tag_remote.push(&refspec_refs, Some(&mut tag_push_opts));
            }
        }

        Ok(())
    }

    /// Remove a mirror by platform and account
    pub fn remove_mirror_by_account(&self, platform: &str, account: &str) -> Result<()> {
        let mut config = self.load_config()?;

        let mirror = config
            .mirrors
            .iter()
            .find(|m| m.platform == platform && m.account_name == account)
            .ok_or_else(|| ToriiError::Mirror("Mirror not found".to_string()))?;

        let remote_name = mirror.name.clone();

        config
            .mirrors
            .retain(|m| !(m.platform == platform && m.account_name == account));
        self.save_config(&config)?;

        let repo = GitRepo::open(&self.repo_path)?;
        repo.repository().remote_delete(&remote_name)?;

        Ok(())
    }

    /// Remove a mirror by name (legacy)
    #[allow(dead_code)]
    pub fn remove_mirror(&self, name: &str) -> Result<()> {
        let mut config = self.load_config()?;

        config.mirrors.retain(|m| m.name != name);
        self.save_config(&config)?;

        let repo = GitRepo::open(&self.repo_path)?;
        repo.repository().remote_delete(name)?;

        Ok(())
    }

    /// Configure autofetch settings
    pub fn configure_autofetch(&self, enable: bool, interval: Option<u32>) -> Result<()> {
        let mut config = self.load_config()?;

        config.autofetch_enabled = enable;
        if let Some(interval_minutes) = interval {
            config.autofetch_interval_minutes = interval_minutes;
        }

        self.save_config(&config)?;

        if enable {
            let duration_str = format_duration(config.autofetch_interval_minutes);
            println!("✅ Autofetch enabled: every {}", duration_str);
            println!("💡 Torii will automatically fetch updates from all mirrors");
        } else {
            println!("❌ Autofetch disabled");
        }

        Ok(())
    }

    /// Show autofetch status
    pub fn show_autofetch_status(&self) -> Result<()> {
        let config = self.load_config()?;

        println!("🔄 Autofetch Configuration:");
        println!();

        if config.autofetch_enabled {
            let duration_str = format_duration(config.autofetch_interval_minutes);
            println!("  Status: ✅ Enabled");
            println!("  Interval: {}", duration_str);
            println!();
            println!(
                "💡 Torii will automatically fetch from all mirrors every {}",
                duration_str
            );
        } else {
            println!("  Status: ❌ Disabled");
            println!();
            println!("💡 Enable with:");
            println!("   torii mirror autofetch --enable --interval <duration>");
            println!("   Examples: 10m, 30s, 2h, 1d");
        }

        Ok(())
    }
}
