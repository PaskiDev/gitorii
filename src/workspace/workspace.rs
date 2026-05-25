use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::error::{Result, ToriiError};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub workspace: HashMap<String, WorkspaceEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceEntry {
    pub repos: Vec<String>,
}

impl WorkspaceConfig {
    fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| ToriiError::InvalidConfig("Could not determine config directory".to_string()))?
            .join("torii");
        fs::create_dir_all(&dir)?;
        Ok(dir.join("workspaces.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = fs::read_to_string(&path)?;
        toml::from_str(&s).map_err(|e| ToriiError::InvalidConfig(format!("Failed to parse workspaces.toml: {}", e)))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        let s = toml::to_string_pretty(self)
            .map_err(|e| ToriiError::InvalidConfig(format!("Failed to serialize workspaces: {}", e)))?;
        fs::write(&path, s)?;
        Ok(())
    }

    pub fn add_repo(&mut self, workspace: &str, repo_path: &str) -> Result<()> {
        let expanded = expand_path(repo_path)?;
        let entry = self.workspace.entry(workspace.to_string()).or_insert(WorkspaceEntry { repos: vec![] });
        let canonical = expanded.to_string_lossy().to_string();
        if !entry.repos.contains(&canonical) {
            entry.repos.push(canonical);
        }
        Ok(())
    }

    pub fn remove_repo(&mut self, workspace: &str, repo_path: &str) -> Result<()> {
        let expanded = expand_path(repo_path)?;
        let canonical = expanded.to_string_lossy().to_string();
        if let Some(entry) = self.workspace.get_mut(workspace) {
            entry.repos.retain(|r| r != &canonical);
        }
        Ok(())
    }

    pub fn get(&self, workspace: &str) -> Option<&WorkspaceEntry> {
        self.workspace.get(workspace)
    }
}

fn expand_path(path: &str) -> Result<PathBuf> {
    if path.starts_with("~/") {
        let home = dirs::home_dir()
            .ok_or_else(|| ToriiError::InvalidConfig("Could not determine home directory".to_string()))?;
        Ok(home.join(&path[2..]))
    } else {
        Ok(PathBuf::from(path))
    }
}

pub struct WorkspaceManager;

#[derive(Debug)]
pub struct RepoStatus {
    #[allow(dead_code)]
    pub path: String,
    pub name: String,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    #[allow(dead_code)]
    pub error: Option<String>,
}

impl WorkspaceManager {
    pub fn status(workspace_name: &str) -> Result<()> {
        let cfg = WorkspaceConfig::load()?;
        let entry = cfg.get(workspace_name)
            .ok_or_else(|| ToriiError::InvalidConfig(format!("Workspace '{}' not found", workspace_name)))?;

        println!("📦 {}", workspace_name);
        println!();

        for repo_path in &entry.repos {
            let status = Self::repo_status(repo_path);
            match status {
                Ok(s) => {
                    let changes = s.staged + s.unstaged + s.untracked;
                    if changes == 0 {
                        println!("  {:<20} ✅ clean        ({})", s.name, s.branch);
                    } else {
                        let mut parts = vec![];
                        if s.staged > 0 { parts.push(format!("{} staged", s.staged)); }
                        if s.unstaged > 0 { parts.push(format!("{} modified", s.unstaged)); }
                        if s.untracked > 0 { parts.push(format!("{} untracked", s.untracked)); }
                        println!("  {:<20} 📝 {}  ({})", s.name, parts.join(", "), s.branch);
                    }
                    if s.ahead > 0 || s.behind > 0 {
                        println!("  {:<20}    ↑{} ahead, ↓{} behind", "", s.ahead, s.behind);
                    }
                }
                Err(e) => {
                    let name = Path::new(repo_path).file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| repo_path.clone());
                    println!("  {:<20} ❌ {}", name, e);
                }
            }
        }

        println!();
        Ok(())
    }

    pub fn save(workspace_name: &str, message: &str, all: bool) -> Result<()> {
        let cfg = WorkspaceConfig::load()?;
        let entry = cfg.get(workspace_name)
            .ok_or_else(|| ToriiError::InvalidConfig(format!("Workspace '{}' not found", workspace_name)))?;

        let mut committed = 0;
        let mut skipped = 0;

        println!("📦 {} — saving", workspace_name);
        println!();

        for repo_path in &entry.repos {
            let name = Path::new(repo_path).file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| repo_path.clone());

            match Self::repo_save(repo_path, message, all) {
                Ok(true) => {
                    println!("  {} ✅ saved", name);
                    committed += 1;
                }
                Ok(false) => {
                    println!("  {} — no changes", name);
                    skipped += 1;
                }
                Err(e) => {
                    println!("  {} ❌ {}", name, e);
                }
            }
        }

        println!();
        println!("{} committed, {} skipped", committed, skipped);
        Ok(())
    }

    pub fn sync(workspace_name: &str, force: bool) -> Result<()> {
        let cfg = WorkspaceConfig::load()?;
        let entry = cfg.get(workspace_name)
            .ok_or_else(|| ToriiError::InvalidConfig(format!("Workspace '{}' not found", workspace_name)))?;

        println!("📦 {} — syncing", workspace_name);
        println!();

        let mut ok = 0;
        let mut failed = 0;

        for repo_path in &entry.repos {
            let name = Path::new(repo_path).file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| repo_path.clone());

            match Self::repo_sync(repo_path, force) {
                Ok(()) => {
                    println!("  {} ✅ synced", name);
                    ok += 1;
                }
                Err(e) => {
                    println!("  {} ❌ {}", name, e);
                    failed += 1;
                }
            }
        }

        println!();
        println!("{} synced, {} failed", ok, failed);
        Ok(())
    }

    pub fn list() -> Result<()> {
        let cfg = WorkspaceConfig::load()?;

        if cfg.workspace.is_empty() {
            println!("No workspaces configured.");
            println!("Add one: torii workspace add <name> <path>");
            return Ok(());
        }

        for (name, entry) in &cfg.workspace {
            println!("📦 {}", name);
            for repo in &entry.repos {
                let exists = Path::new(repo).exists();
                let icon = if exists { "  ✓" } else { "  ✗" };
                println!("{} {}", icon, repo);
            }
            println!();
        }

        Ok(())
    }

    pub fn add(workspace: &str, repo_path: &str) -> Result<()> {
        let mut cfg = WorkspaceConfig::load()?;
        let expanded = expand_path(repo_path)?;

        if !expanded.exists() {
            return Err(ToriiError::InvalidConfig(format!("Path does not exist: {}", expanded.display())).into());
        }

        cfg.add_repo(workspace, repo_path)?;
        cfg.save()?;

        println!("✅ Added {} to workspace '{}'", expanded.display(), workspace);
        Ok(())
    }

    pub fn remove(workspace: &str, repo_path: &str) -> Result<()> {
        let mut cfg = WorkspaceConfig::load()?;
        cfg.remove_repo(workspace, repo_path)?;
        cfg.save()?;
        println!("✅ Removed {} from workspace '{}'", repo_path, workspace);
        Ok(())
    }

    pub fn delete(workspace: &str) -> Result<()> {
        let mut cfg = WorkspaceConfig::load()?;
        if cfg.workspace.remove(workspace).is_none() {
            return Err(ToriiError::InvalidConfig(format!("Workspace '{}' not found", workspace)).into());
        }
        cfg.save()?;
        println!("✅ Deleted workspace '{}'", workspace);
        Ok(())
    }

    fn repo_status(repo_path: &str) -> Result<RepoStatus> {
        let name = Path::new(repo_path).file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| repo_path.to_string());

        let repo = git2::Repository::discover(repo_path)
            .map_err(|_| ToriiError::InvalidConfig(format!("Not a git repo: {}", repo_path)))?;

        let branch = repo.head().ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .unwrap_or_else(|| "detached".to_string());

        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        let statuses = repo.statuses(Some(&mut opts))
            .map_err(|e| ToriiError::Git(e))?;

        let mut staged = 0usize;
        let mut unstaged = 0usize;
        let mut untracked = 0usize;

        for entry in statuses.iter() {
            let s = entry.status();
            if s.intersects(
                git2::Status::INDEX_NEW | git2::Status::INDEX_MODIFIED |
                git2::Status::INDEX_DELETED | git2::Status::INDEX_RENAMED
            ) { staged += 1; }
            if s.intersects(
                git2::Status::WT_MODIFIED | git2::Status::WT_DELETED | git2::Status::WT_RENAMED
            ) { unstaged += 1; }
            if s.contains(git2::Status::WT_NEW) { untracked += 1; }
        }

        // Ahead/behind vs origin
        let (ahead, behind) = Self::ahead_behind(&repo, &branch).unwrap_or((0, 0));

        Ok(RepoStatus { path: repo_path.to_string(), name, branch, ahead, behind, staged, unstaged, untracked, error: None })
    }

    fn ahead_behind(repo: &git2::Repository, branch: &str) -> Option<(usize, usize)> {
        let local_ref = format!("refs/heads/{}", branch);
        let remote_ref = format!("refs/remotes/origin/{}", branch);
        let local = repo.find_reference(&local_ref).ok()?.target()?;
        let remote = repo.find_reference(&remote_ref).ok()?.target()?;
        repo.graph_ahead_behind(local, remote).ok()
    }

    fn repo_save(repo_path: &str, message: &str, all: bool) -> Result<bool> {
        let repo = crate::core::GitRepo::open(repo_path)?;

        // Check for changes
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(false);
        let statuses = repo.repository().statuses(Some(&mut opts))
            .map_err(|e| ToriiError::Git(e))?;

        if statuses.is_empty() {
            return Ok(false);
        }

        if all {
            repo.add_all()?;
        }

        // Re-check after staging
        let mut index = repo.repository().index()
            .map_err(|e| ToriiError::Git(e))?;
        index.read(true).map_err(|e| ToriiError::Git(e))?;
        let tree_oid = index.write_tree().map_err(|e| ToriiError::Git(e))?;

        // Check if there's actually something staged
        let head_tree = repo.repository().head().ok()
            .and_then(|h| h.peel_to_tree().ok());
        if let Some(head) = head_tree {
            if head.id() == tree_oid {
                return Ok(false);
            }
        }

        repo.commit(message)?;
        Ok(true)
    }

    fn repo_sync(repo_path: &str, force: bool) -> Result<()> {
        let repo = crate::core::GitRepo::open(repo_path)?;
        repo.pull()?;
        repo.push(force)?;
        Ok(())
    }
}
