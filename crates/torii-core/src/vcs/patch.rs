//! `torii patch` — export commits as patch files and apply them.
//!
//! Two subcommands:
//!
//!   - `torii patch export <range>` → ≡ `git format-patch <range>`
//!     Produces one `.patch` file per commit, suitable for email or
//!     archive.
//!   - `torii patch apply <file>...` → ≡ `git am <file>...`
//!     Applies one or more `.patch` files as new commits, preserving
//!     authorship and message.
//!
//! Wrapper rationale: same as `subtree` and `archive`. `git format-patch`
//! / `git am` have decades of edge-case handling around mailbox parsing,
//! base64 binary blobs, 3-way fallback, etc. Reimplementing those on top
//! of libgit2 would be 800-1500 LOC of risk; the wrapper is ~80.

use crate::error::{Result, ToriiError};
use std::path::{Path, PathBuf};
use std::process::Command;

// -- export -----------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ExportOpts {
    /// Output directory for the patch files. Default is cwd.
    pub output_dir: Option<PathBuf>,
    /// `--stdout` — write all patches to stdout instead of files.
    pub stdout: bool,
    /// Add a cover letter (`--cover-letter`).
    pub cover_letter: bool,
}

pub fn export(repo_path: &Path, range: &str, opts: &ExportOpts) -> Result<()> {
    let mut args = vec!["format-patch".to_string()];
    if let Some(dir) = &opts.output_dir {
        args.push("-o".to_string());
        args.push(dir.to_string_lossy().to_string());
    }
    if opts.stdout {
        args.push("--stdout".to_string());
    }
    if opts.cover_letter {
        args.push("--cover-letter".to_string());
    }
    args.push(range.to_string());

    println!("📨 patch export  range={range}");
    let status = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .status()
        .map_err(|e| ToriiError::Subprocess {
            tool: "git".into(),
            message: format!("invoke git format-patch: {e}"),
        })?;
    if !status.success() {
        return Err(ToriiError::Subprocess {
            tool: "git".into(),
            message: format!("git format-patch exited with {status}"),
        });
    }
    Ok(())
}

// -- apply ------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ApplyOpts {
    /// `--3way` — try 3-way merge if the patch doesn't apply cleanly.
    pub three_way: bool,
    /// `--abort` — bail out of an in-progress `am` session.
    pub abort: bool,
    /// `--continue` — resume after resolving conflicts.
    pub continue_: bool,
    /// `--skip` — drop the current patch and move on.
    pub skip: bool,
}

pub fn apply(repo_path: &Path, files: &[PathBuf], opts: &ApplyOpts) -> Result<()> {
    let mut args = vec!["am".to_string()];
    if opts.three_way {
        args.push("--3way".to_string());
    }
    if opts.abort {
        args.push("--abort".to_string());
    } else if opts.continue_ {
        args.push("--continue".to_string());
    } else if opts.skip {
        args.push("--skip".to_string());
    } else {
        if files.is_empty() {
            return Err(ToriiError::Usage(
                "patch apply needs at least one file, or --abort / --continue / --skip".into(),
            ));
        }
        for f in files {
            args.push(f.to_string_lossy().to_string());
        }
    }

    println!("📨 patch apply  ({} arg(s))", args.len() - 1);
    let status = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .status()
        .map_err(|e| ToriiError::Subprocess {
            tool: "git".into(),
            message: format!("invoke git am: {e}"),
        })?;
    if !status.success() {
        return Err(ToriiError::Subprocess {
            tool: "git".into(),
            message: format!(
                "git am exited with {status} — resolve and run `torii patch apply --continue`"
            ),
        });
    }
    Ok(())
}
