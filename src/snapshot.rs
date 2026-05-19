use std::path::{Path, PathBuf};
use std::fs;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::error::{Result, ToriiError};
use crate::core::GitRepo;

/// Recursive directory copy used by the legacy-snapshot migration when
/// a cross-filesystem `fs::rename` fails. Best-effort — propagates
/// errors so callers can decide whether to abort or skip the entry.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let src_p = entry.path();
        let dst_p = dst.join(entry.file_name());
        if kind.is_dir() {
            copy_dir_all(&src_p, &dst_p)?;
        } else {
            fs::copy(&src_p, &dst_p)?;
        }
    }
    Ok(())
}

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

        // 0.7.7: snapshots live INSIDE the gitdir (.git/torii/snapshots/)
        // rather than in the working tree (.torii/snapshots/). The old
        // location was traversed by `torii save -a` because nothing put
        // `.torii/` in .gitignore, so a 681 MB working-tree snapshot got
        // committed and pushed in the wild. Putting snapshots under the
        // gitdir mirrors how git itself stores private state (hooks,
        // refs, objects) where `git add` never reaches. See
        // docs/fixed/BUG_SNAPSHOT_LEAKS_INTO_COMMITS.md.
        let gitdir = git2::Repository::discover(&repo_path)
            .map_err(crate::error::ToriiError::Git)?
            .path()
            .to_path_buf();
        let snapshots_dir = gitdir.join("torii").join("snapshots");
        fs::create_dir_all(&snapshots_dir)?;

        // One-shot migration: pull any pre-0.7.7 snapshots out of the
        // working tree into the new gitdir location. Idempotent — runs
        // only if the old dir has entries.
        let old_dir = repo_path.join(".torii").join("snapshots");
        if old_dir.exists() && old_dir != snapshots_dir {
            Self::migrate_legacy_snapshots(&old_dir, &snapshots_dir)?;
        }

        Ok(Self {
            repo_path,
            snapshots_dir,
        })
    }

    /// Move every `<old>/<id>/` directory into `<new>/<id>/`. Skips
    /// destinations that already exist. Removes the old parent if it
    /// ends up empty. Best-effort: copies on cross-FS rename failure,
    /// never aborts the caller.
    fn migrate_legacy_snapshots(old: &Path, new: &Path) -> Result<()> {
        let entries: Vec<PathBuf> = match fs::read_dir(old) {
            Ok(it) => it.flatten().map(|e| e.path()).collect(),
            Err(_) => return Ok(()),
        };
        if entries.is_empty() {
            return Ok(());
        }
        eprintln!("ℹ Migrating {} snapshot(s) from {} → {}",
                  entries.len(), old.display(), new.display());
        for src in entries {
            let name = match src.file_name() { Some(n) => n.to_owned(), None => continue };
            let dst = new.join(&name);
            if dst.exists() { continue; }
            if fs::rename(&src, &dst).is_err() {
                // Cross-FS or busy: fall back to copy + remove, never abort.
                if copy_dir_all(&src, &dst).is_ok() {
                    let _ = fs::remove_dir_all(&src);
                }
            }
        }
        // Best-effort cleanup of the now-empty .torii/snapshots/ and
        // (if empty) .torii/ parent. Failure is fine — config.json or
        // mirrors.json may still live there legitimately.
        let _ = fs::remove_dir(old);
        if let Some(parent) = old.parent() {
            let _ = fs::remove_dir(parent);
        }
        Ok(())
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

        let git_path = self.repo_path.join(".git");
        let snapshot_git = snapshot_dir.join("git_backup");

        // .git is normally a directory (regular checkout). In linked
        // worktrees and submodules it's a regular file whose first line is
        // "gitdir: <path-to-real-gitdir>" pointing at the metadata that
        // actually lives elsewhere — shared with the main repo. In that
        // case copying the file alone preserves the link; the worktree's
        // working-tree content gets copied below alongside it. We do NOT
        // duplicate the linked gitdir because (a) it's shared and (b) the
        // worktree's unique state lives in the working tree.
        // Exclude our own state directory (<gitdir>/torii/) from the
        // .git copy. Since 0.7.7 snapshots live INSIDE the gitdir, so
        // a naive recursive copy of `.git/` into `.git/torii/snapshots/<id>/git_backup`
        // would walk into its own destination forever. The torii/
        // subdir of the gitdir is tool-private state — never useful to
        // include inside a snapshot of itself.
        let torii_state = git_path.join("torii");
        match fs::symlink_metadata(&git_path) {
            Ok(meta) if meta.is_dir() => {
                self.copy_dir_recursive_excluding(&git_path, &snapshot_git, Some(&torii_state))?;
            }
            Ok(_) => {
                // .git is a file (worktree / submodule gitlink). Copy the
                // single file so we know which gitdir this was tied to,
                // then leave the rest alone.
                fs::create_dir_all(&snapshot_git)?;
                fs::copy(&git_path, snapshot_git.join("gitdir-link"))?;
                // Also dump the resolved gitdir path so restoration knows
                // where the real metadata lived.
                if let Ok(content) = fs::read_to_string(&git_path) {
                    let pointer = content.trim();
                    fs::write(snapshot_git.join("RESOLVED-GITDIR"), pointer)?;
                }
            }
            Err(e) => {
                return Err(ToriiError::Io(e));
            }
        }

        Ok(())
    }

    /// Recursively copy `src` into `dst`. Skips any path equal to
    /// `exclude` (matched by canonical-path comparison when both
    /// resolve), so a `.git` copy can be written into a destination
    /// inside `.git/` itself without recursing forever. Used by
    /// `create_bundle` since 0.7.7 — snapshots now live at
    /// `<gitdir>/torii/snapshots/`, which is inside the source of the
    /// bundle's git-dir copy.
    fn copy_dir_recursive(&self, src: &Path, dst: &Path) -> Result<()> {
        self.copy_dir_recursive_excluding(src, dst, None)
    }

    fn copy_dir_recursive_excluding(&self, src: &Path, dst: &Path, exclude: Option<&Path>) -> Result<()> {
        fs::create_dir_all(dst)?;

        // Canonicalise the exclude path once. If canonicalisation fails
        // (path may not exist yet, e.g. dst itself), fall back to the
        // raw form.
        let excl_canon = exclude.map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let src_path = entry.path();

            // Compare canonical paths so the exclusion catches both
            // "src/torii" entered via `.git/torii` and via a symlink.
            if let Some(ref excl) = excl_canon {
                let src_canon = src_path.canonicalize().unwrap_or_else(|_| src_path.clone());
                if &src_canon == excl {
                    continue;
                }
            }

            let dst_path = dst.join(entry.file_name());
            if file_type.is_dir() {
                self.copy_dir_recursive_excluding(&src_path, &dst_path, exclude)?;
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

    /// Delete every snapshot in this repo. Returns the count deleted so
    /// the CLI can report it. Idempotent — empty dir returns 0.
    pub fn clear_all(&self) -> Result<usize> {
        if !self.snapshots_dir.exists() {
            return Ok(0);
        }
        let mut count = 0;
        for entry in fs::read_dir(&self.snapshots_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Print everything we have on one snapshot: metadata + bundle layout.
    /// Doesn't try to list every file under git_backup (could be huge);
    /// shows the top-level entries so the user knows it's a real backup.
    pub fn show(&self, id: &str) -> Result<()> {
        let snapshot_dir = self.snapshots_dir.join(id);
        if !snapshot_dir.exists() {
            return Err(ToriiError::Snapshot(format!("Snapshot not found: {}", id)));
        }
        let metadata_path = snapshot_dir.join("metadata.json");
        if metadata_path.exists() {
            let metadata_json = fs::read_to_string(&metadata_path)?;
            let metadata: SnapshotMetadata = serde_json::from_str(&metadata_json)?;
            println!("📸 Snapshot {}", metadata.id);
            println!("   timestamp: {}", metadata.timestamp.format("%Y-%m-%d %H:%M:%S"));
            if let Some(name) = &metadata.name {
                println!("   name:      {}", name);
            }
            println!("   branch:    {}", metadata.branch);
            if let Some(commit) = &metadata.commit_hash {
                println!("   commit:    {}", commit);
            }
        } else {
            println!("📸 Snapshot {} (no metadata.json — likely partial)", id);
        }
        // Show what's captured inside.
        println!("   contents:");
        for entry in fs::read_dir(&snapshot_dir)? {
            let entry = entry?;
            let kind = if entry.file_type()?.is_dir() { "dir" } else { "file" };
            println!("     {kind}: {}", entry.file_name().to_string_lossy());
        }
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

        // Use the unified resolver: torii config > git config > error.
        // The previous "torii"/"torii@local" placeholder fallback (so
        // stash "never fails") matched the same anti-pattern that
        // BUG_COMMIT_AUTHOR_FALLBACK.md describes for `save`. Silent
        // bogus authorship is worse than failing fast and prompting the
        // user to set their identity — even for stashes.
        let signature = crate::core::resolve_signature(&repo)?;

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

#[cfg(test)]
mod snapshot_location_tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo(dir: &Path) {
        let repo = git2::Repository::init(dir).unwrap();
        // SnapshotManager needs HEAD to resolve a branch — make an
        // empty commit so get_current_branch() doesn't blow up.
        let sig = git2::Signature::now("T", "t@x").unwrap();
        let mut idx = repo.index().unwrap();
        let tree_oid = idx.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
    }

    #[test]
    fn snapshots_land_under_gitdir_not_working_tree() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path();
        init_repo(repo_path);

        let mgr = SnapshotManager::new(repo_path).unwrap();
        let id = mgr.create_snapshot(Some("test")).unwrap();

        // Must exist inside .git/torii/snapshots/, NOT .torii/snapshots/
        let new_loc = repo_path.join(".git/torii/snapshots").join(&id);
        let old_loc = repo_path.join(".torii/snapshots").join(&id);
        assert!(new_loc.exists(), "snapshot should be at .git/torii/snapshots/{}", id);
        assert!(!old_loc.exists(), "snapshot must NOT be in working tree at .torii/snapshots/{}", id);
    }

    #[test]
    fn migrates_legacy_snapshots_from_working_tree_to_gitdir() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path();
        init_repo(repo_path);

        // Seed the legacy location with a fake snapshot dir.
        let legacy = repo_path.join(".torii/snapshots/20200101_000000_000");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("metadata.json"), "{}").unwrap();

        // Constructing the manager triggers migration.
        let _mgr = SnapshotManager::new(repo_path).unwrap();

        let new_loc = repo_path.join(".git/torii/snapshots/20200101_000000_000");
        assert!(new_loc.exists(), "legacy snapshot should be migrated");
        assert!(new_loc.join("metadata.json").exists(), "files inside should come along");
        assert!(!legacy.exists(), "legacy location should be cleaned up");
    }

    #[test]
    fn migration_is_idempotent_when_destination_exists() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path();
        init_repo(repo_path);

        // Same id pre-exists in both locations: migration should not
        // overwrite the new one (preserves whatever 0.7.7+ wrote).
        let id = "20200101_000000_000";
        let legacy = repo_path.join(".torii/snapshots").join(id);
        let new_loc = repo_path.join(".git/torii/snapshots").join(id);
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&new_loc).unwrap();
        fs::write(legacy.join("source.json"), "legacy").unwrap();
        fs::write(new_loc.join("source.json"), "new").unwrap();

        let _mgr = SnapshotManager::new(repo_path).unwrap();

        // New location's content is preserved (not clobbered by legacy).
        let content = fs::read_to_string(new_loc.join("source.json")).unwrap();
        assert_eq!(content, "new");
    }
}
