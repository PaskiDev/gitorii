use crate::error::{Result, ToriiError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
            .ok_or_else(|| {
                ToriiError::InvalidConfig("Could not determine config directory".to_string())
            })?
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
        toml::from_str(&s)
            .map_err(|e| ToriiError::Workspace(format!("Failed to parse workspaces.toml: {}", e)))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        let s = toml::to_string_pretty(self)
            .map_err(|e| ToriiError::Workspace(format!("Failed to serialize workspaces: {}", e)))?;
        fs::write(&path, s)?;
        Ok(())
    }

    pub fn add_repo(&mut self, workspace: &str, repo_path: &str) -> Result<()> {
        let expanded = expand_path(repo_path)?;
        let entry = self
            .workspace
            .entry(workspace.to_string())
            .or_insert(WorkspaceEntry { repos: vec![] });
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
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or_else(|| {
            ToriiError::InvalidConfig("Could not determine home directory".to_string())
        })?;
        Ok(home.join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

pub struct WorkspaceManager;

/// Per-repo status inside a workspace sweep. When the repo couldn't be
/// inspected, `error` is set and the numeric fields are zeroed.
#[derive(Debug, Serialize)]
pub struct WorkspaceRepoStatus {
    pub path: String,
    pub name: String,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub error: Option<String>,
}

/// Outcome of saving one repo in a workspace sweep.
#[derive(Debug, Serialize)]
pub enum SaveOutcome {
    Saved,
    NoChanges,
    Failed(String),
}

#[derive(Debug, Serialize)]
pub struct WorkspaceSaveResult {
    pub name: String,
    pub outcome: SaveOutcome,
}

/// Outcome of syncing one repo in a workspace sweep — `error` is `None`
/// on success.
#[derive(Debug, Serialize)]
pub struct WorkspaceSyncResult {
    pub name: String,
    pub error: Option<String>,
}

/// One workspace with the existence state of each member repo.
#[derive(Debug, Serialize)]
pub struct WorkspaceListEntry {
    pub name: String,
    pub repos: Vec<WorkspaceRepoEntry>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceRepoEntry {
    pub path: String,
    pub exists: bool,
}

fn repo_display_name(repo_path: &str) -> String {
    Path::new(repo_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_path.to_string())
}

impl WorkspaceManager {
    /// Status of every repo in the workspace. Per-repo failures land in
    /// the entry's `error` field instead of aborting the sweep.
    pub fn status(workspace_name: &str) -> Result<Vec<WorkspaceRepoStatus>> {
        let cfg = WorkspaceConfig::load()?;
        let entry = cfg.get(workspace_name).ok_or_else(|| {
            ToriiError::Workspace(format!("Workspace '{}' not found", workspace_name))
        })?;

        Ok(entry
            .repos
            .iter()
            .map(|repo_path| match Self::repo_status(repo_path) {
                Ok(s) => s,
                Err(e) => WorkspaceRepoStatus {
                    path: repo_path.clone(),
                    name: repo_display_name(repo_path),
                    branch: String::new(),
                    ahead: 0,
                    behind: 0,
                    staged: 0,
                    unstaged: 0,
                    untracked: 0,
                    error: Some(e.to_string()),
                },
            })
            .collect())
    }

    /// Commit pending changes in every repo of the workspace. `on_repo`
    /// fires as each repo finishes so callers can stream progress (CLI
    /// prints a line, the IDE emits an event).
    pub fn save(
        workspace_name: &str,
        message: &str,
        all: bool,
        mut on_repo: impl FnMut(&WorkspaceSaveResult),
    ) -> Result<Vec<WorkspaceSaveResult>> {
        let cfg = WorkspaceConfig::load()?;
        let entry = cfg.get(workspace_name).ok_or_else(|| {
            ToriiError::Workspace(format!("Workspace '{}' not found", workspace_name))
        })?;

        Ok(entry
            .repos
            .iter()
            .map(|repo_path| {
                let result = WorkspaceSaveResult {
                    name: repo_display_name(repo_path),
                    outcome: match Self::repo_save(repo_path, message, all) {
                        Ok(true) => SaveOutcome::Saved,
                        Ok(false) => SaveOutcome::NoChanges,
                        Err(e) => SaveOutcome::Failed(e.to_string()),
                    },
                };
                on_repo(&result);
                result
            })
            .collect())
    }

    /// Pull + push every repo of the workspace. `on_repo` fires as each
    /// repo finishes so callers can stream progress.
    pub fn sync(
        workspace_name: &str,
        force: bool,
        mut on_repo: impl FnMut(&WorkspaceSyncResult),
    ) -> Result<Vec<WorkspaceSyncResult>> {
        let cfg = WorkspaceConfig::load()?;
        let entry = cfg.get(workspace_name).ok_or_else(|| {
            ToriiError::Workspace(format!("Workspace '{}' not found", workspace_name))
        })?;

        Ok(entry
            .repos
            .iter()
            .map(|repo_path| {
                let result = WorkspaceSyncResult {
                    name: repo_display_name(repo_path),
                    error: Self::repo_sync(repo_path, force)
                        .err()
                        .map(|e| e.to_string()),
                };
                on_repo(&result);
                result
            })
            .collect())
    }

    /// Every configured workspace with the on-disk existence of its repos.
    pub fn list() -> Result<Vec<WorkspaceListEntry>> {
        let cfg = WorkspaceConfig::load()?;
        Ok(cfg
            .workspace
            .iter()
            .map(|(name, entry)| WorkspaceListEntry {
                name: name.clone(),
                repos: entry
                    .repos
                    .iter()
                    .map(|repo| WorkspaceRepoEntry {
                        path: repo.clone(),
                        exists: Path::new(repo).exists(),
                    })
                    .collect(),
            })
            .collect())
    }

    /// Register a repo in a workspace. Returns the expanded path.
    pub fn add(workspace: &str, repo_path: &str) -> Result<PathBuf> {
        let mut cfg = WorkspaceConfig::load()?;
        let expanded = expand_path(repo_path)?;

        if !expanded.exists() {
            return Err(ToriiError::Usage(format!(
                "Path does not exist: {}",
                expanded.display()
            )));
        }

        cfg.add_repo(workspace, repo_path)?;
        cfg.save()?;
        Ok(expanded)
    }

    pub fn remove(workspace: &str, repo_path: &str) -> Result<()> {
        let mut cfg = WorkspaceConfig::load()?;
        cfg.remove_repo(workspace, repo_path)?;
        cfg.save()?;
        Ok(())
    }

    pub fn delete(workspace: &str) -> Result<()> {
        let mut cfg = WorkspaceConfig::load()?;
        if cfg.workspace.remove(workspace).is_none() {
            return Err(ToriiError::Workspace(format!(
                "Workspace '{}' not found",
                workspace
            )));
        }
        cfg.save()?;
        Ok(())
    }

    fn repo_status(repo_path: &str) -> Result<WorkspaceRepoStatus> {
        let name = repo_display_name(repo_path);

        let repo = git2::Repository::discover(repo_path)
            .map_err(|_| ToriiError::Usage(format!("Not a git repo: {}", repo_path)))?;

        let branch = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .unwrap_or_else(|| "detached".to_string());

        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        let statuses = repo.statuses(Some(&mut opts)).map_err(ToriiError::Git)?;

        let mut staged = 0usize;
        let mut unstaged = 0usize;
        let mut untracked = 0usize;

        for entry in statuses.iter() {
            let s = entry.status();
            if s.intersects(
                git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_DELETED
                    | git2::Status::INDEX_RENAMED,
            ) {
                staged += 1;
            }
            if s.intersects(
                git2::Status::WT_MODIFIED | git2::Status::WT_DELETED | git2::Status::WT_RENAMED,
            ) {
                unstaged += 1;
            }
            if s.contains(git2::Status::WT_NEW) {
                untracked += 1;
            }
        }

        // Ahead/behind vs origin
        let (ahead, behind) = Self::ahead_behind(&repo, &branch).unwrap_or((0, 0));

        Ok(WorkspaceRepoStatus {
            path: repo_path.to_string(),
            name,
            branch,
            ahead,
            behind,
            staged,
            unstaged,
            untracked,
            error: None,
        })
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
        let statuses = repo
            .repository()
            .statuses(Some(&mut opts))
            .map_err(ToriiError::Git)?;

        if statuses.is_empty() {
            return Ok(false);
        }

        if all {
            repo.add_all()?;
        }

        // Re-check after staging
        let mut index = repo.repository().index().map_err(ToriiError::Git)?;
        index.read(true).map_err(ToriiError::Git)?;
        let tree_oid = index.write_tree().map_err(ToriiError::Git)?;

        // Check if there's actually something staged
        let head_tree = repo
            .repository()
            .head()
            .ok()
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
