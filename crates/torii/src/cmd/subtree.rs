//! Subtree — merge another project's history into a subdirectory of this
//! repo, preserving its commits but flattening it into our tree. Inverse
//! design choice from submodules: the embedded repo's history becomes
//! part of ours (no second clone, no `.gitmodules`), but we have to
//! track upstream changes via explicit `pull`/`push` instead of a commit
//! pin.
//!
//! Five entry points, all thin wrappers over `git subtree`:
//!
//! - [`add`]   — initial import of a remote at `--prefix=<dir>`.
//! - [`pull`]  — fetch and merge upstream updates.
//! - [`push`]  — extract the subdirectory's history and push it back.
//! - [`split`] — extract the subdirectory's history into a new branch
//!   without pushing.
//! - [`merge`] — finish a manual conflict resolution after pull.
//!
//! **Why a wrapper, not a from-scratch reimplementation?**
//!
//! `git subtree` is an official git contrib script (~800 lines of bash,
//! refined since 2009) that handles a long tail of edge cases — orphan
//! commits, --squash semantics, parent-detection when a subtree has been
//! moved, history rewriting through merge bases. Reimplementing those
//! correctly in Rust on top of libgit2 (which has no subtree primitives)
//! would be 1k+ LOC with bugs the upstream script has already squashed.
//! Trading 1k LOC of risk for `Command::new("git")` is the right call.
//!
//! What torii adds:
//! - Consistent emoji-tagged output before invoking git.
//! - Sane error messages when `git-subtree` isn't installed (it ships
//!   separately from `git` itself on some distros).
//! - Default `--squash` opt-in via flag, matching the pattern most users
//!   want (one merge commit per pull, not the whole upstream graph).

use crate::error::{Result, ToriiError};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Default)]
pub struct CommonOpts {
    /// Pass `--squash` to flatten the imported history into a single
    /// merge commit. Default for `add` and `pull` when the user opts in.
    pub squash: bool,
}

/// Make sure `git subtree` is reachable before we invoke it; otherwise
/// the error is a misleading `exit code 1`.
fn ensure_git_subtree() -> Result<()> {
    let probe = Command::new("git")
        .args(["subtree", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match probe {
        Ok(s) if s.success() => Ok(()),
        Ok(_) | Err(_) => Err(ToriiError::Subprocess {
            tool: "git".into(),
            message: "`git subtree` is not available. On Debian/Ubuntu: install `git-subtree`. \
             On Arch: `pacman -S git` (it's in the main package). \
             On Fedora: `dnf install git-subtree`."
                .to_string(),
        }),
    }
}

/// Initial import — `git subtree add --prefix=<dir> <url> <ref>`.
pub fn add(
    repo_path: &Path,
    prefix: &str,
    url: &str,
    refname: &str,
    opts: &CommonOpts,
) -> Result<()> {
    ensure_git_subtree()?;
    println!("🌿 subtree add  prefix={prefix}  url={url}  ref={refname}");
    let mut args = vec!["subtree".to_string(), "add".to_string()];
    if opts.squash {
        args.push("--squash".to_string());
    }
    args.push(format!("--prefix={prefix}"));
    args.push(url.to_string());
    args.push(refname.to_string());
    run_git(repo_path, &args)
}

/// Fetch + merge upstream — `git subtree pull --prefix=<dir> <url> <ref>`.
pub fn pull(
    repo_path: &Path,
    prefix: &str,
    url: &str,
    refname: &str,
    opts: &CommonOpts,
) -> Result<()> {
    ensure_git_subtree()?;
    println!("⬇  subtree pull  prefix={prefix}  url={url}  ref={refname}");
    let mut args = vec!["subtree".to_string(), "pull".to_string()];
    if opts.squash {
        args.push("--squash".to_string());
    }
    args.push(format!("--prefix={prefix}"));
    args.push(url.to_string());
    args.push(refname.to_string());
    run_git(repo_path, &args)
}

/// Extract + push the subtree back — `git subtree push --prefix=<dir>
/// <url> <ref>`. Note: no `--squash` here; push always pushes real
/// history.
pub fn push(repo_path: &Path, prefix: &str, url: &str, refname: &str) -> Result<()> {
    ensure_git_subtree()?;
    println!("⬆  subtree push  prefix={prefix}  url={url}  ref={refname}");
    let args = vec![
        "subtree".to_string(),
        "push".to_string(),
        format!("--prefix={prefix}"),
        url.to_string(),
        refname.to_string(),
    ];
    run_git(repo_path, &args)
}

/// Split the subtree's history into a new branch — `git subtree split
/// --prefix=<dir> [-b <branch>] [--annotate=<prefix>]`. With `-b`, the
/// branch is created locally; without it, the resulting commit OID is
/// printed.
pub fn split(
    repo_path: &Path,
    prefix: &str,
    branch: Option<&str>,
    annotate: Option<&str>,
) -> Result<()> {
    ensure_git_subtree()?;
    println!("✂  subtree split  prefix={prefix}");
    let mut args = vec![
        "subtree".to_string(),
        "split".to_string(),
        format!("--prefix={prefix}"),
    ];
    if let Some(b) = branch {
        args.push("-b".to_string());
        args.push(b.to_string());
    }
    if let Some(a) = annotate {
        args.push(format!("--annotate={a}"));
    }
    run_git(repo_path, &args)
}

/// Finish a hand-resolved conflict — `git subtree merge --prefix=<dir>
/// <ref>`. Rare; usually only after a `pull` left conflicts behind.
pub fn merge(repo_path: &Path, prefix: &str, refname: &str, opts: &CommonOpts) -> Result<()> {
    ensure_git_subtree()?;
    println!("🔀 subtree merge  prefix={prefix}  ref={refname}");
    let mut args = vec!["subtree".to_string(), "merge".to_string()];
    if opts.squash {
        args.push("--squash".to_string());
    }
    args.push(format!("--prefix={prefix}"));
    args.push(refname.to_string());
    run_git(repo_path, &args)
}

/// Shared invocation. Inherits stdio so the user sees the full git
/// output (progress, merge messages, conflict notices); we don't try to
/// reformat it.
fn run_git(repo_path: &Path, args: &[String]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .status()
        .map_err(|e| ToriiError::Subprocess {
            tool: "git".into(),
            message: format!("invoke git: {e}"),
        })?;
    if !status.success() {
        return Err(ToriiError::Subprocess {
            tool: "git".into(),
            message: format!("git {} exited with {}", args.join(" "), status),
        });
    }
    Ok(())
}
