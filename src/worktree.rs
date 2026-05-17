//! Worktrees — multiple working copies of the same repository, each on its
//! own branch, sharing the underlying object database.
//!
//! Five entry points, the MVP cut requested for 0.6.8:
//!
//! - [`add`]    — create a new worktree at a path, optionally creating its
//!                branch in the same call.
//! - [`list`]   — show every worktree with branch + clean/dirty status.
//! - [`remove`] — delete a worktree and its directory. Always takes a
//!                safety snapshot first (per project decision).
//! - [`prune`]  — clean up metadata of worktrees whose directory vanished.
//! - [`open`]   — spawn `$SHELL` in the worktree directory. Pure UX
//!                convenience that `git worktree` doesn't offer.
//!
//! Design notes:
//!
//! - **Default path is configurable.** If the user omits `<path>`, we read
//!   `worktree.base_dir` from config (default `..`) and append
//!   `<repo>-<branch-sanitized>/`. Sanitizing replaces `/` with `-`, so
//!   `feature/auth` becomes `feature-auth` in the directory name.
//! - **`worktree.name` is derived, not user-facing.** libgit2 needs a name
//!   distinct from any other worktree's; we use the directory's last
//!   component (also sanitized). The CLI never asks the user for it.
//! - **Snapshot before remove** is unconditional (decided in the design
//!   review). A dirty worktree without `--force` still aborts so the user
//!   sees the warning, but the snapshot is taken either way before any
//!   filesystem changes.
//! - **`open`** uses `$SHELL` and falls back to `/bin/bash`. It blocks on
//!   the child shell so the parent terminal returns control when the user
//!   exits the spawned shell — exactly what `(cd path && $SHELL)` does in
//!   a script.

use crate::error::{Result, ToriiError};
use git2::{BranchType, Repository, Worktree, WorktreeAddOptions, WorktreePruneOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

// -- Public types ----------------------------------------------------------

/// How `add` should resolve the worktree's HEAD.
#[derive(Debug, Clone)]
pub enum BranchSpec {
    /// Create a new branch off the main repo's HEAD and check it out.
    New(String),
    /// Check out an existing local branch.
    Existing(String),
}

#[derive(Debug, Default)]
pub struct AddOpts {
    /// Override `worktree.base_dir` for this invocation. When `None`, uses
    /// the explicit `path` argument or the config-derived default.
    pub explicit_path: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct RemoveOpts {
    /// Remove even if the worktree has uncommitted changes.
    pub force: bool,
    /// Skip the safety snapshot. Off by default — opt-in skip.
    pub no_snapshot: bool,
}

// -- add -------------------------------------------------------------------

/// Create a worktree at `path` (or the config-derived default) with the
/// branch described by `spec`.
pub fn add(repo_path: &Path, spec: BranchSpec, opts: &AddOpts) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;

    // 1. Resolve where the worktree directory will live.
    let target_path = resolve_target_path(&repo, repo_path, &spec, opts)?;
    if target_path.exists() {
        return Err(ToriiError::InvalidConfig(format!(
            "target path already exists: {}. Pick a different path or remove it first.",
            target_path.display()
        )));
    }

    // 2. Ensure the branch exists (creating it if spec asks for it).
    let branch_name = match &spec {
        BranchSpec::New(name) => {
            let head = repo
                .head()
                .map_err(|e| ToriiError::InvalidConfig(format!("repo has no HEAD: {e}")))?;
            let target_commit = head
                .peel_to_commit()
                .map_err(|e| ToriiError::InvalidConfig(format!("HEAD is not a commit: {e}")))?;
            if repo.find_branch(name, BranchType::Local).is_ok() {
                return Err(ToriiError::InvalidConfig(format!(
                    "branch '{name}' already exists. To check it out in a worktree, drop -b: \
                     torii worktree add <path> {name}"
                )));
            }
            repo.branch(name, &target_commit, false)
                .map_err(ToriiError::Git)?;
            name.clone()
        }
        BranchSpec::Existing(name) => {
            repo.find_branch(name, BranchType::Local).map_err(|e| {
                ToriiError::InvalidConfig(format!(
                    "branch '{name}' not found locally: {e}. \
                     Create it first with: torii branch {name} -c"
                ))
            })?;
            name.clone()
        }
    };

    // 3. Build options pointing at the branch reference.
    let branch_ref_name = format!("refs/heads/{}", branch_name);
    let branch_ref = repo
        .find_reference(&branch_ref_name)
        .map_err(ToriiError::Git)?;

    let mut wt_opts = WorktreeAddOptions::new();
    wt_opts.reference(Some(&branch_ref));

    // 4. libgit2 wants a unique "name". Use the directory leaf sanitized.
    let wt_name = derive_worktree_name(&target_path, &branch_name);

    repo.worktree(&wt_name, &target_path, Some(&wt_opts))
        .map_err(ToriiError::Git)?;

    println!(
        "🌳 Worktree created\n   path:   {}\n   branch: {}",
        target_path.display(),
        branch_name
    );
    println!("\n💡 Enter it with:  torii worktree open {}", target_path.display());

    Ok(())
}

fn resolve_target_path(
    _repo: &Repository,
    repo_path: &Path,
    spec: &BranchSpec,
    opts: &AddOpts,
) -> Result<PathBuf> {
    if let Some(explicit) = &opts.explicit_path {
        return Ok(expand_tilde(explicit));
    }

    // Derive `<base>/<repo-name>-<branch-sanitized>/`.
    let cfg = crate::config::ToriiConfig::load_global().unwrap_or_default();
    let base = expand_tilde(Path::new(&cfg.worktree.base_dir));

    // Make `..` relative to the repo root (not cwd) so behaviour is
    // predictable regardless of where the user invokes `torii` from.
    let base = if base.is_relative() {
        repo_path
            .canonicalize()
            .map_err(|e| ToriiError::InvalidConfig(format!("canonicalize repo: {e}")))?
            .join(base)
    } else {
        base
    };

    let repo_name = repo_path
        .canonicalize()
        .ok()
        .and_then(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "repo".to_string());

    let branch = match spec {
        BranchSpec::New(n) | BranchSpec::Existing(n) => n,
    };
    let leaf = format!("{}-{}", repo_name, sanitize_branch(branch));
    Ok(base.join(leaf))
}

fn expand_tilde(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(stripped) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    p.to_path_buf()
}

/// Replace `/` (the only legal-but-problematic char in branch names) with
/// `-` for use in filesystem paths and libgit2 worktree names.
pub fn sanitize_branch(branch: &str) -> String {
    branch.replace('/', "-")
}

fn derive_worktree_name(path: &Path, branch: &str) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| sanitize_branch(branch))
}

// -- list ------------------------------------------------------------------

/// Print every worktree (the main one plus linked ones) with branch and
/// dirty/clean status. The "this one" marker is whichever worktree we're
/// currently inside.
pub fn list(repo_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let here = repo_path.canonicalize().unwrap_or_else(|_| repo_path.to_path_buf());

    println!("🌳 Worktrees:\n");

    // Main worktree (where the .git directory lives, not a .git file).
    let main_path = repo
        .workdir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| repo_path.to_path_buf());
    let main_path = main_path.canonicalize().unwrap_or(main_path);
    print_worktree_row(&main_path, "(main)", &here, None)?;

    // Linked worktrees, by name.
    let names = repo.worktrees().map_err(ToriiError::Git)?;
    for i in 0..names.len() {
        let name = match names.get(i) {
            Some(n) => n,
            None => continue,
        };
        let wt = match repo.find_worktree(name) {
            Ok(w) => w,
            Err(_) => continue,
        };
        let wt_path = wt.path().canonicalize().unwrap_or_else(|_| wt.path().to_path_buf());
        print_worktree_row(&wt_path, name, &here, Some(&wt))?;
    }

    Ok(())
}

fn print_worktree_row(
    path: &Path,
    name: &str,
    here: &Path,
    wt: Option<&Worktree>,
) -> Result<()> {
    let is_here = path == here;
    let marker = if is_here { "📍" } else { " " };

    // Branch + state — open the worktree as its own repo and peek inside.
    let (branch, state) = describe_worktree(path).unwrap_or_else(|e| {
        ("?".to_string(), format!("error: {}", e))
    });

    let locked = wt
        .and_then(|w| w.is_locked().ok())
        .and_then(|s| match s {
            git2::WorktreeLockStatus::Locked(reason) => {
                Some(reason.unwrap_or_else(|| "(no reason)".to_string()))
            }
            git2::WorktreeLockStatus::Unlocked => None,
        });

    let suffix = match (is_here, locked) {
        (true, Some(r)) => format!("(this one, locked: {r:?})"),
        (true, None) => "(this one)".to_string(),
        (false, Some(r)) => format!("(locked: {r:?})"),
        (false, None) => String::new(),
    };

    println!(
        "  {marker} {path}\n      name: {name}   branch: {branch}   {state} {suffix}",
        path = path.display(),
    );
    Ok(())
}

/// Open the worktree as its own repo and report `(branch_name, status)`.
fn describe_worktree(path: &Path) -> Result<(String, String)> {
    let repo = Repository::open(path).map_err(ToriiError::Git)?;
    let head = repo.head().ok();

    let branch = head
        .as_ref()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "(detached)".to_string());

    let mut so = git2::StatusOptions::new();
    so.include_untracked(true).include_ignored(false);
    let statuses = repo.statuses(Some(&mut so)).map_err(ToriiError::Git)?;
    let dirty_count = statuses
        .iter()
        .filter(|s| !s.status().contains(git2::Status::IGNORED))
        .count();

    let state = if dirty_count == 0 {
        "clean".to_string()
    } else {
        format!("{} change(s)", dirty_count)
    };

    Ok((branch, state))
}

// -- remove ----------------------------------------------------------------

/// Remove a worktree and its directory. Always takes a snapshot first.
pub fn remove(repo_path: &Path, target_path: &Path, opts: &RemoveOpts) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let target = target_path
        .canonicalize()
        .map_err(|e| ToriiError::InvalidConfig(format!("path not found: {}: {}", target_path.display(), e)))?;

    // Find the matching worktree by comparing canonical paths.
    let names = repo.worktrees().map_err(ToriiError::Git)?;
    let mut wt_match: Option<(String, Worktree)> = None;
    for i in 0..names.len() {
        let name = match names.get(i) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let wt = match repo.find_worktree(&name) {
            Ok(w) => w,
            Err(_) => continue,
        };
        let wt_canon = wt
            .path()
            .canonicalize()
            .unwrap_or_else(|_| wt.path().to_path_buf());
        if wt_canon == target {
            wt_match = Some((name, wt));
            break;
        }
    }
    let (wt_name, wt) = wt_match.ok_or_else(|| {
        ToriiError::InvalidConfig(format!(
            "{} is not a known worktree of this repo. \
             Use 'torii worktree list' to see the canonical paths.",
            target.display()
        ))
    })?;

    // Dirty check (informational + gate behind --force).
    let dirty = match Repository::open(&target) {
        Ok(wt_repo) => {
            let mut so = git2::StatusOptions::new();
            so.include_untracked(true);
            wt_repo
                .statuses(Some(&mut so))
                .map(|s| s.iter().any(|x| !x.status().contains(git2::Status::IGNORED)))
                .unwrap_or(false)
        }
        Err(_) => false,
    };

    if dirty && !opts.force {
        return Err(ToriiError::InvalidConfig(format!(
            "worktree {} has uncommitted changes. \
             Commit/stash there or pass --force to drop them.",
            target.display()
        )));
    }

    // Snapshot ALWAYS before mutating (project decision). The snapshot is
    // taken of the worktree being removed, not of the main repo — the
    // dirty state we'd want to recover is in the worktree.
    if !opts.no_snapshot {
        match crate::snapshot::SnapshotManager::new(&target) {
            Ok(mgr) => match mgr.create_snapshot(Some(&format!("pre-worktree-remove-{wt_name}"))) {
                Ok(id) => println!(
                    "📸 Snapshot: {} (revert with: torii snapshot restore {})",
                    id, id
                ),
                Err(e) => eprintln!("⚠  Snapshot failed (proceeding anyway): {e}"),
            },
            Err(e) => eprintln!("⚠  Snapshot setup failed (proceeding anyway): {e}"),
        }
    }

    // Remove the working tree directory + prune the metadata.
    std::fs::remove_dir_all(&target).map_err(|e| {
        ToriiError::InvalidConfig(format!("rm -rf {}: {}", target.display(), e))
    })?;

    let mut prune_opts = WorktreePruneOptions::new();
    prune_opts.valid(true).working_tree(true);
    wt.prune(Some(&mut prune_opts)).map_err(ToriiError::Git)?;

    println!("🗑  Worktree '{}' removed from {}", wt_name, target.display());
    Ok(())
}

// -- prune -----------------------------------------------------------------

/// Remove metadata for worktrees whose directory has been deleted or
/// otherwise become invalid.
pub fn prune(repo_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let names = repo.worktrees().map_err(ToriiError::Git)?;

    let mut pruned = 0;
    for i in 0..names.len() {
        let name = match names.get(i) {
            Some(n) => n,
            None => continue,
        };
        let wt = match repo.find_worktree(name) {
            Ok(w) => w,
            Err(_) => continue,
        };
        // The default prune only fires on already-invalid worktrees,
        // which is exactly what we want — leave valid + working trees
        // alone.
        if wt.is_prunable(None).unwrap_or(false) {
            wt.prune(None).map_err(ToriiError::Git)?;
            println!("🧹 Pruned: {}", name);
            pruned += 1;
        }
    }

    if pruned == 0 {
        println!("✨ Nothing to prune.");
    } else {
        println!("\n✅ Pruned {} stale worktree entries.", pruned);
    }
    Ok(())
}

// -- open ------------------------------------------------------------------

/// Spawn `$SHELL` (or `/bin/bash`) in `target` and block until the user
/// exits it. Returns the child shell's exit status as an error if non-zero.
pub fn open(repo_path: &Path, target: &Path) -> Result<()> {
    // Verify the target is in fact a worktree of this repo.
    let target_canon = target
        .canonicalize()
        .map_err(|e| ToriiError::InvalidConfig(format!("{}: {}", target.display(), e)))?;
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;

    let main = repo
        .workdir()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    let mut is_known = main.as_ref().map(|m| *m == target_canon).unwrap_or(false);
    if !is_known {
        let names = repo.worktrees().map_err(ToriiError::Git)?;
        for i in 0..names.len() {
            if let Some(name) = names.get(i) {
                if let Ok(wt) = repo.find_worktree(name) {
                    let p = wt
                        .path()
                        .canonicalize()
                        .unwrap_or_else(|_| wt.path().to_path_buf());
                    if p == target_canon {
                        is_known = true;
                        break;
                    }
                }
            }
        }
    }
    if !is_known {
        return Err(ToriiError::InvalidConfig(format!(
            "{} is not a worktree of this repo. \
             Use 'torii worktree list' to see what's available.",
            target_canon.display()
        )));
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    println!(
        "🚪 Entering {} ({}). Type 'exit' to return.",
        target_canon.display(),
        shell
    );
    let status = Command::new(&shell)
        .current_dir(&target_canon)
        .status()
        .map_err(|e| ToriiError::InvalidConfig(format!("spawn {shell}: {e}")))?;

    if !status.success() {
        return Err(ToriiError::InvalidConfig(format!(
            "shell exited with status {status}"
        )));
    }
    Ok(())
}

// -- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_branch_replaces_slashes() {
        assert_eq!(sanitize_branch("feature/auth"), "feature-auth");
        assert_eq!(sanitize_branch("release/v1/hotfix"), "release-v1-hotfix");
        assert_eq!(sanitize_branch("main"), "main");
        assert_eq!(sanitize_branch(""), "");
    }

    #[test]
    fn expand_tilde_home() {
        let home = dirs::home_dir().expect("HOME must be set in tests");
        assert_eq!(expand_tilde(Path::new("~/foo")), home.join("foo"));
        assert_eq!(expand_tilde(Path::new("~")), home);
        assert_eq!(expand_tilde(Path::new("/abs/path")), PathBuf::from("/abs/path"));
        assert_eq!(expand_tilde(Path::new("rel/path")), PathBuf::from("rel/path"));
    }

    #[test]
    fn derive_name_uses_leaf() {
        let p = Path::new("/tmp/foo/bar-feat");
        assert_eq!(derive_worktree_name(p, "ignored"), "bar-feat");
    }

    #[test]
    fn derive_name_falls_back_to_sanitized_branch() {
        // Root path has no file_name component.
        assert_eq!(
            derive_worktree_name(Path::new("/"), "feature/auth"),
            "feature-auth"
        );
    }
}
