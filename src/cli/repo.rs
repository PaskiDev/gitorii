//! Daily-flow repository commands: init, save, sync, status, log,
//! diff, blame, cherry-pick, branch, show.

use super::scan::DEFAULT_COMMITS_POLICY;
use super::sign::{run_show_signature, SignOverrideGuard};
use crate::core::GitRepo;
use crate::mirror::MirrorManager;
use crate::scanner;
use anyhow::Result;
use std::path::PathBuf;

pub(crate) fn init(path: &Option<String>) -> Result<()> {
    let repo_path = path.as_deref().unwrap_or(".");
    GitRepo::init(repo_path)?;

    // Create .toriignore with sensible defaults
    let toriignore_path = std::path::Path::new(repo_path).join(".toriignore");
    if !toriignore_path.exists() {
        std::fs::write(
            &toriignore_path,
            crate::toriignore::ToriIgnore::default_content(),
        )
        .ok();
    }

    // Scaffold policies/commits.toml so `torii scan --commits` has
    // something to read out of the box.
    let policies_dir = std::path::Path::new(repo_path).join("policies");
    let commits_policy = policies_dir.join("commits.toml");
    if !commits_policy.exists() {
        let _ = std::fs::create_dir_all(&policies_dir);
        let _ = std::fs::write(&commits_policy, DEFAULT_COMMITS_POLICY);
    }

    // Sync .toriignore → .git/info/exclude immediately
    let repo = GitRepo::open(repo_path)?;
    repo.sync_toriignore()?;

    println!("✅ Initialized repository at {}", repo_path);
    println!("   Created .toriignore with default patterns");
    println!("   Created policies/commits.toml (run: torii scan --commits)");
    Ok(())
}

// Mirrors the clap variant fields 1:1 — a param struct would just duplicate the enum.
#[allow(clippy::too_many_arguments)]
pub(crate) fn save(
    message: &Option<String>,
    all: &bool,
    files: &[PathBuf],
    amend: &bool,
    revert: &Option<String>,
    reset: &Option<String>,
    reset_mode: &String,
    unstage: &bool,
    skip_hooks: &bool,
    sign: &bool,
    no_sign: &bool,
) -> Result<()> {
    let repo = GitRepo::open(".")?;

    // 0.7.35 — translate `-S` / `--no-sign` into the
    // env-var that `commit_inner_split` reads. The guard
    // restores the previous value on drop so we don't leak
    // the override into any subprocess invoked later in
    // the same process.
    let _sign_guard = SignOverrideGuard::new(if *sign {
        Some(true)
    } else if *no_sign {
        Some(false)
    } else {
        None
    });

    if *unstage {
        if *all {
            if !files.is_empty() {
                anyhow::bail!("Pass either --all or specific paths, not both");
            }
            repo.unstage_all()?;
            println!("✅ Unstaged all paths");
        } else {
            if files.is_empty() {
                anyhow::bail!("Provide at least one path or use --all");
            }
            repo.unstage(files)?;
            println!("✅ Unstaged {} path(s)", files.len());
        }
        return Ok(());
    }

    if let Some(commit_hash) = reset {
        repo.reset_commit(commit_hash, reset_mode)?;
        println!("✅ Reset to commit: {} (mode: {})", commit_hash, reset_mode);
    } else if let Some(commit_hash) = revert {
        repo.revert_commit(commit_hash)?;
        println!("✅ Reverted commit: {}", commit_hash);
    } else {
        if *all && !files.is_empty() {
            anyhow::bail!("Cannot use --all and specific files at the same time");
        }
        if *all {
            repo.add_all()?;
        } else if !files.is_empty() {
            repo.add(files)?;
        }

        // Scan staged files for sensitive data before committing
        let repo_path = std::path::Path::new(".");

        // Load .toriignore (sections: secrets/size/hooks)
        let ti = crate::toriignore::ToriIgnore::load(repo_path)?;

        // [size] guard
        let staged = scanner::staged_paths(repo_path).unwrap_or_default();
        crate::hooks::check_size(&ti.size, repo_path, &staged)?;

        // [hooks] pre-save
        if !*skip_hooks {
            crate::hooks::pre_save(&ti.hooks, repo_path)?;
        }

        let mut findings = scanner::scan_staged(repo_path)?;
        // [secrets] custom regex rules
        findings.extend(scanner::scan_staged_with_custom(repo_path, &ti.secrets)?);
        if !findings.is_empty() {
            println!("⚠️  Sensitive data detected in staged files:\n");
            for f in &findings {
                if f.line == 0 {
                    println!("   {} — {}", f.file, f.pattern_name);
                } else {
                    println!("   {}:{} — {}", f.file, f.line, f.pattern_name);
                }
                println!("   {}\n", f.preview);
            }
            println!("💡 Tip: use .env.example for placeholder values — those files are always safe to commit.");
            print!("   Continue anyway? [y/N] ");
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("❌ Commit cancelled.");
                return Ok(());
            }
        }

        let msg = message
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--message/-m is required for commit/amend"))?;
        if *amend {
            repo.commit_amend(msg)?;
            println!("✅ Commit amended: {}", msg);
        } else {
            repo.commit(msg)?;
            println!("✅ Changes saved: {}", msg);
        }
        if !*skip_hooks {
            crate::hooks::post_save(&ti.hooks, repo_path);
        }
    }
    Ok(())
}

// Mirrors the clap variant fields 1:1 — a param struct would just duplicate the enum.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync(
    branch: &Option<String>,
    pull: &bool,
    push: &bool,
    force: &bool,
    fetch: &bool,
    all: &bool,
    merge: &bool,
    rebase: &bool,
    preview: &bool,
    verify: &bool,
    skip_hooks: &bool,
) -> Result<()> {
    let repo = GitRepo::open(".")?;
    let repo_path = std::path::Path::new(".");
    let ti = crate::toriignore::ToriIgnore::load(repo_path)?;
    if !*skip_hooks {
        crate::hooks::pre_sync(&ti.hooks, repo_path)?;
    }

    if *verify {
        repo.verify_remote()?;
        return Ok(());
    }

    // --fetch wins over the integrate-branch interpretation: when
    // --fetch is present, the positional argument is a remote name.
    if *fetch {
        if *all {
            repo.fetch_all()?;
        } else if let Some(remote_name) = branch {
            repo.fetch_named(remote_name)?;
        } else {
            repo.fetch()?;
            println!("✅ Fetched from remote");
        }
    } else if let Some(branch_name) = branch {
        if *preview {
            println!("🔍 Preview: Would integrate branch '{}'", branch_name);
            println!("💡 Recommendation: Use merge for feature branches, rebase for clean history");
        } else if *merge {
            println!("🔀 Merging branch '{}'...", branch_name);
            repo.merge_branch(branch_name)?;
            println!("✅ Merged branch: {}", branch_name);
        } else if *rebase {
            println!("🔄 Rebasing onto branch '{}'...", branch_name);
            repo.rebase_branch(branch_name)?;
            println!("✅ Rebased onto: {}", branch_name);
        } else {
            // Smart integration (default to merge for now)
            println!("🔀 Integrating branch '{}'...", branch_name);
            repo.merge_branch(branch_name)?;
            println!("✅ Integrated branch: {}", branch_name);
        }
    } else if *force {
        repo.push(true)?;
        println!("✅ Force synced with remote");
        let mirror_mgr = MirrorManager::new(".")?;
        mirror_mgr.sync_replicas_if_any(true)?;
    } else if *pull {
        repo.pull()?;
        println!("✅ Pulled from remote");
    } else if *push {
        repo.push(false)?;
        println!("✅ Pushed to remote");
        let mirror_mgr = MirrorManager::new(".")?;
        mirror_mgr.sync_replicas_if_any(false)?;
    } else {
        // Default: pull then push
        repo.pull()?;
        repo.push(false)?;
        println!("✅ Synced with remote");
        // Also sync replica mirrors if any are configured
        let mirror_mgr = MirrorManager::new(".")?;
        mirror_mgr.sync_replicas_if_any(false)?;
    }
    if !*skip_hooks {
        crate::hooks::post_sync(&ti.hooks, repo_path);
    }
    Ok(())
}

pub(crate) fn status(tracked: &bool, null: &bool) -> Result<()> {
    if *tracked {
        // ls-files behaviour: walk the index and print each entry.
        let repo = git2::Repository::open(".")?;
        let index = repo.index()?;
        let sep = if *null { '\0' } else { '\n' };
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        for entry in index.iter() {
            let path = String::from_utf8_lossy(&entry.path);
            write!(out, "{}{}", path, sep)?;
        }
    } else {
        let repo = GitRepo::open(".")?;
        print_status(&repo.status()?);
    }
    Ok(())
}

/// Render a [`RepoStatus`] with context and next-step suggestions.
/// Presentation only — the data comes from `GitRepo::status()`.
fn print_status(st: &crate::core::RepoStatus) {
    use crate::core::ChangeKind;

    fn prefix(kind: ChangeKind) -> &'static str {
        match kind {
            ChangeKind::Added => "A ",
            ChangeKind::Modified => "M ",
            ChangeKind::Deleted => "D ",
        }
    }

    println!("📊 Repository Status\n");
    println!("Branch: {}", st.branch);

    if let Some(head) = &st.head {
        let timestamp = chrono::DateTime::from_timestamp(head.seconds_since_epoch, 0)
            .unwrap_or_default();
        let duration = chrono::Utc::now().signed_duration_since(timestamp);
        let time_ago = if duration.num_days() > 0 {
            format!("{} days ago", duration.num_days())
        } else if duration.num_hours() > 0 {
            format!("{} hours ago", duration.num_hours())
        } else if duration.num_minutes() > 0 {
            format!("{} minutes ago", duration.num_minutes())
        } else {
            "just now".to_string()
        };
        println!("Commit: {} - \"{}\" ({})", head.short_id, head.summary, time_ago);
    }

    if let Some(remote) = &st.remote {
        print!("Remote: {}", remote.name);
        if let Some((ahead, behind)) = remote.ahead_behind {
            if ahead > 0 || behind > 0 {
                print!(" (");
                if ahead > 0 {
                    print!("{} ahead", ahead);
                }
                if ahead > 0 && behind > 0 {
                    print!(", ");
                }
                if behind > 0 {
                    print!("{} behind", behind);
                }
                print!(")");
            } else {
                print!(" (up to date)");
            }
        }
        println!();
    }

    println!();

    if st.is_clean() {
        println!("✨ Working tree clean");
    } else {
        if !st.staged.is_empty() {
            println!("✅ Changes staged for commit:");
            for e in &st.staged {
                println!("  {} {}", prefix(e.kind), e.path);
            }
            println!();
        }

        if !st.unstaged.is_empty() {
            println!("📝 Changes not staged:");
            for e in &st.unstaged {
                println!("  {} {}", prefix(e.kind), e.path);
            }
            println!();
        }

        if !st.untracked.is_empty() {
            println!("📦 Untracked files:");
            for p in &st.untracked {
                println!("  ?? {}", p);
            }
            println!();
        }
    }

    println!("💡 Next steps:");
    if st.is_clean() {
        println!("  • Start new work: torii branch feature-name -c");
        println!("  • Update from remote: torii sync");
        println!("  • Create snapshot: torii snapshot create");
    } else if !st.staged.is_empty() && st.unstaged.is_empty() && st.untracked.is_empty() {
        println!("  • Commit staged changes: torii save -m \"message\"");
        println!("  • See staged changes: torii diff --staged");
    } else if !st.unstaged.is_empty() || !st.untracked.is_empty() {
        println!("  • Save all changes: torii save -am \"message\"");
        println!("  • See changes: torii diff");
        if !st.staged.is_empty() {
            println!("  • Commit only staged: torii save -m \"message\"");
        }
    }
}

// Mirrors the clap variant fields 1:1 — a param struct would just duplicate the enum.
#[allow(clippy::too_many_arguments)]
pub(crate) fn log(
    count: &Option<usize>,
    oneline: &bool,
    graph: &bool,
    author: &Option<String>,
    since: &Option<String>,
    until: &Option<String>,
    grep: &Option<String>,
    stat: &bool,
    reflog: &bool,
    signatures: &bool,
) -> Result<()> {
    let repo = GitRepo::open(".")?;
    if *reflog {
        repo.show_reflog(count.unwrap_or(20))?;
    } else {
        repo.log(
            *count,
            *oneline,
            *graph,
            author.as_deref(),
            since.as_deref(),
            until.as_deref(),
            grep.as_deref(),
            *stat,
            *signatures,
        )?;
    }
    Ok(())
}

pub(crate) fn diff(staged: &bool, last: &bool) -> Result<()> {
    let repo = GitRepo::open(".")?;
    repo.diff(*staged, *last)?;
    Ok(())
}

pub(crate) fn blame(file: &String, lines: &Option<String>) -> Result<()> {
    eprintln!(
        "⚠  'torii blame' is deprecated and will be removed in 0.8.\n   \
                     Use 'torii show {} --blame' instead.",
        file
    );
    let repo = GitRepo::open(".")?;
    repo.blame(file, lines.as_deref())?;
    Ok(())
}

pub(crate) fn cherry_pick(commit: &Option<String>, r#continue: &bool, abort: &bool) -> Result<()> {
    let repo = GitRepo::open(".")?;
    if *r#continue {
        repo.cherry_pick_continue()?;
    } else if *abort {
        repo.cherry_pick_abort()?;
    } else {
        let hash = commit
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Commit hash required: torii cherry-pick <hash>"))?;
        repo.cherry_pick(hash)?;
    }
    Ok(())
}

// Mirrors the clap variant fields 1:1 — a param struct would just duplicate the enum.
#[allow(clippy::too_many_arguments)]
pub(crate) fn branch(
    name: &Option<String>,
    create: &bool,
    orphan: &bool,
    delete: &Option<String>,
    force: &bool,
    delete_remote: &Option<String>,
    list: &bool,
    rename: &Option<String>,
    all: &bool,
) -> Result<()> {
    let repo = GitRepo::open(".")?;

    if *list || *all {
        let branches = repo.list_branches()?;
        println!("📋 Branches:");
        for branch in branches {
            println!("  • {}", branch);
        }
        if *all {
            let remote_branches = repo.list_remote_branches()?;
            println!("\n📡 Remote branches:");
            if remote_branches.is_empty() {
                println!("  (none — run 'torii sync --fetch' to update remote refs)");
            } else {
                for branch in remote_branches {
                    println!("  • {}", branch);
                }
            }
        }
    } else if let Some(branch_name) = delete_remote {
        let git_repo = git2::Repository::discover(".")?;
        let remotes = git_repo.remotes()?;
        let mut deleted = vec![];
        let mut errors = vec![];
        for remote_name in remotes.iter().flatten() {
            let result = std::process::Command::new("git")
                .args(["push", remote_name, "--delete", branch_name])
                .output();
            match result {
                Ok(o) if o.status.success() => deleted.push(remote_name.to_string()),
                Ok(o) => errors.push(format!(
                    "{}: {}",
                    remote_name,
                    String::from_utf8_lossy(&o.stderr).trim().to_string()
                )),
                Err(e) => errors.push(format!("{}: {}", remote_name, e)),
            }
        }
        if !deleted.is_empty() {
            println!("✅ Deleted '{}' on: {}", branch_name, deleted.join(", "));
        }
        if !errors.is_empty() {
            for e in &errors {
                eprintln!("⚠️  {}", e);
            }
        }
        if deleted.is_empty() {
            anyhow::bail!("Could not delete '{}' on any remote", branch_name);
        }
    } else if let Some(branch_name) = delete {
        if *force {
            let git_repo = git2::Repository::discover(".")?;
            let mut branch = git_repo.find_branch(branch_name, git2::BranchType::Local)?;
            branch.delete()?;
        } else {
            repo.delete_branch(branch_name)?;
        }
        println!("✅ Deleted branch: {}", branch_name);
    } else if let Some(new_name) = rename {
        let current = repo.get_current_branch()?;
        repo.rename_branch(&current, new_name)?;
        println!("✅ Renamed branch {} to {}", current, new_name);
    } else if let Some(branch_name) = name {
        if *orphan && !*create {
            anyhow::bail!("--orphan requires -c/--create");
        }
        if *create && *orphan {
            repo.create_orphan_branch(branch_name)?;
            println!(
                "✅ Created orphan branch: {} (no parents — first commit will be a new root)",
                branch_name
            );
        } else if *create {
            repo.create_branch(branch_name)?;
            repo.switch_branch(branch_name)?;
            println!("✅ Created and switched to branch: {}", branch_name);
        } else {
            repo.switch_branch(branch_name)?;
            println!("✅ Switched to branch: {}", branch_name);
        }
    } else {
        // Default: list branches
        let branches = repo.list_branches()?;
        println!("📋 Branches:");
        for branch in branches {
            println!("  • {}", branch);
        }
    }
    Ok(())
}

pub(crate) fn show(
    object: &Option<String>,
    blame: &bool,
    lines: &Option<String>,
    signature: &bool,
) -> Result<()> {
    let repo = GitRepo::open(".")?;
    if *signature {
        run_show_signature(&repo, object.as_deref())?;
    } else if *blame {
        let file = object
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("File path required for --blame"))?;
        repo.blame(file, lines.as_deref())?;
    } else {
        repo.show(object.as_deref())?;
    }
    Ok(())
}
