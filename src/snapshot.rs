use std::path::{Path, PathBuf};
use std::fs;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::error::{Result, ToriiError};
use crate::core::GitRepo;

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub name: Option<String>,
    pub branch: String,
    pub commit_hash: Option<String>,
}

pub struct SnapshotManager {
    repo_path: PathBuf,
    snapshots_dir: PathBuf,
}

impl SnapshotManager {
    pub fn new<P: AsRef<Path>>(repo_path: P) -> Result<Self> {
        let repo_path = repo_path.as_ref().to_path_buf();
        let snapshots_dir = repo_path.join(".torii").join("snapshots");
        
        fs::create_dir_all(&snapshots_dir)?;

        Ok(Self {
            repo_path,
            snapshots_dir,
        })
    }

    /// Create a new snapshot
    pub fn create_snapshot(&self, name: Option<&str>) -> Result<String> {
        let repo = GitRepo::open(&self.repo_path)?;
        let timestamp = Utc::now();
        // Include millis so back-to-back snapshots in the same second don't
        // collide and silently overwrite each other (the original `_HMS`
        // format made `stash` lose data when invoked twice quickly).
        let mut id = timestamp.format("%Y%m%d_%H%M%S_%3f").to_string();

        // Defensive: if even the millis collide (highly unlikely), append
        // an integer suffix until the dir is fresh.
        let mut snapshot_dir = self.snapshots_dir.join(&id);
        let mut suffix = 0;
        while snapshot_dir.exists() {
            suffix += 1;
            id = format!("{}_{}", timestamp.format("%Y%m%d_%H%M%S_%3f"), suffix);
            snapshot_dir = self.snapshots_dir.join(&id);
        }
        fs::create_dir_all(&snapshot_dir)?;

        let branch = repo.get_current_branch()?;
        
        let metadata = SnapshotMetadata {
            id: id.clone(),
            timestamp,
            name: name.map(String::from),
            branch,
            commit_hash: None,
        };

        let metadata_path = snapshot_dir.join("metadata.json");
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        fs::write(metadata_path, metadata_json)?;

        self.create_bundle(&snapshot_dir, &repo)?;

        Ok(id)
    }

    /// Create a git bundle for the snapshot
    fn create_bundle(&self, snapshot_dir: &Path, repo: &GitRepo) -> Result<()> {
        // Create bundle with all refs
        let mut revwalk = repo.repository().revwalk()?;
        revwalk.push_head()?;
        
        // For now, we'll use a simpler approach: copy the entire .git directory
        // In production, we'd use proper git bundle creation
        let git_dir = self.repo_path.join(".git");
        let snapshot_git = snapshot_dir.join("git_backup");
        
        self.copy_dir_recursive(&git_dir, &snapshot_git)?;

        Ok(())
    }

    /// Recursively copy directory
    fn copy_dir_recursive(&self, src: &Path, dst: &Path) -> Result<()> {
        fs::create_dir_all(dst)?;
        
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if file_type.is_dir() {
                self.copy_dir_recursive(&src_path, &dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path)?;
            }
        }

        Ok(())
    }

    /// List all snapshots
    pub fn list_snapshots(&self) -> Result<()> {
        let entries = fs::read_dir(&self.snapshots_dir)?;
        
        println!("📸 Snapshots:");
        println!();

        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let metadata_path = entry.path().join("metadata.json");
                if metadata_path.exists() {
                    let metadata_json = fs::read_to_string(metadata_path)?;
                    let metadata: SnapshotMetadata = serde_json::from_str(&metadata_json)?;
                    
                    let name_str = metadata.name
                        .as_ref()
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default();
                    
                    println!("  {} - {}{}", 
                        metadata.id,
                        metadata.timestamp.format("%Y-%m-%d %H:%M:%S"),
                        name_str
                    );
                    println!("    Branch: {}", metadata.branch);
                }
            }
        }

        Ok(())
    }

    /// Restore from a snapshot
    pub fn restore_snapshot(&self, id: &str) -> Result<()> {
        let snapshot_dir = self.snapshots_dir.join(id);
        
        if !snapshot_dir.exists() {
            return Err(ToriiError::Snapshot(format!("Snapshot not found: {}", id)));
        }

        let snapshot_git = snapshot_dir.join("git_backup");
        let git_dir = self.repo_path.join(".git");

        fs::remove_dir_all(&git_dir)?;
        self.copy_dir_recursive(&snapshot_git, &git_dir)?;

        // Reset working directory to match restored git state via git2
        {
            let repo = git2::Repository::discover(&self.repo_path)
                .map_err(|e| ToriiError::Git(e))?;
            let head = repo.head()
                .map_err(|e| ToriiError::Git(e))?
                .peel_to_commit()
                .map_err(|e| ToriiError::Git(e))?;
            repo.reset(
                head.as_object(),
                git2::ResetType::Hard,
                Some(git2::build::CheckoutBuilder::default().force()),
            ).map_err(|e| ToriiError::Git(e))?;
        }

        Ok(())
    }

    /// Delete a snapshot
    pub fn delete_snapshot(&self, id: &str) -> Result<()> {
        let snapshot_dir = self.snapshots_dir.join(id);
        
        if !snapshot_dir.exists() {
            return Err(ToriiError::Snapshot(format!("Snapshot not found: {}", id)));
        }

        fs::remove_dir_all(snapshot_dir)?;
        Ok(())
    }

    /// Configure auto-snapshot settings
    pub fn configure_auto_snapshot(&self, enable: bool, interval: Option<u32>) -> Result<()> {
        let config_path = self.repo_path.join(".torii").join("config.json");
        
        #[derive(Serialize, Deserialize)]
        struct Config {
            auto_snapshot_enabled: bool,
            auto_snapshot_interval_minutes: u32,
        }

        let config = Config {
            auto_snapshot_enabled: enable,
            auto_snapshot_interval_minutes: interval.unwrap_or(30),
        };

        let config_json = serde_json::to_string_pretty(&config)?;
        fs::write(config_path, config_json)?;

        Ok(())
    }

    /// Save work temporarily (like git stash).
    ///
    /// Uses libgit2's native stash API rather than the snapshot bundle path.
    /// The previous implementation copied `.git/` and reset HEAD, which
    /// silently dropped working-tree changes — `git_backup` only contains
    /// committed history, so any uncommitted edits were unrecoverable.
    pub fn stash(&self, name: Option<&str>, include_untracked: bool) -> Result<()> {
        let stash_name = name.unwrap_or("WIP");
        let mut repo = git2::Repository::discover(&self.repo_path)
            .map_err(ToriiError::Git)?;

        // Detect whether there is anything to stash; libgit2 errors with
        // "no changes selected" otherwise and the message is unhelpful.
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(include_untracked)
            .recurse_untracked_dirs(include_untracked);
        let is_empty = {
            let statuses = repo.statuses(Some(&mut opts)).map_err(ToriiError::Git)?;
            statuses.is_empty()
        };
        if is_empty {
            return Err(ToriiError::Snapshot(
                "Nothing to stash — working tree is clean.".to_string(),
            ));
        }

        // Build signature; if user.name/email aren't configured fall back to
        // a generic identity so stash never fails purely on a missing config.
        let signature = repo.signature().or_else(|_| {
            git2::Signature::now("torii", "torii@local")
        }).map_err(ToriiError::Git)?;

        let mut flags = git2::StashFlags::DEFAULT;
        if include_untracked {
            flags |= git2::StashFlags::INCLUDE_UNTRACKED;
        }
        let oid = repo.stash_save2(&signature, Some(stash_name), Some(flags))
            .map_err(ToriiError::Git)?;

        println!("📦 Stashed changes");
        println!("   stash@{{0}}: {}", &oid.to_string()[..7]);
        println!("   Name: {}", stash_name);
        if include_untracked {
            println!("   Untracked files included");
        }
        println!();
        println!("💡 To restore: torii snapshot unstash");

        Ok(())
    }

    /// Restore stashed work via libgit2's native stash API.
    /// `id` selects which stash entry: `"0"` (default) is the most recent,
    /// `"1"` the one before, etc. `keep` retains the stash entry after apply.
    pub fn unstash(&self, id: Option<&str>, keep: bool) -> Result<()> {
        let mut repo = git2::Repository::discover(&self.repo_path)
            .map_err(ToriiError::Git)?;

        let index: usize = match id {
            Some(s) => s.trim_start_matches("stash@{").trim_end_matches('}')
                .parse()
                .map_err(|_| ToriiError::Snapshot(
                    format!("invalid stash index `{}` (use a number: 0, 1, …)", s)
                ))?,
            None => 0,
        };

        // Confirm the entry exists for a friendlier error than libgit2's.
        let mut count = 0;
        repo.stash_foreach(|_, _, _| { count += 1; true }).map_err(ToriiError::Git)?;
        if count == 0 {
            return Err(ToriiError::Snapshot("No stash found".to_string()));
        }
        if index >= count {
            return Err(ToriiError::Snapshot(format!(
                "stash@{{{}}} doesn't exist (have {} stash{})", index, count,
                if count == 1 { "" } else { "es" }
            )));
        }

        println!("🔄 Restoring stash@{{{}}}", index);
        if keep {
            let mut opts = git2::StashApplyOptions::new();
            opts.reinstantiate_index();
            repo.stash_apply(index, Some(&mut opts)).map_err(ToriiError::Git)?;
            println!("   Stash kept (use `torii snapshot unstash {} --no-keep` to drop)", index);
        } else {
            repo.stash_pop(index, None).map_err(ToriiError::Git)?;
            println!("   Stash popped");
        }
        println!("✅ Stash restored");

        Ok(())
    }

    /// Undo last operation
    pub fn undo(&self) -> Result<()> {
        // Find most recent auto snapshot
        let mut snapshots: Vec<_> = fs::read_dir(&self.snapshots_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("before-") || name.contains("auto-")
            })
            .collect();
        
        snapshots.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));
        
        let latest = snapshots.last()
            .ok_or_else(|| ToriiError::Snapshot("No operation to undo".to_string()))?;
        
        let snapshot_id = latest.file_name().to_string_lossy().to_string();
        
        println!("🔄 Undoing last operation...");
        println!("   Restoring snapshot: {}", snapshot_id);
        
        self.restore_snapshot(&snapshot_id)?;
        
        println!("✅ Operation undone");
        
        Ok(())
    }
}
