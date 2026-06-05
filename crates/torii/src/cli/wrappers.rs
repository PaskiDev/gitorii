//! Thin dispatch into `crate::cmd` wrappers: worktree, submodule,
//! subtree, bisect, notes, patch, describe, archive, remove, rename,
//! grep, clean.

use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum BisectCommands {
    /// Enter bisect mode. Optionally pass `<bad> [<good>...]` to seed it.
    Start {
        /// Known-bad commit (defaults to HEAD when seeding inline later).
        bad: Option<String>,
        /// One or more known-good commits.
        good: Vec<String>,
    },
    /// Mark the given (or current) commit as bad.
    Bad { commit: Option<String> },
    /// Mark the given (or current) commit as good.
    Good { commit: Option<String> },
    /// Skip the current commit (unbuildable/untestable).
    Skip { commit: Option<String> },
    /// Exit bisect mode and restore HEAD.
    Reset,
    /// Print the bisect log so far.
    Log,
    /// Run `<cmd>` for every candidate; exit 0 = good, non-zero = bad, 125 = skip.
    Run {
        #[arg(trailing_var_arg = true, required = true)]
        cmd: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum NotesCommands {
    /// List commits that have notes attached.
    List,
    /// Add a note to a commit. Opens $EDITOR if -m not given.
    Add {
        commit: String,
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// Overwrite an existing note.
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Append to the commit's existing note.
    Append {
        commit: String,
        #[arg(short = 'm', long)]
        message: String,
    },
    /// Print the note attached to a commit.
    Show { commit: String },
    /// Open the note in $EDITOR for changes.
    Edit { commit: String },
    /// Copy notes from one commit to another.
    Copy {
        from: String,
        to: String,
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Remove a commit's note.
    Remove { commit: String },
}

#[derive(Subcommand)]
pub(crate) enum PatchCommands {
    /// Export a commit range as one `.patch` per commit.
    Export {
        /// Revision range, e.g. `v0.6.8..HEAD` or `HEAD~3..`.
        range: String,
        /// Output directory (default: cwd).
        #[arg(short = 'o', long)]
        output_dir: Option<PathBuf>,
        /// Write patches to stdout instead of files.
        #[arg(long)]
        stdout: bool,
        /// Include a cover letter as `0000-cover-letter.patch`.
        #[arg(long)]
        cover_letter: bool,
    },
    /// Apply one or more patch files as new commits.
    Apply {
        /// Patch files (use `--continue`/`--abort`/`--skip` for ongoing ops).
        files: Vec<PathBuf>,
        /// Fall back to 3-way merge on conflicts.
        #[arg(long)]
        three_way: bool,
        /// Resume after manual conflict resolution.
        #[arg(long = "continue")]
        continue_: bool,
        /// Drop the current patch and move on.
        #[arg(long)]
        skip: bool,
        /// Bail out of an in-progress apply session.
        #[arg(long)]
        abort: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum SubmoduleCommands {
    /// Register and clone a new submodule.
    Add {
        /// Source URL of the submodule (git@host:owner/repo.git, https://…, etc.).
        url: String,
        /// Where in this repo to place it (e.g. vendor/lib).
        path: PathBuf,
        /// Track a specific branch (writes submodule.<n>.branch in .gitmodules).
        #[arg(long)]
        branch: Option<String>,
        /// Override the submodule name (defaults to the path).
        #[arg(long)]
        name: Option<String>,
        /// After cloning the top-level submodule, recursively init+update
        /// any nested submodules it contains.
        #[arg(long)]
        recursive: bool,
    },

    /// List submodules with HEAD, working-tree id and state.
    Status,

    /// Copy URLs from `.gitmodules` into `.git/config`.
    Init {
        /// Overwrite existing entries in `.git/config`.
        #[arg(long)]
        force: bool,
    },

    /// Fetch and checkout the commit each submodule is pinned at.
    Update {
        /// Also run `init` first for submodules that aren't initialised.
        #[arg(long)]
        init: bool,
        /// Recurse into nested submodules after each top-level update.
        #[arg(long)]
        recursive: bool,
    },

    /// Re-copy URLs from `.gitmodules` into `.git/config`.
    Sync,

    /// Run a shell command in each submodule's working directory.
    Foreach {
        /// Command to run via $SHELL -c. Stops at the first non-zero exit.
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },

    /// Deregister a submodule cleanly (.gitmodules, .git/config, .git/modules, working tree).
    Remove {
        /// Path of the submodule to remove (must match `path` in .gitmodules).
        path: PathBuf,
    },
}

#[derive(Subcommand)]
pub(crate) enum SubtreeCommands {
    /// Initial import of `<url>:<ref>` at `--prefix=<dir>`.
    Add {
        /// Subdirectory inside the super-repo (e.g. vendor/lib).
        #[arg(long)]
        prefix: String,
        /// Source URL or local path.
        url: String,
        /// Ref (branch, tag, commit) on the source side.
        #[arg(value_name = "REF")]
        refname: String,
        /// Flatten upstream history into one merge commit.
        #[arg(long)]
        squash: bool,
    },

    /// Fetch and merge upstream updates into the subtree.
    Pull {
        #[arg(long)]
        prefix: String,
        url: String,
        #[arg(value_name = "REF")]
        refname: String,
        #[arg(long)]
        squash: bool,
    },

    /// Extract the subtree and push it back to its source.
    Push {
        #[arg(long)]
        prefix: String,
        url: String,
        #[arg(value_name = "REF")]
        refname: String,
    },

    /// Extract the subtree's history into a new branch without pushing.
    Split {
        #[arg(long)]
        prefix: String,
        /// Create a local branch at the split commit.
        #[arg(short = 'b', long)]
        branch: Option<String>,
        /// Annotate cherry-picked commits with this prefix.
        #[arg(long)]
        annotate: Option<String>,
    },

    /// Finish a manual conflict resolution after `pull`.
    Merge {
        #[arg(long)]
        prefix: String,
        #[arg(value_name = "REF")]
        refname: String,
        #[arg(long)]
        squash: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum WorktreeCommands {
    /// Create a new worktree.
    ///
    /// One of `-b <new-branch>` or a positional `<existing-branch>` is
    /// required. If `<path>` is omitted, it's derived from
    /// `worktree.base_dir` + repo name + branch name.
    Add {
        /// Path for the new worktree. Defaults to <worktree.base_dir>/<repo>-<branch>.
        path: Option<PathBuf>,

        /// Create a new branch with this name (off the current HEAD).
        #[arg(short = 'b', long = "branch", value_name = "NEW_BRANCH")]
        new_branch: Option<String>,

        /// Check out this existing local branch in the worktree.
        #[arg(value_name = "EXISTING_BRANCH")]
        existing_branch: Option<String>,
    },

    /// List all worktrees with branch and clean/dirty status.
    List,

    /// Remove a worktree and its directory (always takes a snapshot first).
    Remove {
        /// Path to the worktree to remove.
        path: PathBuf,

        /// Remove even if the working tree has uncommitted changes.
        #[arg(long)]
        force: bool,

        /// Skip the safety snapshot taken before removing.
        #[arg(long)]
        no_snapshot: bool,
    },

    /// Clean up metadata of worktrees whose directory has been deleted.
    Prune,

    /// Launch $SHELL inside a worktree directory; returns when the shell exits.
    Open {
        /// Path to the worktree to open.
        path: PathBuf,
    },

    /// Lock a worktree against `prune` (and accidental cleanup tools).
    Lock {
        /// Path to the worktree to lock.
        path: PathBuf,
        /// Optional reason saved alongside the lock; surfaces in `list`.
        #[arg(short = 'r', long)]
        reason: Option<String>,
    },

    /// Release a previously locked worktree.
    Unlock {
        /// Path to the worktree to unlock.
        path: PathBuf,
    },

    /// Move a worktree directory and patch its link files.
    Move {
        /// Current path of the worktree.
        old: PathBuf,
        /// Target path.
        new: PathBuf,
    },

    /// Re-validate every linked worktree's link files and report broken ones.
    Repair,
}

pub(crate) fn worktree(action: &Option<WorktreeCommands>) -> Result<()> {
    use crate::worktree;
    let repo_path = std::path::Path::new(".");
    // Default to `list` when no subcommand is given — git/cargo/npm convention.
    match action.as_ref() {
        None | Some(WorktreeCommands::List) => {
            worktree::list(repo_path)?;
        }
        Some(WorktreeCommands::Add {
            path,
            new_branch,
            existing_branch,
        }) => {
            let spec = match (new_branch, existing_branch) {
                (Some(_), Some(_)) => anyhow::bail!(
                    "Pass either -b <new-branch> OR a positional <existing-branch>, not both."
                ),
                (Some(name), None) => worktree::BranchSpec::New(name.clone()),
                (None, Some(name)) => worktree::BranchSpec::Existing(name.clone()),
                (None, None) => anyhow::bail!(
                    "Specify the branch: either -b <new-branch> or a positional <existing-branch>."
                ),
            };
            let opts = worktree::AddOpts {
                explicit_path: path.clone(),
            };
            worktree::add(repo_path, spec, &opts)?;
        }
        Some(WorktreeCommands::Remove {
            path,
            force,
            no_snapshot,
        }) => {
            let opts = worktree::RemoveOpts {
                force: *force,
                no_snapshot: *no_snapshot,
            };
            worktree::remove(repo_path, path, &opts)?;
        }
        Some(WorktreeCommands::Prune) => {
            worktree::prune(repo_path)?;
        }
        Some(WorktreeCommands::Open { path }) => {
            worktree::open(repo_path, path)?;
        }
        Some(WorktreeCommands::Lock { path, reason }) => {
            worktree::lock(repo_path, path, reason.as_deref())?;
        }
        Some(WorktreeCommands::Unlock { path }) => {
            worktree::unlock(repo_path, path)?;
        }
        Some(WorktreeCommands::Move { old, new }) => {
            worktree::move_wt(repo_path, old, new)?;
        }
        Some(WorktreeCommands::Repair) => {
            worktree::repair(repo_path)?;
        }
    }
    Ok(())
}

pub(crate) fn submodule(action: &Option<SubmoduleCommands>) -> Result<()> {
    use crate::submodule;
    let repo_path = std::path::Path::new(".");
    match action.as_ref() {
        None | Some(SubmoduleCommands::Status) => {
            submodule::status(repo_path)?;
        }
        Some(SubmoduleCommands::Add {
            url,
            path,
            branch,
            name,
            recursive,
        }) => {
            let opts = submodule::AddOpts {
                branch: branch.clone(),
                name: name.clone(),
                recursive: *recursive,
            };
            submodule::add(repo_path, url, path, &opts)?;
        }
        Some(SubmoduleCommands::Init { force }) => {
            submodule::init(repo_path, *force)?;
        }
        Some(SubmoduleCommands::Update { init, recursive }) => {
            let opts = submodule::UpdateOpts {
                init: *init,
                recursive: *recursive,
            };
            submodule::update(repo_path, &opts)?;
        }
        Some(SubmoduleCommands::Sync) => {
            submodule::sync(repo_path)?;
        }
        Some(SubmoduleCommands::Foreach { cmd }) => {
            if cmd.is_empty() {
                anyhow::bail!(
                    "foreach needs a command, e.g. torii submodule foreach 'cargo build'"
                );
            }
            let joined = cmd.join(" ");
            submodule::foreach(repo_path, &joined)?;
        }
        Some(SubmoduleCommands::Remove { path }) => {
            submodule::remove(repo_path, path)?;
        }
    }
    Ok(())
}

pub(crate) fn subtree(action: &SubtreeCommands) -> Result<()> {
    use crate::subtree;
    let repo_path = std::path::Path::new(".");
    match action {
        SubtreeCommands::Add {
            prefix,
            url,
            refname,
            squash,
        } => {
            subtree::add(
                repo_path,
                prefix,
                url,
                refname,
                &subtree::CommonOpts { squash: *squash },
            )?;
        }
        SubtreeCommands::Pull {
            prefix,
            url,
            refname,
            squash,
        } => {
            subtree::pull(
                repo_path,
                prefix,
                url,
                refname,
                &subtree::CommonOpts { squash: *squash },
            )?;
        }
        SubtreeCommands::Push {
            prefix,
            url,
            refname,
        } => {
            subtree::push(repo_path, prefix, url, refname)?;
        }
        SubtreeCommands::Split {
            prefix,
            branch,
            annotate,
        } => {
            subtree::split(repo_path, prefix, branch.as_deref(), annotate.as_deref())?;
        }
        SubtreeCommands::Merge {
            prefix,
            refname,
            squash,
        } => {
            subtree::merge(
                repo_path,
                prefix,
                refname,
                &subtree::CommonOpts { squash: *squash },
            )?;
        }
    }
    Ok(())
}

pub(crate) fn bisect(action: &BisectCommands) -> Result<()> {
    let p = std::path::Path::new(".");
    match action {
        BisectCommands::Start { bad, good } => crate::bisect::start(p, bad.as_deref(), good)?,
        BisectCommands::Bad { commit } => crate::bisect::bad(p, commit.as_deref())?,
        BisectCommands::Good { commit } => crate::bisect::good(p, commit.as_deref())?,
        BisectCommands::Skip { commit } => crate::bisect::skip(p, commit.as_deref())?,
        BisectCommands::Reset => crate::bisect::reset(p)?,
        BisectCommands::Log => crate::bisect::log(p)?,
        BisectCommands::Run { cmd } => crate::bisect::run(p, cmd)?,
    }
    Ok(())
}

pub(crate) fn describe(tags: &bool, long: &bool, dirty: &bool, candidates: &u32) -> Result<()> {
    let opts = crate::describe::Opts {
        tags: *tags,
        long: *long,
        dirty: *dirty,
        candidates: *candidates,
    };
    crate::describe::describe(std::path::Path::new("."), &opts)?;
    Ok(())
}

pub(crate) fn archive(
    revision: &str,
    output: &Option<String>,
    format: &Option<String>,
    prefix: &Option<String>,
) -> Result<()> {
    let opts = crate::archive::Opts {
        output: output.clone(),
        format: format.clone(),
        prefix: prefix.clone(),
    };
    crate::archive::archive(std::path::Path::new("."), revision, &opts)?;
    Ok(())
}

pub(crate) fn remove(
    paths: &[PathBuf],
    cached: &bool,
    recursive: &bool,
    force: &bool,
) -> Result<()> {
    let opts = crate::fileops::RmOpts {
        cached: *cached,
        recursive: *recursive,
        force: *force,
    };
    crate::fileops::rm(std::path::Path::new("."), paths, &opts)?;
    Ok(())
}

pub(crate) fn rename(from: &std::path::Path, to: &std::path::Path, force: &bool) -> Result<()> {
    let opts = crate::fileops::MvOpts { force: *force };
    crate::fileops::mv(std::path::Path::new("."), from, to, &opts)?;
    Ok(())
}

pub(crate) fn grep(
    pattern: &str,
    paths: &[String],
    ignore_case: &bool,
    word_regexp: &bool,
    files_with_matches: &bool,
    no_line_number: &bool,
) -> Result<()> {
    let opts = crate::grep::Opts {
        ignore_case: *ignore_case,
        word_regexp: *word_regexp,
        files_with_matches: *files_with_matches,
        no_line_number: *no_line_number,
        extra: Vec::new(),
    };
    crate::grep::grep(std::path::Path::new("."), pattern, paths, &opts)?;
    Ok(())
}

pub(crate) fn notes(action: &Option<NotesCommands>) -> Result<()> {
    let p = std::path::Path::new(".");
    match action.as_ref() {
        None | Some(NotesCommands::List) => crate::notes::list(p)?,
        Some(NotesCommands::Add {
            commit,
            message,
            force,
        }) => {
            crate::notes::add(p, commit, message.as_deref(), *force)?;
        }
        Some(NotesCommands::Append { commit, message }) => {
            crate::notes::append(p, commit, message)?;
        }
        Some(NotesCommands::Show { commit }) => crate::notes::show(p, commit)?,
        Some(NotesCommands::Edit { commit }) => crate::notes::edit(p, commit)?,
        Some(NotesCommands::Copy { from, to, force }) => {
            crate::notes::copy(p, from, to, *force)?;
        }
        Some(NotesCommands::Remove { commit }) => crate::notes::remove(p, commit)?,
    }
    Ok(())
}

pub(crate) fn patch(action: &PatchCommands) -> Result<()> {
    let p = std::path::Path::new(".");
    match action {
        PatchCommands::Export {
            range,
            output_dir,
            stdout,
            cover_letter,
        } => {
            let opts = crate::patch::ExportOpts {
                output_dir: output_dir.clone(),
                stdout: *stdout,
                cover_letter: *cover_letter,
            };
            crate::patch::export(p, range, &opts)?;
        }
        PatchCommands::Apply {
            files,
            three_way,
            continue_,
            skip,
            abort,
        } => {
            let opts = crate::patch::ApplyOpts {
                three_way: *three_way,
                continue_: *continue_,
                skip: *skip,
                abort: *abort,
            };
            crate::patch::apply(p, files, &opts)?;
        }
    }
    Ok(())
}

pub(crate) fn clean(
    force: &bool,
    dirs: &bool,
    include_ignored: &bool,
    only_ignored: &bool,
) -> Result<()> {
    let opts = crate::clean::Opts {
        force: *force,
        dirs: *dirs,
        include_ignored: *include_ignored,
        only_ignored: *only_ignored,
    };
    crate::clean::clean(std::path::Path::new("."), &opts)?;
    Ok(())
}
