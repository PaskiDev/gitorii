//! Submodules — embed another git repo at a specific path and commit
//! inside this one. Differs from worktrees (multiple checkouts of *one*
//! repo) and subtrees (history-rewriting merges of *another* repo
//! flattened into this one's tree).
//!
//! Seven entry points, the MVP cut for 0.6.9:
//!
//! - [`add`]      — register a new submodule, clone it, stage it for commit.
//! - [`status`]   — list submodules with HEAD, index, working-tree state.
//! - [`init`]     — copy `.gitmodules` URLs into `.git/config` (so update
//!                  knows where to fetch from). Idempotent.
//! - [`update`]   — fetch + checkout the commit each submodule is pinned
//!                  at. Equivalent to `git submodule update --init`.
//! - [`sync`]     — re-copy `.gitmodules` URLs into `.git/config` (useful
//!                  after the upstream submodule URL changes).
//! - [`foreach`]  — run an arbitrary command inside each submodule's
//!                  working directory, with `SUBMODULE_NAME` / `_PATH` in
//!                  the environment. Stops at the first failure.
//! - [`remove`]   — deregister a submodule cleanly: scrub `.gitmodules`,
//!                  remove from `.git/config`, delete `.git/modules/<n>`,
//!                  and `git rm` the working-tree path. Stages the
//!                  deletion for commit.
//!
//! Design notes:
//!
//! - **Submodules are quirky.** Their state is scattered across three
//!   files (`.gitmodules`, `.git/config`, `.git/modules/<n>/config`) plus
//!   the index entry. The libgit2 API hides some of this but not all of
//!   it — `remove` in particular drives most of it manually because
//!   libgit2 has no `submodule_remove`.
//! - **`foreach` shells out via `$SHELL -c`.** We don't try to parse the
//!   command; pass anything that works in your shell. Two env vars set:
//!   `TORII_SUBMODULE_NAME` and `TORII_SUBMODULE_PATH` (relative to
//!   super-repo root).
//! - **Recursion is not supported in 0.6.9.** `update --recursive` is
//!   the headline missing flag. Add later.

use crate::error::{Result, ToriiError};
use git2::{Repository, SubmoduleUpdateOptions};
use std::path::Path;
use std::process::Command;

// -- add --------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct AddOpts {
    /// Branch to track (written to `.gitmodules` as `submodule.X.branch`).
    /// When `None` the submodule is pinned to whatever commit `clone`
    /// checks out (typically the remote default branch's tip).
    pub branch: Option<String>,
    /// Optional name for the submodule. Defaults to the path.
    pub name: Option<String>,
    /// After the top-level submodule is cloned, recursively initialise
    /// and update any nested submodules it contains. Off by default.
    pub recursive: bool,
}

/// Register a new submodule at `path` cloned from `url`, then stage it.
pub fn add(repo_path: &Path, url: &str, path: &Path, opts: &AddOpts) -> Result<()> {
    let mut repo = Repository::open(repo_path).map_err(ToriiError::Git)?;

    let abs_target = repo.workdir()
        .ok_or_else(|| ToriiError::InvalidConfig("repo has no working directory (bare)".into()))?
        .join(path);
    if abs_target.exists() {
        return Err(ToriiError::InvalidConfig(format!(
            "{} already exists. Submodule paths must be empty.",
            abs_target.display()
        )));
    }

    let sm_name = opts
        .name
        .clone()
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    // 1. Register in .gitmodules + .git/config, clone the contents, and
    //    stage the result. We bind the Submodule handle in an inner
    //    scope so it's dropped before we go back to mutating `repo` for
    //    set_branch — libgit2's Submodule borrows the parent immutably.
    let workdir_oid = {
        let mut sm = repo
            .submodule(url, path, true)
            .map_err(ToriiError::Git)?;

        // 2. Actually clone the contents. Default options fetch the
        //    remote and check out the ref recorded in the gitlink.
        let mut clone_opts = SubmoduleUpdateOptions::new();
        let _cloned = sm.clone(Some(&mut clone_opts)).map_err(ToriiError::Git)?;

        // 3. Add to the super-repo's index + finalize .gitmodules entry.
        sm.add_to_index(true).map_err(ToriiError::Git)?;
        sm.add_finalize().map_err(ToriiError::Git)?;

        sm.workdir_id()
    };

    // 4. Branch tracking (optional) — must happen after the Submodule
    //    handle is dropped because submodule_set_branch needs &mut repo.
    if let Some(branch) = &opts.branch {
        repo.submodule_set_branch(&sm_name, branch)
            .map_err(ToriiError::Git)?;
    }

    println!(
        "📦 Submodule added\n   url:    {}\n   path:   {}\n   commit: {}",
        url,
        path.display(),
        workdir_oid
            .map(|o| o.to_string()[..7].to_string())
            .unwrap_or_else(|| "?".to_string())
    );
    if let Some(branch) = &opts.branch {
        println!("   branch: {branch}");
    }

    // Recursive: if the freshly-cloned submodule has its own .gitmodules,
    // init+update everything underneath. Walks via the submodule's own
    // libgit2 handle, not by shelling out, so behaviour stays in-process.
    if opts.recursive {
        let nested_root = repo
            .workdir()
            .ok_or_else(|| ToriiError::InvalidConfig("bare repo".into()))?
            .join(path);
        recurse_update(&nested_root, true)?;
    }

    println!("\n💡 Don't forget to commit:  torii save -am \"add submodule {}\"", path.display());

    Ok(())
}

/// Internal helper: init+update every submodule of `repo_path`, descending
/// into each one. Called by `add --recursive` and `update --recursive`.
fn recurse_update(repo_path: &Path, init_missing: bool) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let mut subs = repo.submodules().map_err(ToriiError::Git)?;
    if subs.is_empty() {
        return Ok(());
    }
    for sm in &mut subs {
        let name = sm.name().unwrap_or("?").to_string();
        let mut up_opts = SubmoduleUpdateOptions::new();
        sm.update(init_missing, Some(&mut up_opts))
            .map_err(|e| ToriiError::InvalidConfig(format!("recurse update {name}: {e}")))?;
        // Now descend.
        let child_path = sm.path().to_path_buf();
        let child_abs = repo
            .workdir()
            .ok_or_else(|| ToriiError::InvalidConfig("bare repo".into()))?
            .join(&child_path);
        if child_abs.exists() {
            recurse_update(&child_abs, init_missing)?;
        }
    }
    Ok(())
}

// -- status -----------------------------------------------------------------

/// Print every submodule with its HEAD vs. index status. Mirrors
/// `git submodule status` but with torii's richer output.
pub fn status(repo_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let subs = repo.submodules().map_err(ToriiError::Git)?;

    if subs.is_empty() {
        println!("📦 No submodules in this repo.");
        return Ok(());
    }

    println!("📦 Submodules:\n");
    for sm in &subs {
        let name = sm.name().unwrap_or("?");
        let path = sm.path().display();
        let url = sm.url().unwrap_or("(no url)");
        let head = sm
            .head_id()
            .map(|o| o.to_string()[..7].to_string())
            .unwrap_or_else(|| "—".to_string());
        let wd = sm
            .workdir_id()
            .map(|o| o.to_string()[..7].to_string())
            .unwrap_or_else(|| "(not cloned)".to_string());
        let state = describe_submodule_state(&repo, name).unwrap_or_else(|_| "?".to_string());

        println!("  • {name}");
        println!("      path:   {path}");
        println!("      url:    {url}");
        println!("      head:   {head}    working: {wd}    state: {state}");
    }

    Ok(())
}

fn describe_submodule_state(repo: &Repository, name: &str) -> Result<String> {
    let status = repo
        .submodule_status(name, git2::SubmoduleIgnore::None)
        .map_err(ToriiError::Git)?;

    let mut parts = Vec::new();
    if status.contains(git2::SubmoduleStatus::IN_HEAD) {
        // expected, normal
    }
    if !status.contains(git2::SubmoduleStatus::IN_WD) {
        parts.push("not initialised".to_string());
    }
    if status.contains(git2::SubmoduleStatus::WD_UNINITIALIZED) {
        parts.push("uninitialised".to_string());
    }
    if status.contains(git2::SubmoduleStatus::WD_MODIFIED) {
        parts.push("modified".to_string());
    }
    if status.contains(git2::SubmoduleStatus::INDEX_MODIFIED)
        || status.contains(git2::SubmoduleStatus::WD_INDEX_MODIFIED)
    {
        parts.push("staged changes".to_string());
    }
    if status.contains(git2::SubmoduleStatus::WD_WD_MODIFIED) {
        parts.push("dirty working tree".to_string());
    }
    if status.contains(git2::SubmoduleStatus::WD_UNTRACKED) {
        parts.push("untracked files".to_string());
    }
    if parts.is_empty() {
        parts.push("clean".to_string());
    }
    Ok(parts.join(", "))
}

// -- init -------------------------------------------------------------------

/// Copy URLs from `.gitmodules` into `.git/config` so `update` knows
/// where to fetch each submodule from. Idempotent.
pub fn init(repo_path: &Path, force: bool) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let mut subs = repo.submodules().map_err(ToriiError::Git)?;

    if subs.is_empty() {
        println!("📦 No submodules to initialise.");
        return Ok(());
    }

    for sm in &mut subs {
        let name = sm.name().unwrap_or("?").to_string();
        sm.init(force).map_err(ToriiError::Git)?;
        println!("🔧 Initialised: {name}");
    }
    println!("\n✅ Initialised {} submodule(s).", subs.len());
    Ok(())
}

// -- update -----------------------------------------------------------------

#[derive(Debug, Default)]
pub struct UpdateOpts {
    /// Run `init` first for submodules that aren't yet initialised. Default
    /// off (matches `git submodule update`); pass `true` to mimic
    /// `git submodule update --init`.
    pub init: bool,
    /// Recurse into nested submodules after updating each top-level one.
    /// Off by default (one-level update only).
    pub recursive: bool,
}

/// Fetch and checkout each submodule at the commit the super-repo records.
pub fn update(repo_path: &Path, opts: &UpdateOpts) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let mut subs = repo.submodules().map_err(ToriiError::Git)?;

    if subs.is_empty() {
        println!("📦 No submodules to update.");
        return Ok(());
    }

    for sm in &mut subs {
        let name = sm.name().unwrap_or("?").to_string();
        let mut up_opts = SubmoduleUpdateOptions::new();
        sm.update(opts.init, Some(&mut up_opts))
            .map_err(|e| ToriiError::InvalidConfig(format!("update {name}: {e}")))?;
        let at = sm
            .workdir_id()
            .map(|o| o.to_string()[..7].to_string())
            .unwrap_or_else(|| "?".to_string());
        println!("⬆  {name}  →  {at}");

        if opts.recursive {
            let child = sm.path().to_path_buf();
            let child_abs = repo
                .workdir()
                .ok_or_else(|| ToriiError::InvalidConfig("bare repo".into()))?
                .join(&child);
            if child_abs.exists() {
                recurse_update(&child_abs, opts.init)?;
            }
        }
    }
    println!("\n✅ Updated {} submodule(s){}.", subs.len(),
        if opts.recursive { " (recursive)" } else { "" });
    Ok(())
}

// -- sync -------------------------------------------------------------------

/// Re-copy URLs from `.gitmodules` into `.git/config`. Useful after an
/// upstream submodule URL changes (server moved, fork-then-merge, etc.).
pub fn sync(repo_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let mut subs = repo.submodules().map_err(ToriiError::Git)?;

    if subs.is_empty() {
        println!("📦 No submodules to sync.");
        return Ok(());
    }
    for sm in &mut subs {
        let name = sm.name().unwrap_or("?").to_string();
        sm.sync().map_err(ToriiError::Git)?;
        println!("🔄 Synced: {name}");
    }
    println!("\n✅ Synced {} submodule(s).", subs.len());
    Ok(())
}

// -- foreach ----------------------------------------------------------------

/// Run `cmd` in each submodule's working directory via `$SHELL -c`. Exports
/// `TORII_SUBMODULE_NAME` and `TORII_SUBMODULE_PATH`. Stops at the first
/// non-zero exit.
pub fn foreach(repo_path: &Path, cmd: &str) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let subs = repo.submodules().map_err(ToriiError::Git)?;

    if subs.is_empty() {
        println!("📦 No submodules.");
        return Ok(());
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let workdir = repo
        .workdir()
        .ok_or_else(|| ToriiError::InvalidConfig("bare repo".into()))?
        .to_path_buf();

    for sm in &subs {
        let name = sm.name().unwrap_or("?").to_string();
        let path = sm.path().to_path_buf();
        let abs = workdir.join(&path);
        if !abs.exists() {
            println!("⏭  {name} (not initialised, skipping)");
            continue;
        }
        println!("▶ {name}  ({})", path.display());
        let status = Command::new(&shell)
            .args(["-c", cmd])
            .current_dir(&abs)
            .env("TORII_SUBMODULE_NAME", &name)
            .env("TORII_SUBMODULE_PATH", path.to_string_lossy().as_ref())
            .status()
            .map_err(|e| ToriiError::InvalidConfig(format!("spawn shell: {e}")))?;
        if !status.success() {
            return Err(ToriiError::InvalidConfig(format!(
                "foreach stopped: '{cmd}' exited {status} in {name}"
            )));
        }
    }
    Ok(())
}

// -- remove -----------------------------------------------------------------

/// Deregister a submodule cleanly. This is what libgit2 doesn't do for
/// you — there's no `Submodule::remove`. We scrub each of the four places
/// submodule state lives:
///
///   1. `.gitmodules` — strip the `[submodule "name"]` section.
///   2. `.git/config` — same, but in the local config.
///   3. `.git/modules/<name>/` — the cached gitdir (so re-add starts
///      clean).
///   4. The working-tree path — rm it and stage the deletion in the
///      super-repo's index.
///
/// The user still needs to commit the resulting state.
pub fn remove(repo_path: &Path, path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| ToriiError::InvalidConfig("bare repo".into()))?
        .to_path_buf();

    // Find the submodule whose `path()` matches.
    let subs = repo.submodules().map_err(ToriiError::Git)?;
    let target = subs
        .iter()
        .find(|s| s.path() == path)
        .ok_or_else(|| {
            ToriiError::InvalidConfig(format!(
                "{} is not a known submodule. Run 'torii submodule status' to list.",
                path.display()
            ))
        })?;
    let name = target.name().unwrap_or("?").to_string();
    let path = target.path().to_path_buf();

    // 1. Strip from .gitmodules.
    let gitmodules = workdir.join(".gitmodules");
    if gitmodules.exists() {
        strip_section_from_ini(&gitmodules, &format!("submodule \"{name}\""))?;
    }

    // 2. Strip from .git/config (local).
    let git_config = repo.path().join("config");
    strip_section_from_ini(&git_config, &format!("submodule \"{name}\""))?;

    // 3. Wipe .git/modules/<name>/.
    let cached_gitdir = repo.path().join("modules").join(&name);
    if cached_gitdir.exists() {
        std::fs::remove_dir_all(&cached_gitdir).map_err(|e| {
            ToriiError::InvalidConfig(format!(
                "remove cached gitdir {}: {}",
                cached_gitdir.display(),
                e
            ))
        })?;
    }

    // 4. Working-tree path + index. We do this through libgit2 directly
    //    rather than shelling out — `git rm --cached` refuses to run when
    //    `.gitmodules` has uncommitted changes, which is exactly the case
    //    we're in (we just stripped it). libgit2's index API doesn't
    //    care.
    let abs_path = workdir.join(&path);
    {
        let mut index = repo.index().map_err(ToriiError::Git)?;
        // remove_path is silent if the entry isn't present — fine.
        let _ = index.remove_path(&path);
        // Also remove anything inside the path (defence in depth).
        let _ = index.remove_dir(&path, 0);
        index.write().map_err(ToriiError::Git)?;
    }
    if abs_path.exists() {
        if let Err(e) = std::fs::remove_dir_all(&abs_path) {
            // Index/.gitmodules are already updated; leftover dir is a
            // partial state the user must clean up manually. Tell them
            // explicitly rather than report success.
            eprintln!(
                "⚠️  Submodule '{}' deregistered from index and .gitmodules,\n   but failed to remove working-tree dir {}: {}\n   Remove it manually: rm -rf {}",
                name,
                abs_path.display(),
                e,
                abs_path.display(),
            );
        }
    }

    println!(
        "🗑  Submodule '{}' deregistered.\n   Stage the result and commit:  torii save -am \"remove submodule {}\"",
        name,
        path.display()
    );
    Ok(())
}

/// Remove the `[<section>]` block (and its contiguous indented body) from
/// an INI-style file. Used for both `.gitmodules` and `.git/config`.
///
/// Crude: walks lines, skips from the matching header until either EOF or
/// the next `[…]` header. Adequate for git's own format because git
/// neither nests sections nor inlines them with comments inside.
fn strip_section_from_ini(file: &Path, section: &str) -> Result<()> {
    if !file.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(file).map_err(|e| {
        ToriiError::InvalidConfig(format!("read {}: {}", file.display(), e))
    })?;
    let target_header = format!("[{section}]");
    let mut out = String::with_capacity(content.len());
    let mut skipping = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == target_header {
            skipping = true;
            continue;
        }
        if skipping {
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                // Next section starts — stop skipping, keep this line.
                skipping = false;
            } else {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    std::fs::write(file, out).map_err(|e| {
        ToriiError::InvalidConfig(format!("write {}: {}", file.display(), e))
    })?;
    Ok(())
}

// -- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn strip_section_removes_block_only() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            "[core]\n\trepositoryformatversion = 0\n[submodule \"vendor/x\"]\n\turl = a\n\tpath = vendor/x\n[remote \"origin\"]\n\turl = b"
        )
        .unwrap();
        strip_section_from_ini(tmp.path(), "submodule \"vendor/x\"").unwrap();
        let out = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(out.contains("[core]"));
        assert!(out.contains("[remote \"origin\"]"));
        assert!(!out.contains("[submodule"));
        assert!(!out.contains("vendor/x"));
    }

    #[test]
    fn strip_section_no_op_on_missing() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "[core]\n\trepositoryformatversion = 0\n").unwrap();
        strip_section_from_ini(tmp.path(), "submodule \"absent\"").unwrap();
        let out = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(out.contains("[core]"));
    }

    #[test]
    fn strip_section_handles_eof_after_block() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            "[core]\n\trepositoryformatversion = 0\n[submodule \"x\"]\n\turl = u\n"
        )
        .unwrap();
        strip_section_from_ini(tmp.path(), "submodule \"x\"").unwrap();
        let out = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(!out.contains("submodule"));
    }
}
