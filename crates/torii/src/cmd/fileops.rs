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
        return Err(ToriiError::Usage("`rm` needs at least one path".into()));
    }
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| ToriiError::RepoState("bare repo".into()))?
        .to_path_buf();

    let mut index = repo.index().map_err(ToriiError::Git)?;

    for path in paths {
        let abs = if path.is_absolute() {
            path.clone()
        } else {
            workdir.join(path)
        };
        let meta = std::fs::symlink_metadata(&abs).ok();

        // Dirty-modification guard. Cheap heuristic: ask the index.
        if !opts.force {
            let status = repo.status_file(path).unwrap_or(git2::Status::empty());
            if status.contains(git2::Status::WT_MODIFIED)
                || status.contains(git2::Status::WT_NEW)
                || status.contains(git2::Status::INDEX_MODIFIED)
            {
                return Err(ToriiError::RepoState(format!(
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
                    return Err(ToriiError::Usage(format!(
                        "{} is a directory — pass -r to recurse.",
                        path.display()
                    )));
                }
                index
                    .remove_dir(path, 0)
                    .map_err(|e| ToriiError::RepoState(format!("index remove_dir: {e}")))?;
            } else {
                index
                    .remove_path(path)
                    .map_err(|e| ToriiError::RepoState(format!("index remove_path: {e}")))?;
            }
        } else if index.get_path(path, 0).is_some() {
            // Already missing on disk, but still tracked — clean up the
            // index entry and surface any error rather than swallow it.
            index
                .remove_path(path)
                .map_err(|e| ToriiError::RepoState(format!("index remove_path: {e}")))?;
        }

        // Filesystem removal (unless --cached).
        if !opts.cached {
            if let Some(m) = &meta {
                let res = if m.is_dir() {
                    std::fs::remove_dir_all(&abs)
                } else {
                    std::fs::remove_file(&abs)
                };
                res.map_err(|e| ToriiError::Fs(format!("rm {}: {}", abs.display(), e)))?;
            }
        }

        println!("🗑  {}", path.display());
    }

    index.write().map_err(ToriiError::Git)?;
    println!(
        "\n✅ Removed {} path(s) — stage already updated.",
        paths.len()
    );
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
        .ok_or_else(|| ToriiError::RepoState("bare repo".into()))?
        .to_path_buf();
    let abs_from = if from.is_absolute() {
        from.to_path_buf()
    } else {
        workdir.join(from)
    };
    let abs_to = if to.is_absolute() {
        to.to_path_buf()
    } else {
        workdir.join(to)
    };

    if !abs_from.exists() {
        return Err(ToriiError::Usage(format!(
            "source {} does not exist",
            from.display()
        )));
    }
    if abs_to.exists() && !opts.force {
        return Err(ToriiError::Usage(format!(
            "target {} already exists. Pass --force to overwrite.",
            to.display()
        )));
    }
    if let Some(parent) = abs_to.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ToriiError::Fs(format!("mkdir {}: {}", parent.display(), e)))?;
    }

    let from_is_dir = abs_from.is_dir();

    // 1. Filesystem rename.
    std::fs::rename(&abs_from, &abs_to).map_err(|e| ToriiError::Fs(format!("rename: {e}")))?;

    // 2. Index update — remove old, add new. Rename detection in `log`
    // and `diff` happens at display time via libgit2's similarity heuristic,
    // we just need to record the old delete + new add as the staged state.
    let mut index = repo.index().map_err(ToriiError::Git)?;
    if from_is_dir {
        // `Index::add_path`/`remove_path` only accept files — for a
        // directory, re-record every tracked entry under the old prefix
        // at its new path. Untracked files inside the dir moved on disk
        // already and stay untracked, same as `git mv`.
        let norm = |p: &Path| -> String {
            p.to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_string()
        };
        let from_prefix = format!("{}/", norm(from));
        let to_base = norm(to);
        let moved: Vec<(String, String)> = index
            .iter()
            .filter_map(|e| {
                let p = String::from_utf8_lossy(&e.path).to_string();
                p.strip_prefix(&from_prefix)
                    .map(|rest| (p.clone(), format!("{}/{}", to_base, rest)))
            })
            .collect();
        for (old, new) in &moved {
            index
                .remove_path(Path::new(old))
                .map_err(|e| ToriiError::RepoState(format!("index remove_path {old}: {e}")))?;
            index
                .add_path(Path::new(new))
                .map_err(|e| ToriiError::RepoState(format!("index add_path {new}: {e}")))?;
        }
    } else {
        if index.get_path(from, 0).is_some() {
            index
                .remove_path(from)
                .map_err(|e| ToriiError::RepoState(format!("index remove_path: {e}")))?;
        }
        index
            .add_path(to)
            .map_err(|e| ToriiError::RepoState(format!("index add_path: {e}")))?;
    }
    index.write().map_err(ToriiError::Git)?;

    println!("🔀 {} → {}", from.display(), to.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Repository;

    /// Temp repo with `dir/a.txt`, `dir/sub/b.txt` tracked and
    /// `dir/untracked.txt` on disk only.
    fn repo_with_tracked_dir() -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        std::fs::create_dir_all(tmp.path().join("dir/sub")).unwrap();
        std::fs::write(tmp.path().join("dir/a.txt"), "a").unwrap();
        std::fs::write(tmp.path().join("dir/sub/b.txt"), "b").unwrap();
        std::fs::write(tmp.path().join("dir/untracked.txt"), "u").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("dir/a.txt")).unwrap();
        index.add_path(Path::new("dir/sub/b.txt")).unwrap();
        index.write().unwrap();
        (tmp, repo)
    }

    fn index_paths(repo: &Repository) -> Vec<String> {
        // git2 caches the Index per Repository instance — force a re-read
        // so we observe what `mv` (which opened its own Repository) wrote.
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        index
            .iter()
            .map(|e| String::from_utf8_lossy(&e.path).to_string())
            .collect()
    }

    #[test]
    fn mv_directory_moves_disk_and_reindexes_tracked_entries() {
        let (tmp, repo) = repo_with_tracked_dir();
        let opts = MvOpts { force: false };

        mv(tmp.path(), Path::new("dir"), Path::new("renamed"), &opts)
            .expect("mv of a tracked directory must succeed");

        // Disk: everything moved, source gone.
        assert!(!tmp.path().join("dir").exists());
        assert!(tmp.path().join("renamed/a.txt").exists());
        assert!(tmp.path().join("renamed/sub/b.txt").exists());
        assert!(tmp.path().join("renamed/untracked.txt").exists());

        // Index: tracked entries re-recorded under the new prefix;
        // the untracked file stays untracked.
        let paths = index_paths(&repo);
        assert!(
            paths.contains(&"renamed/a.txt".to_string()),
            "index: {paths:?}"
        );
        assert!(
            paths.contains(&"renamed/sub/b.txt".to_string()),
            "index: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("dir/")),
            "index: {paths:?}"
        );
        assert!(
            !paths.contains(&"renamed/untracked.txt".to_string()),
            "index: {paths:?}"
        );
    }

    #[test]
    fn mv_single_file_still_works() {
        let (tmp, repo) = repo_with_tracked_dir();
        let opts = MvOpts { force: false };

        mv(
            tmp.path(),
            Path::new("dir/a.txt"),
            Path::new("dir/c.txt"),
            &opts,
        )
        .unwrap();

        assert!(tmp.path().join("dir/c.txt").exists());
        let paths = index_paths(&repo);
        assert!(paths.contains(&"dir/c.txt".to_string()));
        assert!(!paths.contains(&"dir/a.txt".to_string()));
    }

    #[test]
    fn mv_directory_with_trailing_slash_normalizes() {
        let (tmp, repo) = repo_with_tracked_dir();
        let opts = MvOpts { force: false };

        mv(tmp.path(), Path::new("dir/"), Path::new("renamed/"), &opts)
            .expect("trailing slashes must not break prefix matching");

        let paths = index_paths(&repo);
        assert!(
            paths.contains(&"renamed/a.txt".to_string()),
            "index: {paths:?}"
        );
    }
}
