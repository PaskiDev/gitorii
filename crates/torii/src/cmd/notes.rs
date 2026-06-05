//! `torii notes` — annotations attached to commits, kept in a separate
//! `refs/notes/commits` namespace so they don't alter commit OIDs.
//!
//! Wrapper over `git notes`. libgit2 has `Repository::note*` primitives,
//! but the porcelain UX (editor invocation for `add`/`edit`, merge
//! semantics for `copy`, message rewriting) is fiddly enough that
//! mirroring `git notes` 1:1 is cheaper than reimplementing it.
//!
//! Subcommands matched to `git notes`:
//!   add | append | show | edit | copy | remove | list
//! Plus a torii nicety: `torii notes` with no args lists every commit
//! that has notes (≡ `git notes list`).

use crate::error::{Result, ToriiError};
use std::path::Path;
use std::process::Command;

pub fn add(repo_path: &Path, commit: &str, message: Option<&str>, force: bool) -> Result<()> {
    let mut args = vec!["notes".to_string(), "add".to_string()];
    if force {
        args.push("-f".to_string());
    }
    if let Some(msg) = message {
        args.push("-m".to_string());
        args.push(msg.to_string());
    }
    args.push(commit.to_string());
    run_git(repo_path, &args)
}

pub fn append(repo_path: &Path, commit: &str, message: &str) -> Result<()> {
    run_git(
        repo_path,
        &[
            "notes".to_string(),
            "append".to_string(),
            "-m".to_string(),
            message.to_string(),
            commit.to_string(),
        ],
    )
}

pub fn show(repo_path: &Path, commit: &str) -> Result<()> {
    run_git(
        repo_path,
        &["notes".to_string(), "show".to_string(), commit.to_string()],
    )
}

pub fn edit(repo_path: &Path, commit: &str) -> Result<()> {
    run_git(
        repo_path,
        &["notes".to_string(), "edit".to_string(), commit.to_string()],
    )
}

pub fn copy(repo_path: &Path, from: &str, to: &str, force: bool) -> Result<()> {
    let mut args = vec!["notes".to_string(), "copy".to_string()];
    if force {
        args.push("-f".to_string());
    }
    args.push(from.to_string());
    args.push(to.to_string());
    run_git(repo_path, &args)
}

pub fn remove(repo_path: &Path, commit: &str) -> Result<()> {
    run_git(
        repo_path,
        &["notes".to_string(), "remove".to_string(), commit.to_string()],
    )
}

pub fn list(repo_path: &Path) -> Result<()> {
    run_git(repo_path, &["notes".to_string(), "list".to_string()])
}

fn run_git(repo_path: &Path, args: &[String]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .status()
        .map_err(|e| ToriiError::Subprocess { tool: "git".into(), message: format!("invoke git notes: {e}") })?;
    if !status.success() {
        return Err(ToriiError::Subprocess { tool: "git".into(), message: format!(
            "git {} exited with {}",
            args.join(" "),
            status
        ) });
    }
    Ok(())
}
