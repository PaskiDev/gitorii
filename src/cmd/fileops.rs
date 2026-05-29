//! `torii rm` and `torii mv` — tracked-file operations.
//!
//! Both touch two places at once: the libgit2 index (so the change is
//! staged for the next `torii save`) and the working tree on disk.
//!
//! - `rm`: removes the file(s) from disk and the index. `--cached` keeps
//!   the file on disk (untracks only). `-r` allows directories.
//! - `mv`: rename or move tracked files. Atomic from git's perspective —
//!   the old path becomes a delete + the new path an add, but `torii
//!   status` and `torii log --follow` recognise it as a rename.
//!
//! Both refuse to overwrite uncommitted modifications by default; pass
//! `--force` to override.

use crate::error::{Result, ToriiError};
use git2::Repository;
use std::path::{Path, PathBuf};

// -- rm ---------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct RmOpts {
    /// Don't actually delete from disk, just untrack.
    pub cached: bool,
    /// Allow removing directories recursively.
    pub recursive: bool,
    /// Proceed even if the file has uncommitted modifications.
    pub force: bool,
}

pub fn rm(repo_path: &Path, paths: &[PathBuf], opts: &RmOpts) -> Result<()> {
    if paths.is_empty() {
        return Err(ToriiError::InvalidConfig("`rm` needs at least one path".into()));
    }
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| ToriiError::InvalidConfig("bare repo".into()))?
        .to_path_buf();

    let mut index = repo.index().map_err(ToriiError::Git)?;

    for path in paths {
        let abs = if path.is_absolute() { path.clone() } else { workdir.join(path) };
        let meta = std::fs::symlink_metadata(&abs).ok();

        // Dirty-modification guard. Cheap heuristic: ask the index.
        if !opts.force {
            let status = repo.status_file(path).unwrap_or(git2::Status::empty());
            if status.contains(git2::Status::WT_MODIFIED)
                || status.contains(git2::Status::WT_NEW)
                || status.contains(git2::Status::INDEX_MODIFIED)
            {
                return Err(ToriiError::InvalidConfig(format!(
                    "{} has staged or local modifications. \
                     Commit/stash or pass --force to drop them.",
                    path.display()
                )));
            }
        }

        // Index removal.
        if let Some(m) = &meta {
            if m.is_dir() {
                if !opts.recursive {
                    return Err(ToriiError::InvalidConfig(format!(
                        "{} is a directory — pass -r to recurse.",
                        path.display()
                    )));
                }
                index
                    .remove_dir(path, 0)
                    .map_err(|e| ToriiError::InvalidConfig(format!("index remove_dir: {e}")))?;
            } else {
                index
                    .remove_path(path)
                    .map_err(|e| ToriiError::InvalidConfig(format!("index remove_path: {e}")))?;
            }
        } else if index.get_path(path, 0).is_some() {
            // Already missing on disk, but still tracked — clean up the
            // index entry and surface any error rather than swallow it.
            index
                .remove_path(path)
                .map_err(|e| ToriiError::InvalidConfig(format!("index remove_path: {e}")))?;
        }

        // Filesystem removal (unless --cached).
        if !opts.cached {
            if let Some(m) = &meta {
                let res = if m.is_dir() {
                    std::fs::remove_dir_all(&abs)
                } else {
                    std::fs::remove_file(&abs)
                };
                res.map_err(|e| {
                    ToriiError::InvalidConfig(format!("rm {}: {}", abs.display(), e))
                })?;
            }
        }

        println!("🗑  {}", path.display());
    }

    index.write().map_err(ToriiError::Git)?;
    println!("\n✅ Removed {} path(s) — stage already updated.", paths.len());
    Ok(())
}

// -- mv ---------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct MvOpts {
    /// Allow overwriting `to` if it already exists.
    pub force: bool,
}

pub fn mv(repo_path: &Path, from: &Path, to: &Path, opts: &MvOpts) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| ToriiError::InvalidConfig("bare repo".into()))?
        .to_path_buf();
    let abs_from = if from.is_absolute() { from.to_path_buf() } else { workdir.join(from) };
    let abs_to = if to.is_absolute() { to.to_path_buf() } else { workdir.join(to) };

    if !abs_from.exists() {
        return Err(ToriiError::InvalidConfig(format!(
            "source {} does not exist",
            from.display()
        )));
    }
    if abs_to.exists() && !opts.force {
        return Err(ToriiError::InvalidConfig(format!(
            "target {} already exists. Pass --force to overwrite.",
            to.display()
        )));
    }
    if let Some(parent) = abs_to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ToriiError::InvalidConfig(format!("mkdir {}: {}", parent.display(), e))
        })?;
    }

    // 1. Filesystem rename.
    std::fs::rename(&abs_from, &abs_to)
        .map_err(|e| ToriiError::InvalidConfig(format!("rename: {e}")))?;

    // 2. Index update — remove old, add new. Rename detection in `log`
    // and `diff` happens at display time via libgit2's similarity heuristic,
    // we just need to record the old delete + new add as the staged state.
    let mut index = repo.index().map_err(ToriiError::Git)?;
    if index.get_path(from, 0).is_some() {
        index
            .remove_path(from)
            .map_err(|e| ToriiError::InvalidConfig(format!("index remove_path: {e}")))?;
    }
    index
        .add_path(to)
        .map_err(|e| ToriiError::InvalidConfig(format!("index add_path: {e}")))?;
    index.write().map_err(ToriiError::Git)?;

    println!("🔀 {} → {}", from.display(), to.display());
    Ok(())
}
