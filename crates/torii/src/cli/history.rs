//! `torii history` — history rewriting and inspection.

use super::fsck::run_fsck;
use super::sign::SignOverrideGuard;
use crate::core::GitRepo;
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum HistoryCommands {
    /// Rewrite commit history dates
    Rewrite {
        /// Start date (YYYY-MM-DD HH:MM)
        start: String,

        /// End date (YYYY-MM-DD HH:MM)
        end: String,
    },

    /// Compact the repository — repack objects, expire reflog,
    /// drop unreachable refs. Same operation as `git gc`.
    ///
    /// Renamed from `clean` → `gc` → `compact` over 0.7.0 as we
    /// converged on plain English. `gc` still works as an alias for
    /// users coming from git; old `clean` (top-level, was GC) is a
    /// deprecated alias and prints a warning.
    #[command(alias = "gc")]
    Compact,

    /// **Deprecated** — alias for `torii history gc`. Will be removed in 0.8.
    /// `torii clean` (top-level) is now the cleanup-untracked-files command.
    #[command(hide = true)]
    Clean,

    /// Remove a file from the entire git history
    RemoveFile {
        /// File path to remove from all commits
        file: String,
    },

    /// Rebase current branch onto a target
    Rebase {
        /// Target branch or commit to rebase onto
        target: Option<String>,

        /// Interactive rebase
        #[arg(short, long)]
        interactive: bool,

        /// Path to a pre-written rebase todo file (skips editor)
        #[arg(long, value_name = "FILE")]
        todo_file: Option<PathBuf>,

        /// Rebase from the root commit (no target needed; useful to squash initial commits)
        #[arg(long)]
        root: bool,

        /// Continue an in-progress rebase
        #[arg(long)]
        r#continue: bool,

        /// Abort the current rebase
        #[arg(long)]
        abort: bool,

        /// Skip the current patch
        #[arg(long)]
        skip: bool,
    },

    /// Find unreachable objects (orphaned commits/blobs/trees) — recovery aid
    /// after a destructive operation like reset --hard, force-push, or rebase.
    /// By default lists the unreachable objects with a one-line summary.
    /// Use --show <oid> to inspect content; --restore to write a blob to disk.
    #[command(
        alias = "fsck",
        after_help = "Examples:
  torii history orphans                              List unreachable objects
  torii history orphans --show abc1234               Print object content (commit/blob)
  torii history orphans --restore abc1234 --to f.txt Recover a blob to disk

`torii history fsck` works too — alias kept for users coming from git."
    )]
    Orphans {
        /// Show an object's content (commit message + tree, or blob bytes).
        #[arg(long, value_name = "OID")]
        show: Option<String>,

        /// Restore a blob to disk (use with --to).
        #[arg(long, value_name = "OID")]
        restore: Option<String>,

        /// Destination path for --restore.
        #[arg(long, value_name = "PATH")]
        to: Option<PathBuf>,
    },

    /// Rewrite author (and optionally committer) identity across history.
    ///
    /// Match a single `--old` identity and replace with `--new`. Use this for
    /// one-off renames; for batch rewrites driven by a file see
    /// `torii history mailmap apply`.
    #[command(after_help = "Examples:
  torii history reauthor --old \"outsider <x@y.com>\" --new \"Pasqual <paski@paski.dev>\"
  torii history reauthor --old outsider --new \"Pasqual <paski@paski.dev>\"           # match by name only
  torii history reauthor --old x@y.com --new \"Pasqual <paski@paski.dev>\"            # match by email only
  torii history reauthor --old ... --new ... --committer        # also rewrite committer
  torii history reauthor --old ... --new ... --since v0.6.0     # only commits since v0.6.0
  torii history reauthor --old ... --new ... --dry-run          # preview, no changes
  torii history reauthor --old ... --new ... --no-snapshot      # skip safety snapshot
  torii history reauthor --old ... --new ... --allow-dirty      # allow uncommitted changes

History is rewritten in-place. Annotated tags get a new tagger that matches
the rewrite. A safety snapshot is taken by default (revert with
'torii snapshot restore <id>'). If commits are signed, signatures invalidate
— re-sign manually after the rewrite or document the rotation.")]
    Reauthor {
        /// Identity to match. Accepts "Name <email>", a bare name, or a bare email.
        #[arg(long)]
        old: String,

        /// Replacement identity. Must be in "Name <email>" form.
        #[arg(long)]
        new: String,

        /// Limit rewrite to commits since this revision (exclusive).
        #[arg(long, value_name = "REV")]
        since: Option<String>,

        /// Preview the rewrite without touching the repo.
        #[arg(long)]
        dry_run: bool,

        /// Skip the safety snapshot taken before rewriting.
        #[arg(long)]
        no_snapshot: bool,

        /// Also rewrite the committer (default: only author).
        #[arg(long)]
        committer: bool,

        /// Proceed even if the working tree has uncommitted changes.
        #[arg(long)]
        allow_dirty: bool,

        /// Force GPG signing of every rewritten commit, even if
        /// `git.sign_commits` is `false`. Lets you re-sign a range
        /// of historical commits as part of a single reauthor pass,
        /// instead of running reauthor + `torii sign` back-to-back.
        /// Requires `git.gpg_key` to be set.
        #[arg(long)]
        sign: bool,
    },

    /// Apply a `.mailmap` file (standard git format) across history.
    ///
    /// See <https://git-scm.com/docs/gitmailmap> for the format. Use this for
    /// batch identity reconciliation; for a single rename use
    /// `torii history reauthor`.
    #[command(after_help = "Examples:
  torii history mailmap apply                          Apply repo .mailmap
  torii history mailmap apply --file other.mailmap     Apply a different file
  torii history mailmap apply --since v0.6.0           Limit to a range
  torii history mailmap apply --dry-run                Preview, no changes
  torii history mailmap apply --no-snapshot            Skip safety snapshot

Mailmap supports four line forms:
  Proper Name <commit@email>
  <proper@email> <commit@email>
  Proper Name <proper@email> <commit@email>
  Proper Name <proper@email> Commit Name <commit@email>")]
    Mailmap {
        #[command(subcommand)]
        action: MailmapCommands,
    },
}

#[derive(Subcommand)]
pub(crate) enum MailmapCommands {
    /// Apply rewrites from a `.mailmap` file to every reachable commit.
    Apply {
        /// Mailmap file path (default: `.mailmap` at repo root).
        #[arg(long, value_name = "FILE")]
        file: Option<PathBuf>,

        /// Limit rewrite to commits since this revision (exclusive).
        #[arg(long, value_name = "REV")]
        since: Option<String>,

        /// Preview the rewrite without touching the repo.
        #[arg(long)]
        dry_run: bool,

        /// Skip the safety snapshot taken before rewriting.
        #[arg(long)]
        no_snapshot: bool,

        /// Also rewrite the committer (default: only author).
        #[arg(long)]
        committer: bool,

        /// Proceed even if the working tree has uncommitted changes.
        #[arg(long)]
        allow_dirty: bool,
    },
}

pub(crate) fn run(action: &HistoryCommands) -> Result<()> {
    let repo = GitRepo::open(".")?;
    match action {
        HistoryCommands::Rewrite { start, end } => {
            repo.rewrite_history(start, end)?;
            println!("✅ History rewritten successfully");
        }
        HistoryCommands::Compact => {
            repo.clean_history()?;
            println!("✅ Repository compacted (objects repacked, reflog expired)");
        }
        HistoryCommands::Clean => {
            eprintln!(
                            "⚠  'torii history clean' is deprecated and will be removed in 0.8.\n   \
                             Use 'torii history compact' (or 'gc' alias) instead.\n   \
                             Heads up: 'torii clean' (top-level) now exists as untracked-file cleanup."
                        );
            repo.clean_history()?;
            println!("✅ Repository compacted");
        }
        HistoryCommands::RemoveFile { file } => {
            repo.remove_file_from_history(file)?;
        }
        HistoryCommands::Rebase {
            target,
            interactive,
            todo_file,
            root,
            r#continue,
            abort,
            skip,
        } => {
            if *r#continue {
                repo.rebase_continue()?;
            } else if *abort {
                repo.rebase_abort()?;
            } else if *skip {
                repo.rebase_skip()?;
            } else if *root {
                if let Some(todo) = todo_file {
                    repo.rebase_root_with_todo(todo)?;
                } else {
                    repo.rebase_root_interactive()?;
                }
            } else if let Some(todo) = todo_file {
                let base = target.as_deref().ok_or_else(|| anyhow::anyhow!("Target required: torii history rebase <base> --todo-file plan.txt (or use --root)"))?;
                repo.rebase_with_todo(base, todo)?;
            } else if *interactive {
                let base = target.as_deref().ok_or_else(|| anyhow::anyhow!("Target required: torii history rebase HEAD~3 --interactive (or use --root)"))?;
                repo.rebase_interactive(base)?;
            } else if let Some(base) = target {
                repo.rebase_branch(base)?;
                println!("✅ Rebased onto: {}", base);
            } else {
                anyhow::bail!("Specify a target or use --root / --interactive / --todo-file / --continue / --abort / --skip");
            }
        }
        HistoryCommands::Orphans { show, restore, to } => {
            run_fsck(show.as_deref(), restore.as_deref(), to.as_deref())?;
        }
        HistoryCommands::Reauthor {
            old,
            new,
            since,
            dry_run,
            no_snapshot,
            committer,
            allow_dirty,
            sign,
        } => {
            use crate::history_reauthor;
            let old_m = history_reauthor::OldMatcher::parse_loose(old)?;
            let new_id = history_reauthor::Identity::parse_full(new)?;
            let opts = history_reauthor::Options {
                since: since.clone(),
                dry_run: *dry_run,
                no_snapshot: *no_snapshot,
                committer: *committer,
                allow_dirty: *allow_dirty,
            };
            // 0.7.36 — `--sign` forces signing through
            // the same env-var override used by
            // `torii save -S`. commit_inner_split (the
            // path reauthor takes) reads it and signs
            // every rewritten commit.
            let _sign_guard = SignOverrideGuard::new(if *sign { Some(true) } else { None });
            let stats =
                history_reauthor::reauthor(std::path::Path::new("."), old_m, new_id, &opts)?;
            history_reauthor::print_summary(&stats, *dry_run);
        }
        HistoryCommands::Mailmap { action } => match action {
            MailmapCommands::Apply {
                file,
                since,
                dry_run,
                no_snapshot,
                committer,
                allow_dirty,
            } => {
                use crate::history_reauthor;
                let mailmap_path = file.clone().unwrap_or_else(|| PathBuf::from(".mailmap"));
                if !mailmap_path.exists() {
                    anyhow::bail!("mailmap file not found: {}", mailmap_path.display());
                }
                let opts = history_reauthor::Options {
                    since: since.clone(),
                    dry_run: *dry_run,
                    no_snapshot: *no_snapshot,
                    committer: *committer,
                    allow_dirty: *allow_dirty,
                };
                let stats = history_reauthor::mailmap_apply(
                    std::path::Path::new("."),
                    &mailmap_path,
                    &opts,
                )?;
                history_reauthor::print_summary(&stats, *dry_run);
            }
        },
    }
    Ok(())
}
