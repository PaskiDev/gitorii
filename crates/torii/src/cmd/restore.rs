//! `torii restore` — put a tracked file back the way it was.
//!
//! The verb the toolbox was missing. `remove` deletes a file, `clean` deletes
//! untracked ones, `snapshot` saves the whole work-in-progress; none of them
//! undoes an edit to a single tracked file, which left `git checkout --` as
//! the only way out.
//!
//! Two jobs, one for each place a change can sit:
//!
//! - the working tree: `torii restore <path>` throws the edit away and writes
//!   back what the index holds (which is what HEAD holds, unless the path was
//!   staged).
//! - the index: `torii restore --staged <path>` takes the path out of the
//!   next commit and leaves the file on disk exactly as it is.
//!
//! Throwing away an edit destroys work that was never committed, so — same
//! rule as `worktree remove` — a snapshot is taken first and its id printed.
//! Nothing is at risk when the path is already clean, and no snapshot is
//! taken then.

use crate::error::{Result, ToriiError};
use git2::Repository;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Opts {
    /// Take the path out of the index instead of off the disk.
    pub staged: bool,
    /// Skip the safety snapshot. Off by default — opt-in skip.
    pub no_snapshot: bool,
}

pub fn restore(repo_path: &Path, paths: &[PathBuf], opts: &Opts) -> Result<()> {
    if paths.is_empty() {
        return Err(ToriiError::Usage(
            "`restore` needs at least one path".into(),
        ));
    }
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    if repo.workdir().is_none() {
        return Err(ToriiError::RepoState("bare repo".into()));
    }

    let mut specs = Vec::new();
    for path in paths {
        let spec = relative_spec(&repo, path)?;
        // A directory is the files under it: git's pathspec would match the
        // name itself and nothing else, which looked like "already clean".
        let expanded = expand_directory(&repo, &spec)?;
        match expanded {
            Some(files) => specs.extend(files),
            None => {
                ensure_tracked(&repo, &spec)?;
                specs.push(spec);
            }
        }
    }

    if opts.staged {
        unstage(&repo, &specs)
    } else {
        discard(&repo, repo_path, &specs, opts.no_snapshot)
    }
}

/// Take the paths out of the index: their entries go back to HEAD, the files
/// on disk are not touched. Nothing is destroyed, so no snapshot.
fn unstage(repo: &Repository, specs: &[String]) -> Result<()> {
    let head = repo
        .head()
        .and_then(|h| h.peel(git2::ObjectType::Commit))
        .map_err(ToriiError::Git)?;
    repo.reset_default(Some(&head), specs.iter())
        .map_err(ToriiError::Git)?;
    for spec in specs {
        println!("✅ Unstaged: {spec}");
    }
    Ok(())
}

/// Write the index's content back over the working tree, losing whatever was
/// typed since. Snapshots first unless told not to.
fn discard(repo: &Repository, repo_path: &Path, specs: &[String], no_snapshot: bool) -> Result<()> {
    let dirty: Vec<&String> = specs
        .iter()
        .filter(|spec| {
            repo.status_file(Path::new(spec.as_str()))
                .map(|s| s.intersects(git2::Status::WT_MODIFIED | git2::Status::WT_TYPECHANGE))
                .unwrap_or(false)
        })
        .collect();

    if dirty.is_empty() {
        for spec in specs {
            println!("✅ Already as committed: {spec}");
        }
        return Ok(());
    }

    // Snapshot BEFORE mutating, and only when there is something to lose.
    if !no_snapshot {
        match crate::snapshot::SnapshotManager::new(repo_path) {
            Ok(mgr) => match mgr.create_snapshot(Some("pre-restore")) {
                Ok(id) => println!(
                    "📸 Snapshot: {} (revert with: torii snapshot restore {})",
                    id, id
                ),
                Err(e) => eprintln!("⚠  Snapshot failed (proceeding anyway): {e}"),
            },
            Err(e) => eprintln!("⚠  Snapshot setup failed (proceeding anyway): {e}"),
        }
    }

    let mut builder = git2::build::CheckoutBuilder::new();
    builder.force().remove_untracked(false);
    for spec in specs {
        builder.path(spec.as_str());
    }
    // `update_index(false)`: the index is the source here, not the target —
    // a staged version of the path must survive a working-tree restore.
    builder.update_index(false);
    repo.checkout_index(None, Some(&mut builder))
        .map_err(ToriiError::Git)?;

    for spec in dirty {
        println!("✅ Restored: {spec}");
    }
    Ok(())
}

/// Every tracked file under `spec`, or `None` when `spec` is not a directory
/// git knows about — a tracked file, or nothing at all, which `ensure_tracked`
/// is left to report.
fn expand_directory(repo: &Repository, spec: &str) -> Result<Option<Vec<String>>> {
    let index = repo.index().map_err(ToriiError::Git)?;
    if index.get_path(Path::new(spec), 0).is_some() {
        return Ok(None); // a tracked file, not a directory
    }
    let prefix = format!("{}/", spec.trim_end_matches('/'));
    let under: Vec<String> = index
        .iter()
        .filter_map(|entry| String::from_utf8(entry.path).ok())
        .filter(|p| p.starts_with(&prefix))
        .collect();
    if under.is_empty() {
        return Ok(None); // not a directory either — let ensure_tracked speak
    }
    Ok(Some(under))
}

/// Paths arrive as the user typed them — absolute, or relative to wherever
/// they are standing. git wants them relative to the work tree root.
fn relative_spec(repo: &Repository, path: &Path) -> Result<String> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| ToriiError::RepoState("bare repo".into()))?;
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| ToriiError::Fs(e.to_string()))?
            .join(path)
    };
    // The file may be gone (that is one reason to restore it), so canonicalise
    // the root only and strip textually.
    let root = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());
    let abs = abs.canonicalize().unwrap_or(abs);
    let rel = abs
        .strip_prefix(&root)
        .map_err(|_| ToriiError::Usage(format!("{} is outside this repository", path.display())))?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// Restoring only means something for a path git already knows: an untracked
/// file has no earlier version to go back to, and saying so is kinder than
/// silently doing nothing.
fn ensure_tracked(repo: &Repository, spec: &str) -> Result<()> {
    let index = repo.index().map_err(ToriiError::Git)?;
    if index.get_path(Path::new(spec), 0).is_some() {
        return Ok(());
    }
    let in_head = repo
        .head()
        .and_then(|h| h.peel_to_tree())
        .map(|tree| tree.get_path(Path::new(spec)).is_ok())
        .unwrap_or(false);
    if in_head {
        return Ok(());
    }
    Err(ToriiError::Usage(format!(
        "{spec} is not tracked — there is no committed version to restore. \
         Use `torii clean -f` to drop untracked files."
    )))
}
