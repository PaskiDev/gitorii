//! `torii clean` — remove files the working tree has but git doesn't track.
//!
//! Equivalent to `git clean`. Defaults to a **dry-run** for safety (git's
//! default behaviour is the same: refuses to delete without `-f`).
//!
//! Flags:
//!   - `-f` / `--force`: actually delete (otherwise dry-run).
//!   - `-d`: include untracked directories.
//!   - `-x`: also remove `.gitignore`-matched files.
//!   - `-X`: only remove `.gitignore`-matched files (inverse of `-x`).
//!
//! 0.7.0 introduces this as a new top-level command. The previous
//! `torii history clean` (GC) is renamed to `torii history gc`; the old
//! name still works as a deprecated alias.

use crate::error::{Result, ToriiError};
use git2::{Repository, Status, StatusOptions};
use std::path::Path;

#[derive(Debug, Default)]
pub struct Opts {
    /// Actually delete files. Without this, just lists what would go.
    pub force: bool,
    /// Recurse into untracked directories.
    pub dirs: bool,
    /// Also remove ignored files.
    pub include_ignored: bool,
    /// Only remove ignored files (no untracked-non-ignored).
    pub only_ignored: bool,
}

pub fn clean(repo_path: &Path, opts: &Opts) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| ToriiError::InvalidConfig("bare repo".into()))?
        .to_path_buf();

    let mut so = StatusOptions::new();
    so.include_untracked(true);
    so.recurse_untracked_dirs(opts.dirs);
    so.include_ignored(opts.include_ignored || opts.only_ignored);

    let statuses = repo.statuses(Some(&mut so)).map_err(ToriiError::Git)?;

    let mut targets: Vec<(String, bool)> = Vec::new(); // (path, is_ignored)
    for entry in statuses.iter() {
        let st = entry.status();
        let is_untracked = st.contains(Status::WT_NEW);
        let is_ignored = st.contains(Status::IGNORED);

        let keep = match (opts.only_ignored, opts.include_ignored) {
            (true, _) => is_ignored,
            (false, true) => is_untracked || is_ignored,
            (false, false) => is_untracked,
        };

        if keep {
            if let Some(path) = entry.path() {
                targets.push((path.to_string(), is_ignored));
            }
        }
    }

    if targets.is_empty() {
        println!("✨ Nothing to clean.");
        return Ok(());
    }

    let action = if opts.force { "Removing" } else { "Would remove" };
    println!("🧹 {action}:");
    for (path, ignored) in &targets {
        let tag = if *ignored { " (ignored)" } else { "" };
        println!("  - {path}{tag}");
    }

    if !opts.force {
        println!(
            "\n(dry-run — pass -f to actually delete. {} entr{} matched.)",
            targets.len(),
            if targets.len() == 1 { "y" } else { "ies" }
        );
        return Ok(());
    }

    let mut removed = 0usize;
    let mut errors: Vec<(std::path::PathBuf, std::io::Error)> = Vec::new();
    for (path, _) in &targets {
        let abs = workdir.join(path);
        let meta = std::fs::symlink_metadata(&abs).ok();
        match meta {
            Some(m) if m.is_dir() => {
                if !opts.dirs {
                    continue; // git's behaviour: dirs need -d
                }
                match std::fs::remove_dir_all(&abs) {
                    Ok(_) => removed += 1,
                    Err(e) => errors.push((abs, e)),
                }
            }
            Some(_) => {
                match std::fs::remove_file(&abs) {
                    Ok(_) => removed += 1,
                    Err(e) => errors.push((abs, e)),
                }
            }
            None => {}
        }
    }
    println!("\n✅ Cleaned {} entr{}.", removed,
        if removed == 1 { "y" } else { "ies" });
    if !errors.is_empty() {
        eprintln!("\n⚠️  {} path(s) failed to remove:", errors.len());
        for (p, e) in &errors {
            eprintln!("   {} — {}", p.display(), e);
        }
        return Err(ToriiError::InvalidConfig(format!(
            "clean: {} path(s) failed (see above)", errors.len()
        )));
    }
    Ok(())
}
