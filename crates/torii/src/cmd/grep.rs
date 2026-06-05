//! `torii grep` — search tracked files for a pattern. Wrapper over
//! `git grep`, which is consistently faster than `ripgrep` on
//! tracked-only content because it indexes through the gitdir's pack
//! files. Different concern from `torii scan` (secrets pattern matching).
//!
//! Flags surfaced (others can be passed via `--`):
//!   - `--ignore-case` / `-i`
//!   - `--word-regexp` / `-w`
//!   - `--line-number` / `-n`  (on by default — git grep doesn't show
//!     line numbers without this; we flip the default)
//!   - `--files-with-matches` / `-l`
//!   - any literal extra args after `--`
//!
//! Anything not listed is passed through `--` so power users still have
//! full git grep semantics available.

use crate::error::{Result, ToriiError};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Default)]
pub struct Opts {
    pub ignore_case: bool,
    pub word_regexp: bool,
    pub files_with_matches: bool,
    /// Suppress line numbers (otherwise on by default).
    pub no_line_number: bool,
    /// Raw extra args appended after `--`.
    pub extra: Vec<String>,
}

pub fn grep(repo_path: &Path, pattern: &str, paths: &[String], opts: &Opts) -> Result<()> {
    let mut args = vec!["grep".to_string()];
    if !opts.no_line_number {
        args.push("-n".to_string());
    }
    if opts.ignore_case {
        args.push("-i".to_string());
    }
    if opts.word_regexp {
        args.push("-w".to_string());
    }
    if opts.files_with_matches {
        args.push("-l".to_string());
    }
    args.push(pattern.to_string());
    if !paths.is_empty() {
        args.push("--".to_string());
        args.extend(paths.iter().cloned());
    }
    args.extend(opts.extra.iter().cloned());

    let status = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .status()
        .map_err(|e| ToriiError::Subprocess { tool: "git".into(), message: format!("invoke git grep: {e}") })?;

    // git grep exits 1 when nothing matched — not an error from torii's
    // perspective, just zero results. Only propagate >1 as a real failure.
    match status.code() {
        Some(0) | Some(1) => Ok(()),
        Some(code) => Err(ToriiError::Subprocess { tool: "git".into(), message: format!(
            "git grep exited with status {code}"
        ) }),
        None => Err(ToriiError::Subprocess { tool: "git".into(), message: "git grep terminated by signal".into() }),
    }
}
