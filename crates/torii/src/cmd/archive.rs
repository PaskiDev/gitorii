//! `torii archive` — export a tree or commit as a tarball.
//!
//! Thin wrapper over `git archive`. Rationale: tar+zip+tar.gz are already
//! implemented correctly in git's contrib code, and adding the `tar` +
//! `flate2` crates as direct dependencies would balloon build time for
//! marginal gain. `git archive` is in every standard git install.
//!
//! Examples (resolved at the CLI layer):
//!   torii archive HEAD -o release.tar.gz
//!   torii archive v0.6.9 --prefix=gitorii-0.6.9/ -o gitorii-0.6.9.tar.gz
//!   torii archive HEAD --format=zip -o release.zip

use crate::error::{Result, ToriiError};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Default)]
pub struct Opts {
    /// Output file. When `None`, archive is written to stdout (matches
    /// `git archive` default; only useful in pipelines).
    pub output: Option<String>,
    /// Force a specific format. Without this, git infers from `--output`'s
    /// extension (`.tar`, `.tar.gz`, `.zip`). Valid values: `tar`, `zip`,
    /// `tar.gz`, `tgz`.
    pub format: Option<String>,
    /// Prepend `prefix/` to every entry. Common for release tarballs so
    /// they unpack into a versioned subdirectory.
    pub prefix: Option<String>,
}

pub fn archive(repo_path: &Path, revision: &str, opts: &Opts) -> Result<()> {
    let mut args = vec!["archive".to_string()];
    if let Some(fmt) = &opts.format {
        args.push(format!("--format={fmt}"));
    }
    if let Some(prefix) = &opts.prefix {
        args.push(format!("--prefix={prefix}"));
    }
    if let Some(out) = &opts.output {
        args.push(format!("--output={out}"));
    }
    args.push(revision.to_string());

    println!("📦 archive  rev={revision}");
    let status = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .status()
        .map_err(|e| ToriiError::Subprocess { tool: "git".into(), message: format!("invoke git archive: {e}") })?;
    if !status.success() {
        return Err(ToriiError::Subprocess { tool: "git".into(), message: format!(
            "git archive exited with {status}"
        ) });
    }
    if let Some(out) = &opts.output {
        println!("✅ Written: {out}");
    }
    Ok(())
}
