//! `torii bisect` — binary search for the commit that introduced a bug.
//!
//! State machine wrapper over `git bisect`. libgit2 has no bisect
//! primitives, and git's implementation stores state in
//! `.git/BISECT_*` files plus reflog entries — reimplementing that
//! correctly is 300+ LOC of risk. Wrapper is ~80, behaves identically.
//!
//! Flow:
//!
//!   torii bisect start           # enter bisect mode
//!   torii bisect bad             # current HEAD is bad
//!   torii bisect good v0.6.0     # v0.6.0 was good
//!     # git checks out the midpoint; test; mark good/bad; repeat
//!   torii bisect good
//!     # or torii bisect bad / skip
//!   ... until git reports "FIRST BAD COMMIT" ...
//!   torii bisect reset           # restore HEAD to where you were

use crate::error::{Result, ToriiError};
use std::path::Path;
use std::process::Command;

pub fn start(repo_path: &Path, bad: Option<&str>, good: &[String]) -> Result<()> {
    let mut args = vec!["bisect".to_string(), "start".to_string()];
    if let Some(b) = bad {
        args.push(b.to_string());
    }
    for g in good {
        args.push(g.to_string());
    }
    run_git(repo_path, &args)
}

/// Mark the given (or current) commit as bad.
pub fn bad(repo_path: &Path, commit: Option<&str>) -> Result<()> {
    let mut args = vec!["bisect".to_string(), "bad".to_string()];
    if let Some(c) = commit {
        args.push(c.to_string());
    }
    run_git(repo_path, &args)
}

/// Mark the given (or current) commit as good.
pub fn good(repo_path: &Path, commit: Option<&str>) -> Result<()> {
    let mut args = vec!["bisect".to_string(), "good".to_string()];
    if let Some(c) = commit {
        args.push(c.to_string());
    }
    run_git(repo_path, &args)
}

/// Skip the current commit (e.g. it doesn't build, can't test it).
pub fn skip(repo_path: &Path, commit: Option<&str>) -> Result<()> {
    let mut args = vec!["bisect".to_string(), "skip".to_string()];
    if let Some(c) = commit {
        args.push(c.to_string());
    }
    run_git(repo_path, &args)
}

/// Exit bisect mode, restore HEAD to the branch you started on.
pub fn reset(repo_path: &Path) -> Result<()> {
    run_git(repo_path, &["bisect".to_string(), "reset".to_string()])
}

/// Print the current bisect log (useful to share/replay).
pub fn log(repo_path: &Path) -> Result<()> {
    run_git(repo_path, &["bisect".to_string(), "log".to_string()])
}

/// Run `cmd` for every candidate commit; mark good if exit 0, bad if
/// non-zero (or skip if exit 125, matching git's contract).
pub fn run(repo_path: &Path, cmd: &[String]) -> Result<()> {
    if cmd.is_empty() {
        return Err(ToriiError::Usage(
            "bisect run needs a command to execute".into(),
        ));
    }
    let mut args = vec!["bisect".to_string(), "run".to_string()];
    args.extend(cmd.iter().cloned());
    run_git(repo_path, &args)
}

fn run_git(repo_path: &Path, args: &[String]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .status()
        .map_err(|e| ToriiError::Subprocess {
            tool: "git".into(),
            message: format!("invoke git bisect: {e}"),
        })?;
    if !status.success() {
        return Err(ToriiError::Subprocess {
            tool: "git".into(),
            message: format!("git {} exited with {}", args.join(" "), status),
        });
    }
    Ok(())
}
