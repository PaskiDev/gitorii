use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use crate::error::{Result, ToriiError};

/// Global Torii configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToriiConfig {
    /// User settings
    pub user: UserConfig,

    /// Snapshot settings
    pub snapshot: SnapshotConfig,

    /// Mirror settings
    pub mirror: MirrorConfig,

    /// Git settings
    pub git: GitConfig,

    /// UI settings
    pub ui: UiConfig,

    /// Platform auth tokens
    #[serde(default)]
    pub auth: AuthConfig,

    /// Update notifier settings
    #[serde(default)]
    pub update: UpdateConfig,

    /// Worktree settings
    #[serde(default)]
    pub worktree: WorktreeConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorktreeConfig {
    /// Where `torii worktree add` puts new worktrees when no path is given.
    /// Default is `..` (sibling directories of the main repo). Examples:
    ///   ".."                  → ../<repo>-<branch>/
    ///   "~/worktrees"         → ~/worktrees/<repo>-<branch>/
    ///   "/tmp/wt"             → /tmp/wt/<repo>-<branch>/
    /// `~` expansion is honoured. The `<repo>-<branch>` suffix is appended
    /// automatically (branch slashes replaced with `-`).
    pub base_dir: String,

    /// Paths from the main repo to also drop into every freshly-created
    /// worktree. Typical entries: `.env`, `target/`, `node_modules/`, build
    /// caches that aren't tracked by git but you don't want to regenerate
    /// from scratch in every linked working copy.
    ///
    /// Each entry is resolved relative to the main repo's working
    /// directory. Heuristic per entry:
    ///   - directory present → symlink into the new worktree
    ///   - file present      → copy into the new worktree
    ///   - missing           → silently skipped
    ///
    /// Default is empty (no inheritance, vanilla `git worktree` behaviour).
    #[serde(default)]
    pub inherit_paths: Vec<String>,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            base_dir: "..".to_string(),
            inherit_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateConfig {
    /// Check crates.io for newer versions on CLI exit
    pub check: bool,

    /// Hours between checks (cached locally)
    pub interval_hours: u64,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self { check: true, interval_hours: 24 }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AuthConfig {
    /// GitHub personal access token
    pub github_token: Option<String>,

    /// GitLab personal access token
    pub gitlab_token: Option<String>,

    /// Gitea token
    pub gitea_token: Option<String>,

    /// Forgejo token
    pub forgejo_token: Option<String>,

    /// Codeberg token
    pub codeberg_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserConfig {
    /// Default author name for commits
    pub name: Option<String>,
    
    /// Default author email for commits
    pub email: Option<String>,
    
    /// Preferred editor
    pub editor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SnapshotConfig {
    /// Enable auto-snapshots
    pub auto_enabled: bool,
    
    /// Auto-snapshot interval in minutes
    pub auto_interval_minutes: u32,
    
    /// Retention period in days
    pub retention_days: u32,
    
    /// Maximum number of snapshots to keep
    pub max_snapshots: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MirrorConfig {
    /// Enable auto-fetch from mirrors
    pub autofetch_enabled: bool,
    
    /// Auto-fetch interval in minutes
    pub autofetch_interval_minutes: u32,
    
    /// Default protocol (ssh or https)
    pub default_protocol: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GitConfig {
    /// Default branch name for new repos
    pub default_branch: String,
    
    /// Auto-sign commits with GPG
    pub sign_commits: bool,
    
    /// GPG key ID
    pub gpg_key: Option<String>,
    
    /// Always use rebase instead of merge for pulls
    pub pull_rebase: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UiConfig {
    /// Use colored output
    pub colors: bool,
    
    /// Show emoji in output
    pub emoji: bool,
    
    /// Verbose output
    pub verbose: bool,
    
    /// Preferred date format
    pub date_format: String,
}

impl Default for ToriiConfig {
    fn default() -> Self {
        Self {
            user: UserConfig {
                name: None,
                email: None,
                editor: std::env::var("EDITOR").ok(),
            },
            snapshot: SnapshotConfig {
                auto_enabled: false,
                auto_interval_minutes: 30,
                retention_days: 30,
                max_snapshots: Some(100),
            },
            mirror: MirrorConfig {
                autofetch_enabled: false,
                autofetch_interval_minutes: 30,
                default_protocol: "ssh".to_string(),
            },
            git: GitConfig {
                default_branch: "main".to_string(),
                sign_commits: false,
                gpg_key: None,
                pull_rebase: false,
            },
            ui: UiConfig {
                colors: true,
                emoji: true,
                verbose: false,
                date_format: "%Y-%m-%d %H:%M".to_string(),
            },
            auth: AuthConfig::default(),
            update: UpdateConfig::default(),
            worktree: WorktreeConfig::default(),
        }
    }
}

impl ToriiConfig {
    /// Get the global config file path
    fn global_config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| ToriiError::InvalidConfig("Could not determine config directory for this platform".to_string()))?
            .join("torii");
        fs::create_dir_all(&config_dir)?;
        Ok(config_dir.join("config.toml"))
    }
    
    /// Get the local repo config file path
    fn local_config_path<P: AsRef<Path>>(repo_path: P) -> Result<PathBuf> {
        let torii_dir = repo_path.as_ref().join(".torii");
        fs::create_dir_all(&torii_dir)?;
        Ok(torii_dir.join("config.toml"))
    }
    
    /// Load global configuration
    pub fn load_global() -> Result<Self> {
        let config_path = Self::global_config_path()?;
        
        if !config_path.exists() {
            return Ok(Self::default());
        }
        
        let config_str = fs::read_to_string(&config_path)?;
        let config: ToriiConfig = toml::from_str(&config_str)
            .map_err(|e| ToriiError::InvalidConfig(format!("Failed to parse config: {}", e)))?;
        
        Ok(config)
    }
    
    /// Load local repository configuration (merged with global)
    pub fn load_local<P: AsRef<Path>>(repo_path: P) -> Result<Self> {
        let mut config = Self::load_global()?;
        
        let local_path = Self::local_config_path(&repo_path)?;
        if local_path.exists() {
            let local_str = fs::read_to_string(&local_path)?;
            let local_config: ToriiConfig = toml::from_str(&local_str)
                .map_err(|e| ToriiError::InvalidConfig(format!("Failed to parse local config: {}", e)))?;
            
            // Merge local config over global (local takes precedence)
            config = Self::merge(config, local_config);
        }
        
        Ok(config)
    }
    
    /// Save global configuration
    pub fn save_global(&self) -> Result<()> {
        let config_path = Self::global_config_path()?;
        let config_str = toml::to_string_pretty(self)
            .map_err(|e| ToriiError::InvalidConfig(format!("Failed to serialize config: {}", e)))?;
        fs::write(&config_path, config_str)?;
        Ok(())
    }
    
    /// Save local repository configuration
    pub fn save_local<P: AsRef<Path>>(&self, repo_path: P) -> Result<()> {
        let config_path = Self::local_config_path(repo_path)?;
        let config_str = toml::to_string_pretty(self)
            .map_err(|e| ToriiError::InvalidConfig(format!("Failed to serialize config: {}", e)))?;
        fs::write(&config_path, config_str)?;
        Ok(())
    }
    
    /// Merge two configs (second takes precedence for non-None values)
    fn merge(mut base: Self, overlay: Self) -> Self {
        // User config
        if overlay.user.name.is_some() {
            base.user.name = overlay.user.name;
        }
        if overlay.user.email.is_some() {
            base.user.email = overlay.user.email;
        }
        if overlay.user.editor.is_some() {
            base.user.editor = overlay.user.editor;
        }
        
        // Snapshot config
        base.snapshot = overlay.snapshot;
        
        // Mirror config
        base.mirror = overlay.mirror;
        
        // Git config
        base.git = overlay.git;

        // UI config
        base.ui = overlay.ui;

        // Auth config
        if overlay.auth.github_token.is_some() { base.auth.github_token = overlay.auth.github_token; }
        if overlay.auth.gitlab_token.is_some() { base.auth.gitlab_token = overlay.auth.gitlab_token; }
        if overlay.auth.gitea_token.is_some() { base.auth.gitea_token = overlay.auth.gitea_token; }
        if overlay.auth.forgejo_token.is_some() { base.auth.forgejo_token = overlay.auth.forgejo_token; }
        if overlay.auth.codeberg_token.is_some() { base.auth.codeberg_token = overlay.auth.codeberg_token; }

        // Worktree config — full overwrite, like snapshot/mirror.
        base.worktree = overlay.worktree;

        base
    }
    
    /// Get a configuration value by key path (e.g., "user.name", "snapshot.auto_enabled")
    pub fn get(&self, key: &str) -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() != 2 {
            return None;
        }
        
        match (parts[0], parts[1]) {
            ("user", "name") => self.user.name.clone(),
            ("user", "email") => self.user.email.clone(),
            ("user", "editor") => self.user.editor.clone(),
            ("snapshot", "auto_enabled") => Some(self.snapshot.auto_enabled.to_string()),
            ("snapshot", "auto_interval_minutes") => Some(self.snapshot.auto_interval_minutes.to_string()),
            ("snapshot", "retention_days") => Some(self.snapshot.retention_days.to_string()),
            ("snapshot", "max_snapshots") => self.snapshot.max_snapshots.map(|v| v.to_string()),
            ("mirror", "autofetch_enabled") => Some(self.mirror.autofetch_enabled.to_string()),
            ("mirror", "autofetch_interval_minutes") => Some(self.mirror.autofetch_interval_minutes.to_string()),
            ("mirror", "default_protocol") => Some(self.mirror.default_protocol.clone()),
            ("git", "default_branch") => Some(self.git.default_branch.clone()),
            ("git", "sign_commits") => Some(self.git.sign_commits.to_string()),
            ("git", "gpg_key") => self.git.gpg_key.clone(),
            // 0.7.14: git-friendly alias. Mirrors how git stores it.
            ("user", "signingkey") => self.git.gpg_key.clone(),
            ("commit", "gpgsign") => Some(self.git.sign_commits.to_string()),
            ("git", "pull_rebase") => Some(self.git.pull_rebase.to_string()),
            ("ui", "colors") => Some(self.ui.colors.to_string()),
            ("ui", "emoji") => Some(self.ui.emoji.to_string()),
            ("ui", "verbose") => Some(self.ui.verbose.to_string()),
            ("ui", "date_format") => Some(self.ui.date_format.clone()),
            ("auth", "github_token") => self.auth.github_token.clone().map(|_| "[set]".to_string()),
            ("auth", "gitlab_token") => self.auth.gitlab_token.clone().map(|_| "[set]".to_string()),
            ("auth", "gitea_token") => self.auth.gitea_token.clone().map(|_| "[set]".to_string()),
            ("auth", "forgejo_token") => self.auth.forgejo_token.clone().map(|_| "[set]".to_string()),
            ("auth", "codeberg_token") => self.auth.codeberg_token.clone().map(|_| "[set]".to_string()),
            ("worktree", "base_dir") => Some(self.worktree.base_dir.clone()),
            ("worktree", "inherit_paths") => {
                if self.worktree.inherit_paths.is_empty() {
                    None
                } else {
                    Some(self.worktree.inherit_paths.join(","))
                }
            }
            _ => None,
        }
    }
    
    /// Set a configuration value by key path
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() != 2 {
            return Err(ToriiError::InvalidConfig(format!("Invalid config key: {}", key)));
        }
        
        match (parts[0], parts[1]) {
            ("user", "name") => self.user.name = Some(value.to_string()),
            ("user", "email") => self.user.email = Some(value.to_string()),
            ("user", "editor") => self.user.editor = Some(value.to_string()),
            ("snapshot", "auto_enabled") => {
                self.snapshot.auto_enabled = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be true or false".to_string()))?;
            }
            ("snapshot", "auto_interval_minutes") => {
                self.snapshot.auto_interval_minutes = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be a number".to_string()))?;
            }
            ("snapshot", "retention_days") => {
                self.snapshot.retention_days = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be a number".to_string()))?;
            }
            ("snapshot", "max_snapshots") => {
                self.snapshot.max_snapshots = Some(value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be a number".to_string()))?);
            }
            ("mirror", "autofetch_enabled") => {
                self.mirror.autofetch_enabled = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be true or false".to_string()))?;
            }
            ("mirror", "autofetch_interval_minutes") => {
                self.mirror.autofetch_interval_minutes = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be a number".to_string()))?;
            }
            ("mirror", "default_protocol") => {
                if value != "ssh" && value != "https" {
                    return Err(ToriiError::InvalidConfig("Protocol must be 'ssh' or 'https'".to_string()));
                }
                self.mirror.default_protocol = value.to_string();
            }
            ("git", "default_branch") => self.git.default_branch = value.to_string(),
            ("git", "sign_commits") => {
                self.git.sign_commits = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be true or false".to_string()))?;
            }
            ("git", "gpg_key") => self.git.gpg_key = Some(value.to_string()),
            // 0.7.14: git-friendly aliases that map to git.gpg_key /
            // git.sign_commits respectively. Either name works.
            ("user", "signingkey") => self.git.gpg_key = Some(value.to_string()),
            ("commit", "gpgsign") => {
                self.git.sign_commits = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be true or false".to_string()))?;
            }
            ("git", "pull_rebase") => {
                self.git.pull_rebase = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be true or false".to_string()))?;
            }
            ("ui", "colors") => {
                self.ui.colors = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be true or false".to_string()))?;
            }
            ("ui", "emoji") => {
                self.ui.emoji = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be true or false".to_string()))?;
            }
            ("ui", "verbose") => {
                self.ui.verbose = value.parse()
                    .map_err(|_| ToriiError::InvalidConfig("Value must be true or false".to_string()))?;
            }
            ("ui", "date_format") => self.ui.date_format = value.to_string(),
            ("auth", "github_token") => self.auth.github_token = Some(value.to_string()),
            ("auth", "gitlab_token") => self.auth.gitlab_token = Some(value.to_string()),
            ("auth", "gitea_token") => self.auth.gitea_token = Some(value.to_string()),
            ("auth", "forgejo_token") => self.auth.forgejo_token = Some(value.to_string()),
            ("auth", "codeberg_token") => self.auth.codeberg_token = Some(value.to_string()),
            ("worktree", "base_dir") => {
                if value.trim().is_empty() {
                    return Err(ToriiError::InvalidConfig(
                        "worktree.base_dir must not be empty (use '..' for sibling directories)".to_string(),
                    ));
                }
                self.worktree.base_dir = value.to_string();
            }
            ("worktree", "inherit_paths") => {
                // Accept comma-separated list; empty string clears.
                self.worktree.inherit_paths = if value.trim().is_empty() {
                    Vec::new()
                } else {
                    value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };
            }
            _ => return Err(ToriiError::InvalidConfig(format!("Unknown config key: {}", key))),
        }
        
        Ok(())
    }
    
    /// List all configuration values
    pub fn list(&self) -> Vec<(String, String)> {
        let mut items = Vec::new();
        
        // User
        if let Some(name) = &self.user.name {
            items.push(("user.name".to_string(), name.clone()));
        }
        if let Some(email) = &self.user.email {
            items.push(("user.email".to_string(), email.clone()));
        }
        if let Some(editor) = &self.user.editor {
            items.push(("user.editor".to_string(), editor.clone()));
        }
        
        // Snapshot
        items.push(("snapshot.auto_enabled".to_string(), self.snapshot.auto_enabled.to_string()));
        items.push(("snapshot.auto_interval_minutes".to_string(), self.snapshot.auto_interval_minutes.to_string()));
        items.push(("snapshot.retention_days".to_string(), self.snapshot.retention_days.to_string()));
        if let Some(max) = self.snapshot.max_snapshots {
            items.push(("snapshot.max_snapshots".to_string(), max.to_string()));
        }
        
        // Mirror
        items.push(("mirror.autofetch_enabled".to_string(), self.mirror.autofetch_enabled.to_string()));
        items.push(("mirror.autofetch_interval_minutes".to_string(), self.mirror.autofetch_interval_minutes.to_string()));
        items.push(("mirror.default_protocol".to_string(), self.mirror.default_protocol.clone()));
        
        // Git
        items.push(("git.default_branch".to_string(), self.git.default_branch.clone()));
        items.push(("git.sign_commits".to_string(), self.git.sign_commits.to_string()));
        if let Some(key) = &self.git.gpg_key {
            items.push(("git.gpg_key".to_string(), key.clone()));
        }
        items.push(("git.pull_rebase".to_string(), self.git.pull_rebase.to_string()));
        
        // UI
        items.push(("ui.colors".to_string(), self.ui.colors.to_string()));
        items.push(("ui.emoji".to_string(), self.ui.emoji.to_string()));
        items.push(("ui.verbose".to_string(), self.ui.verbose.to_string()));
        items.push(("ui.date_format".to_string(), self.ui.date_format.clone()));

        // Auth (always show, mask value if set)
        items.push(("auth.github_token".to_string(), if self.auth.github_token.is_some() { "[set]".to_string() } else { "[not set]".to_string() }));
        items.push(("auth.gitlab_token".to_string(), if self.auth.gitlab_token.is_some() { "[set]".to_string() } else { "[not set]".to_string() }));
        items.push(("auth.gitea_token".to_string(), if self.auth.gitea_token.is_some() { "[set]".to_string() } else { "[not set]".to_string() }));
        items.push(("auth.forgejo_token".to_string(), if self.auth.forgejo_token.is_some() { "[set]".to_string() } else { "[not set]".to_string() }));
        items.push(("auth.codeberg_token".to_string(), if self.auth.codeberg_token.is_some() { "[set]".to_string() } else { "[not set]".to_string() }));

        // Worktree
        items.push(("worktree.base_dir".to_string(), self.worktree.base_dir.clone()));
        if !self.worktree.inherit_paths.is_empty() {
            items.push((
                "worktree.inherit_paths".to_string(),
                self.worktree.inherit_paths.join(","),
            ));
        }

        items
    }
}
