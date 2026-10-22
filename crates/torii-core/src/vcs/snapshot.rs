use crate::core::GitRepo;
use crate::error::{Result, ToriiError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

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

fn tree_blobs(repo: &git2::Repository, tree: &git2::Tree) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut blobs: Vec<(String, git2::Oid)> = Vec::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            if let Some(name) = entry.name() {
                blobs.push((format!("{dir}{name}"), entry.id()));
            }
        }
        0
    })
    .map_err(ToriiError::Git)?;
    let mut files = BTreeMap::new();
    for (path, oid) in blobs {
        let blob = repo.find_blob(oid).map_err(ToriiError::Git)?;
        files.insert(path, blob.content().to_vec());
    }
    Ok(files)
}

fn overlay_dir(dir: &Path, prefix: &str, into: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let rel = if prefix.is_empty() {
            name.into_owned()
        } else {
            format!("{prefix}/{name}")
        };
        let p = entry.path();
        if p.is_dir() {
            overlay_dir(&p, &rel, into)?;
        } else if p.is_file() {
            into.insert(rel, fs::read(&p)?);
        }
    }
    Ok(())
}

fn append_file_diff(patch: &mut String, path: &str, old: &[u8], new: &[u8]) -> Result<()> {
    if old.contains(&0) || new.contains(&0) {
        patch.push_str(&format!("diff --git a/{path} b/{path}\n"));
        patch.push_str("Binary files differ\n");
        return Ok(());
    }
    let mut opts = git2::DiffOptions::new();
    let mut file_patch = git2::Patch::from_buffers(
        old,
        Some(Path::new(path)),
        new,
        Some(Path::new(path)),
        Some(&mut opts),
    )
    .map_err(ToriiError::Git)?;
    let buf = file_patch.to_buf().map_err(ToriiError::Git)?;
    patch.push_str(&String::from_utf8_lossy(&buf));
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotDiffKind {
    Added,
    Removed,
    Changed,
    Binary,
}

impl SnapshotDiffKind {
    pub fn mark(self) -> &'static str {
        match self {
            Self::Added => "+",
            Self::Removed => "-",
            Self::Changed => "~",
            Self::Binary => "b",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotDiffFile {
    pub path: String,
    pub kind: SnapshotDiffKind,
    pub patch: String,
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
        eprintln!(
            "ℹ Migrating {} snapshot(s) from {} → {}",
            entries.len(),
            old.display(),
            new.display()
        );
        for src in entries {
            let name = match src.file_name() {
                Some(n) => n.to_owned(),
                None => continue,
            };
            let dst = new.join(&name);
            if dst.exists() {
                continue;
            }
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

    /// If `snapshot.auto_enabled` is on, take a snapshot named `reason`.
    /// The caller decides whether the tree is dirty enough to bother.
    pub fn maybe_auto(&self, reason: &str) -> Result<Option<String>> {
        let cfg = crate::config::ToriiConfig::load_local(&self.repo_path)?;
        if !cfg.snapshot.auto_enabled {
            return Ok(None);
        }
        Ok(Some(self.create_snapshot(Some(reason))?))
    }
}

/// Best-effort auto-snapshot from a live `git2::Repository`. Prints on
/// success; a failure is a warning, not a stop.
pub fn maybe_auto_snapshot_here(repo: &git2::Repository, reason: &str) {
    let Some(workdir) = repo.workdir() else {
        return;
    };
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let dirty = repo
        .statuses(Some(&mut opts))
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !dirty {
        return;
    }
    match SnapshotManager::new(workdir).and_then(|m| m.maybe_auto(reason)) {
        Ok(Some(id)) => println!(
            "📸 auto-snapshot {} (restore with: torii snapshot restore {})",
            id, id
        ),
        Ok(None) => {}
        Err(e) => eprintln!("⚠  auto-snapshot failed (proceeding): {e}"),
    }
}

impl SnapshotManager {
    /// Create a new snapshot
    pub fn create_snapshot(&self, name: Option<&str>) -> Result<String> {
        let repo = GitRepo::open(&self.repo_path)?;
        let (id, snapshot_dir) = self.new_snapshot_dir(&repo, name)?;

        self.create_bundle(&snapshot_dir, &repo)?;
        self.capture_workdir(&snapshot_dir, repo.repository())?;

        Ok(id)
    }

    /// Snapshot only `paths` (relative to the work tree root) as they are on
    /// disk now, into `worktree/` and with no `git_backup`. For an operation
    /// that overwrites a few files and leaves `.git` alone: a gitdir copy
    /// costs the whole repo per call, which filled 4.9 GB when a formatter's
    /// changes were reverted file by file.
    pub fn create_files_snapshot(&self, name: Option<&str>, paths: &[&str]) -> Result<String> {
        let repo = GitRepo::open(&self.repo_path)?;
        let (id, snapshot_dir) = self.new_snapshot_dir(&repo, name)?;

        let wt = snapshot_dir.join("worktree");
        for path in paths {
            let dst = wt.join(path);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(self.repo_path.join(path), &dst)?;
        }

        Ok(id)
    }

    /// Reserve a fresh `<snapshots>/<id>/` and write its metadata.
    fn new_snapshot_dir(&self, repo: &GitRepo, name: Option<&str>) -> Result<(String, PathBuf)> {
        let timestamp = Utc::now();
        // Include millis so back-to-back snapshots in the same second don't
        // collide and silently overwrite each other (the original `_HMS`
        // format made `stash` lose data when invoked twice quickly).
        let mut id = timestamp.format("%Y%m%d_%H%M%S_%3f").to_string();

        // Defensive: if even the millis collide (highly unlikely), append
        // an integer suffix until the dir is fresh. Use atomic create_dir
        // so two parallel `torii save` invocations can't both decide the
        // same dir is free and silently overwrite each other's bundle.
        fs::create_dir_all(&self.snapshots_dir)?;
        let mut snapshot_dir = self.snapshots_dir.join(&id);
        let mut suffix = 0;
        loop {
            match fs::create_dir(&snapshot_dir) {
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    suffix += 1;
                    id = format!("{}_{}", timestamp.format("%Y%m%d_%H%M%S_%3f"), suffix);
                    snapshot_dir = self.snapshots_dir.join(&id);
                }
                Err(e) => return Err(e.into()),
            }
        }

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

        Ok((id, snapshot_dir))
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

    fn copy_dir_recursive_excluding(
        &self,
        src: &Path,
        dst: &Path,
        exclude: Option<&Path>,
    ) -> Result<()> {
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

                    let name_str = metadata
                        .name
                        .as_ref()
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default();

                    println!(
                        "  {} - {}{}",
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

    /// Copy dirty / untracked workdir files into `snapshot_dir/worktree`
    /// and record deletions. `git_backup` is only `.git`, so without
    /// this a snapshot cannot restore uncommitted work.
    fn capture_workdir(&self, snapshot_dir: &Path, repo: &git2::Repository) -> Result<()> {
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        let statuses = repo.statuses(Some(&mut opts)).map_err(ToriiError::Git)?;
        let wt = snapshot_dir.join("worktree");
        let mut deleted = Vec::new();
        for entry in statuses.iter() {
            let Some(path) = entry.path() else { continue };
            let st = entry.status();
            if st.is_wt_deleted() || st.is_index_deleted() {
                deleted.push(path.to_string());
                continue;
            }
            let src = self.repo_path.join(path);
            if src.is_dir() {
                copy_dir_all(&src, &wt.join(path))?;
            } else if src.is_file() {
                let dst = wt.join(path);
                if let Some(parent) = dst.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&src, &dst)?;
            }
        }
        if !deleted.is_empty() {
            fs::write(snapshot_dir.join("deleted.txt"), deleted.join("\n"))?;
        }
        Ok(())
    }

    /// Restore from a snapshot.
    ///
    /// Snapshots live *inside* `.git/torii/snapshots/` since 0.7.7.
    /// The previous restore deleted `.git` wholesale, which deleted the
    /// snapshot it was restoring from and left an empty directory.
    pub fn restore_snapshot(&self, id: &str) -> Result<()> {
        let snapshot_dir = self.snapshots_dir.join(id);

        if !snapshot_dir.exists() {
            return Err(ToriiError::Snapshot(format!("Snapshot not found: {}", id)));
        }

        let snapshot_git = snapshot_dir.join("git_backup");
        let worktree = snapshot_dir.join("worktree");
        // A files snapshot: put its files back, leave `.git` alone.
        if !snapshot_git.exists() && worktree.is_dir() {
            self.copy_dir_recursive(&worktree, &self.repo_path)?;
            return Ok(());
        }
        if !snapshot_git.exists() {
            return Err(ToriiError::Snapshot(format!(
                "Snapshot {} has no git_backup",
                id
            )));
        }

        let git_dir = self.repo_path.join(".git");
        if !git_dir.is_dir() {
            return Err(ToriiError::RepoState(
                "restore needs a directory .git (linked worktrees are not supported yet)"
                    .to_string(),
            ));
        }

        // Copy the backup out from under .git before we rewrite .git.
        // Staging lives in torii/ so the wipe below leaves it alone.
        let staging = self
            .snapshots_dir
            .parent()
            .unwrap_or(&self.snapshots_dir)
            .join("restore-staging");
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        self.copy_dir_recursive(&snapshot_git, &staging)?;

        for entry in fs::read_dir(&git_dir)? {
            let entry = entry?;
            if entry.file_name() == "torii" {
                continue;
            }
            let p = entry.path();
            if p.is_dir() {
                fs::remove_dir_all(&p)?;
            } else {
                fs::remove_file(&p)?;
            }
        }
        self.copy_dir_recursive(&staging, &git_dir)?;
        let _ = fs::remove_dir_all(&staging);

        {
            let repo = git2::Repository::discover(&self.repo_path).map_err(ToriiError::Git)?;
            let head = repo
                .head()
                .map_err(ToriiError::Git)?
                .peel_to_commit()
                .map_err(ToriiError::Git)?;
            repo.reset(
                head.as_object(),
                git2::ResetType::Hard,
                Some(git2::build::CheckoutBuilder::default().force()),
            )
            .map_err(ToriiError::Git)?;
        }

        if worktree.exists() {
            self.copy_dir_recursive(&worktree, &self.repo_path)?;
        }
        let deleted = snapshot_dir.join("deleted.txt");
        if deleted.exists() {
            for line in fs::read_to_string(&deleted)?.lines() {
                let p = self.repo_path.join(line);
                if p.is_dir() {
                    let _ = fs::remove_dir_all(&p);
                } else if p.exists() {
                    let _ = fs::remove_file(&p);
                }
            }
        }

        Ok(())
    }

    /// Per-file diffs of this snapshot against the current worktree
    /// (HEAD plus uncommitted files). Empty means they match.
    pub fn diff_files_against_current(&self, id: &str) -> Result<Vec<SnapshotDiffFile>> {
        let old = self.snapshot_files(id)?;
        let new = self.current_files()?;
        let mut paths: BTreeSet<&str> = old.keys().map(|s| s.as_str()).collect();
        paths.extend(new.keys().map(|s| s.as_str()));

        let mut files = Vec::new();
        for path in paths {
            let a = old.get(path).map(Vec::as_slice).unwrap_or(&[]);
            let b = new.get(path).map(Vec::as_slice).unwrap_or(&[]);
            if a == b {
                continue;
            }
            let kind = if a.contains(&0) || b.contains(&0) {
                SnapshotDiffKind::Binary
            } else if a.is_empty() {
                SnapshotDiffKind::Added
            } else if b.is_empty() {
                SnapshotDiffKind::Removed
            } else {
                SnapshotDiffKind::Changed
            };
            let mut patch = String::new();
            append_file_diff(&mut patch, path, a, b)?;
            files.push(SnapshotDiffFile {
                path: path.to_string(),
                kind,
                patch,
            });
        }
        Ok(files)
    }

    /// Unified diff of this snapshot against the current worktree.
    /// Empty string means they match.
    pub fn diff_against_current(&self, id: &str) -> Result<String> {
        let files = self.diff_files_against_current(id)?;
        Ok(files.into_iter().map(|f| f.patch).collect())
    }

    fn snapshot_files(&self, id: &str) -> Result<BTreeMap<String, Vec<u8>>> {
        let snapshot_dir = self.snapshots_dir.join(id);
        if !snapshot_dir.exists() {
            return Err(ToriiError::Snapshot(format!("Snapshot not found: {}", id)));
        }
        let git_backup = snapshot_dir.join("git_backup");
        let worktree = snapshot_dir.join("worktree");
        let mut files = if git_backup.is_dir() {
            let snap_repo = git2::Repository::open(&git_backup).map_err(ToriiError::Git)?;
            let blobs = match snap_repo.head().and_then(|h| h.peel_to_tree()) {
                Ok(tree) => tree_blobs(&snap_repo, &tree)?,
                Err(_) => BTreeMap::new(),
            };
            blobs
        } else if worktree.is_dir() {
            // A files snapshot holds only some files: everything else is
            // as it is now.
            self.current_files()?
        } else {
            return Err(ToriiError::Snapshot(format!(
                "Snapshot {} has no git_backup to diff against",
                id
            )));
        };
        if worktree.is_dir() {
            overlay_dir(&worktree, "", &mut files)?;
        }
        let deleted = snapshot_dir.join("deleted.txt");
        if deleted.exists() {
            for line in fs::read_to_string(&deleted)?.lines() {
                files.remove(line);
            }
        }
        Ok(files)
    }

    fn current_files(&self) -> Result<BTreeMap<String, Vec<u8>>> {
        let repo = git2::Repository::discover(&self.repo_path).map_err(ToriiError::Git)?;
        let mut files = BTreeMap::new();
        if let Ok(head) = repo.head() {
            if let Ok(tree) = head.peel_to_tree() {
                files = tree_blobs(&repo, &tree)?;
            }
        }
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        let statuses = repo.statuses(Some(&mut opts)).map_err(ToriiError::Git)?;
        for entry in statuses.iter() {
            let Some(path) = entry.path() else { continue };
            let st = entry.status();
            if st.is_wt_deleted() || st.is_index_deleted() {
                files.remove(path);
                continue;
            }
            let src = self.repo_path.join(path);
            if src.is_dir() {
                overlay_dir(&src, path, &mut files)?;
            } else if src.is_file() {
                files.insert(path.to_string(), fs::read(&src)?);
            }
        }
        Ok(files)
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
            println!(
                "   timestamp: {}",
                metadata.timestamp.format("%Y-%m-%d %H:%M:%S")
            );
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
            let kind = if entry.file_type()?.is_dir() {
                "dir"
            } else {
                "file"
            };
            println!("     {kind}: {}", entry.file_name().to_string_lossy());
        }
        Ok(())
    }

    /// Configure auto-snapshot settings.
    ///
    /// Writes the same store `torii config` reads: per-repo
    /// `.torii/config.toml`, keys `snapshot.auto_enabled` and
    /// `snapshot.auto_interval_minutes`. The old `.torii/config.json`
    /// (different names, unread by `torii config`) is removed so the
    /// two cannot disagree.
    pub fn configure_auto_snapshot(&self, enable: bool, interval: Option<u32>) -> Result<()> {
        let mut cfg = crate::config::ToriiConfig::load_local(&self.repo_path)?;
        cfg.snapshot.auto_enabled = enable;
        if let Some(mins) = interval {
            if mins == 0 {
                return Err(ToriiError::InvalidConfig(
                    "interval must be at least 1 minute".to_string(),
                ));
            }
            cfg.snapshot.auto_interval_minutes = mins;
        }
        cfg.save_local(&self.repo_path)?;

        let legacy = self.repo_path.join(".torii").join("config.json");
        if legacy.exists() {
            let _ = fs::remove_file(&legacy);
        }
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
        let mut repo = git2::Repository::discover(&self.repo_path).map_err(ToriiError::Git)?;

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
        let oid = repo
            .stash_save2(&signature, Some(stash_name), Some(flags))
            .map_err(ToriiError::Git)?;

        // Defensive: libgit2's `stash_save2` has been observed to
        // return OK without actually saving anything in some edge
        // cases (changes only in the index with DEFAULT flags). Verify
        // the working tree is clean post-stash; if it isn't, libgit2
        // lied to us and we don't want the user to think their
        // changes are safe when they aren't.
        let mut verify = git2::StatusOptions::new();
        verify
            .include_untracked(include_untracked)
            .recurse_untracked_dirs(include_untracked);
        let still_dirty = !repo
            .statuses(Some(&mut verify))
            .map_err(ToriiError::Git)?
            .is_empty();
        if still_dirty {
            return Err(ToriiError::Snapshot(
                "stash_save2 returned OK but the working tree is still dirty — \
                 libgit2 didn't actually stash anything. Workaround: \
                 `torii snapshot create -n WIP` (named persistent snapshot)."
                    .to_string(),
            ));
        }

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
        let mut repo = git2::Repository::discover(&self.repo_path).map_err(ToriiError::Git)?;

        let index: usize = match id {
            Some(s) => s
                .trim_start_matches("stash@{")
                .trim_end_matches('}')
                .parse()
                .map_err(|_| {
                    ToriiError::Snapshot(format!(
                        "invalid stash index `{}` (use a number: 0, 1, …)",
                        s
                    ))
                })?,
            None => 0,
        };

        // Confirm the entry exists for a friendlier error than libgit2's.
        let mut count = 0;
        repo.stash_foreach(|_, _, _| {
            count += 1;
            true
        })
        .map_err(ToriiError::Git)?;
        if count == 0 {
            return Err(ToriiError::Snapshot("No stash found".to_string()));
        }
        if index >= count {
            return Err(ToriiError::Snapshot(format!(
                "stash@{{{}}} doesn't exist (have {} stash{})",
                index,
                count,
                if count == 1 { "" } else { "es" }
            )));
        }

        println!("🔄 Restoring stash@{{{}}}", index);
        if keep {
            let mut opts = git2::StashApplyOptions::new();
            opts.reinstantiate_index();
            repo.stash_apply(index, Some(&mut opts))
                .map_err(ToriiError::Git)?;
            println!(
                "   Stash kept (use `torii snapshot unstash {} --no-keep` to drop)",
                index
            );
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

        let latest = snapshots
            .last()
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
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }

    #[test]
    fn configure_auto_snapshot_writes_toml_not_json() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path();
        init_repo(repo_path);

        let mgr = SnapshotManager::new(repo_path).unwrap();
        // leftover JSON with the old key names must not survive
        let json = repo_path.join(".torii/config.json");
        fs::create_dir_all(json.parent().unwrap()).unwrap();
        fs::write(
            &json,
            r#"{ "auto_snapshot_enabled": false, "auto_snapshot_interval_minutes": 30 }"#,
        )
        .unwrap();

        mgr.configure_auto_snapshot(true, Some(120)).unwrap();

        assert!(
            !json.exists(),
            "legacy .torii/config.json must be removed so it cannot disagree"
        );
        let cfg = crate::config::ToriiConfig::load_local(repo_path).unwrap();
        assert!(cfg.snapshot.auto_enabled);
        assert_eq!(cfg.snapshot.auto_interval_minutes, 120);
    }

    #[test]
    fn maybe_auto_is_a_noop_when_disabled() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path();
        init_repo(repo_path);
        let mgr = SnapshotManager::new(repo_path).unwrap();
        // Disabled here, explicitly: the global config of the machine
        // running the tests may enable it.
        mgr.configure_auto_snapshot(false, None).unwrap();
        assert!(mgr.maybe_auto("pre-fast-forward").unwrap().is_none());
        assert!(fs::read_dir(repo_path.join(".git/torii/snapshots"))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn maybe_auto_creates_a_snapshot_when_enabled() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path();
        init_repo(repo_path);
        let mgr = SnapshotManager::new(repo_path).unwrap();
        mgr.configure_auto_snapshot(true, Some(30)).unwrap();
        let id = mgr
            .maybe_auto("pre-fast-forward")
            .unwrap()
            .expect("snapshot");
        assert!(repo_path.join(".git/torii/snapshots").join(&id).exists());
    }

    fn commit_file(dir: &Path, name: &str, content: &str, msg: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        let repo = git2::Repository::open(dir).unwrap();
        let sig = git2::Signature::now("T", "t@x").unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(Path::new(name)).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&parent])
            .unwrap();
    }

    #[test]
    fn restore_roundtrips_a_committed_file() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        init_repo(dir);
        commit_file(dir, "tracked.txt", "committed\n", "add tracked");

        let mgr = SnapshotManager::new(dir).unwrap();
        let id = mgr.create_snapshot(Some("before-edit")).unwrap();
        std::fs::write(dir.join("tracked.txt"), "mutated\n").unwrap();

        mgr.restore_snapshot(&id).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("tracked.txt")).unwrap(),
            "committed\n"
        );
        assert!(
            dir.join(".git/HEAD").exists(),
            "restore must not delete the repository"
        );
        assert!(
            dir.join(".git/torii/snapshots").join(&id).exists(),
            "the snapshot itself must survive restore"
        );
    }

    #[test]
    fn restore_roundtrips_uncommitted_work() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        init_repo(dir);
        commit_file(dir, "tracked.txt", "committed\n", "add tracked");

        let mgr = SnapshotManager::new(dir).unwrap();
        std::fs::write(dir.join("tracked.txt"), "dirty-wip\n").unwrap();
        std::fs::write(dir.join("untracked.txt"), "keep-me\n").unwrap();
        let id = mgr.create_snapshot(Some("with-wip")).unwrap();

        std::fs::write(dir.join("tracked.txt"), "overwritten\n").unwrap();
        std::fs::remove_file(dir.join("untracked.txt")).unwrap();

        mgr.restore_snapshot(&id).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("tracked.txt")).unwrap(),
            "dirty-wip\n",
            "uncommitted edit must come back"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("untracked.txt")).unwrap(),
            "keep-me\n",
            "untracked file must come back"
        );
    }

    #[test]
    fn restore_keeps_other_snapshots() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        init_repo(dir);
        let mgr = SnapshotManager::new(dir).unwrap();
        let a = mgr.create_snapshot(Some("a")).unwrap();
        let b = mgr.create_snapshot(Some("b")).unwrap();
        mgr.restore_snapshot(&a).unwrap();
        assert!(dir.join(".git/torii/snapshots").join(&a).exists());
        assert!(dir.join(".git/torii/snapshots").join(&b).exists());
    }

    #[test]
    fn diff_against_current_is_empty_when_nothing_changed() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        init_repo(dir);
        commit_file(dir, "tracked.txt", "committed\n", "add tracked");
        let mgr = SnapshotManager::new(dir).unwrap();
        let id = mgr.create_snapshot(Some("same")).unwrap();
        let patch = mgr.diff_against_current(&id).unwrap();
        assert!(
            patch.is_empty(),
            "unchanged worktree must diff empty, got: {patch}"
        );
    }

    #[test]
    fn diff_against_current_shows_an_edit_and_an_untracked_file() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        init_repo(dir);
        commit_file(dir, "tracked.txt", "committed\n", "add tracked");
        let mgr = SnapshotManager::new(dir).unwrap();
        let id = mgr.create_snapshot(Some("before")).unwrap();
        std::fs::write(dir.join("tracked.txt"), "mutated\n").unwrap();
        std::fs::write(dir.join("new.txt"), "fresh\n").unwrap();
        let patch = mgr.diff_against_current(&id).unwrap();
        assert!(
            patch.contains("committed") && patch.contains("mutated"),
            "edit must appear in the patch: {patch}"
        );
        assert!(
            patch.contains("fresh") && patch.contains("new.txt"),
            "untracked file must appear in the patch: {patch}"
        );
        let files = mgr.diff_files_against_current(&id).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(
            names.contains(&"tracked.txt") && names.contains(&"new.txt"),
            "{names:?}"
        );
    }

    #[test]
    fn a_files_snapshot_saves_and_diffs_only_its_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        init_repo(dir);
        commit_file(dir, "a.txt", "a committed\n", "add a");
        commit_file(dir, "b.txt", "b committed\n", "add b");
        std::fs::write(dir.join("a.txt"), "a edited\n").unwrap();
        let mgr = SnapshotManager::new(dir).unwrap();
        let id = mgr
            .create_files_snapshot(Some("pre-restore"), &["a.txt"])
            .unwrap();
        assert!(!mgr.snapshots_dir.join(&id).join("git_backup").exists());

        std::fs::write(dir.join("a.txt"), "a committed\n").unwrap();
        let files = mgr.diff_files_against_current(&id).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(names, ["a.txt"], "only the saved file differs");

        mgr.restore_snapshot(&id).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "a edited\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("b.txt")).unwrap(),
            "b committed\n"
        );
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
        assert!(
            new_loc.exists(),
            "snapshot should be at .git/torii/snapshots/{}",
            id
        );
        assert!(
            !old_loc.exists(),
            "snapshot must NOT be in working tree at .torii/snapshots/{}",
            id
        );
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
        assert!(
            new_loc.join("metadata.json").exists(),
            "files inside should come along"
        );
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
