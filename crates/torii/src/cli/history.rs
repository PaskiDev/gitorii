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

    /// Rewrite commit **messages** across history, preserving author,
    /// committer, dates and content. The message-equivalent of `reauthor`
    /// (identity) and `rewrite` (dates).
    #[command(after_help = "Examples:
  torii history reword <hash> -m \"feat: clearer subject\"   Reword one commit
  torii history reword <hash> -F message.txt               Read message from a file
  torii history reword --map rewords.txt                   Batch: '<hash> <message>' per line
  torii history reword <hash> -m \"...\" --dry-run           Preview, no changes

Author, committer and all timestamps are preserved exactly. A safety snapshot is
taken by default (revert with 'torii snapshot restore <id>'). History is
rewritten in place — push with 'torii sync --push --force'.")]
    Reword {
        /// Commit to reword (any revision). Omit when using --map.
        #[arg(value_name = "COMMIT", required_unless_present = "map")]
        commit: Option<String>,

        /// New commit message (single commit).
        #[arg(short, long, conflicts_with_all = ["file", "map"])]
        message: Option<String>,

        /// Read the new message from a file (single commit; supports multi-line).
        #[arg(short = 'F', long, value_name = "FILE", conflicts_with = "map")]
        file: Option<PathBuf>,

        /// Batch map file: one '<hash> <new single-line message>' per line.
        #[arg(long, value_name = "FILE", conflicts_with = "commit")]
        map: Option<PathBuf>,

        /// Limit the walk to commits since this revision (exclusive).
        #[arg(long, value_name = "REV")]
        since: Option<String>,

        /// Preview the rewrite without touching the repo.
        #[arg(long)]
        dry_run: bool,

        /// Skip the safety snapshot taken before rewriting.
        #[arg(long)]
        no_snapshot: bool,

        /// Proceed even if the working tree has uncommitted changes.
        #[arg(long)]
        allow_dirty: bool,

        /// Force GPG signing of every rewritten commit, even if
        /// `git.sign_commits` is `false`. Requires `git.gpg_key`.
        #[arg(short = 'S', long)]
        sign: bool,
    },

    /// Replace text in file **contents** across all history (à la
    /// `git filter-repo --replace-text` / `filter-branch --tree-filter` sed).
    #[command(after_help = "Examples:
  torii history replace-text --literal \"hunter2\"                 Redact a literal (→ ***REMOVED***)
  torii history replace-text --literal \"old.com==>new.com\"       Literal replace
  torii history replace-text --regex \"AKIA[0-9A-Z]{16}==>***\"    Regex replace
  torii history replace-text --rules secrets.txt                 Rules file (filter-repo format)
  torii history replace-text --redact-secrets                    Redact lines the scanner flags
  torii history replace-text --literal X --paths src --dry-run   Scope + preview

Rules file: one `literal:OLD[==>NEW]` or `regex:PAT[==>REPL]` per line; missing
`==>` redacts to ***REMOVED***. History is rewritten in place — push --force.")]
    ReplaceText {
        /// Rules file (filter-repo expression format).
        #[arg(long, value_name = "FILE")]
        rules: Option<PathBuf>,
        /// Literal replacement (repeatable): "OLD==>NEW" or "OLD" (→ ***REMOVED***).
        #[arg(long, value_name = "RULE")]
        literal: Vec<String>,
        /// Regex replacement (repeatable): "PAT==>REPL" or "PAT" (→ ***REMOVED***).
        #[arg(long, value_name = "RULE")]
        regex: Vec<String>,
        /// Redact every line flagged by the built-in secret scanner.
        #[arg(long)]
        redact_secrets: bool,
        /// Limit to these paths (repeatable; prefix match).
        #[arg(long, value_name = "PATH")]
        paths: Vec<String>,
        #[arg(long, value_name = "REV")]
        since: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_snapshot: bool,
        #[arg(long)]
        allow_dirty: bool,
        #[arg(long)]
        prune_empty: bool,
    },

    /// Filter **paths** across history: keep/remove/rename paths or extract a
    /// subdirectory as the new root (`filter-repo --path`/`--subdirectory-filter`).
    #[command(after_help = "Examples:
  torii history filter-path --remove secrets/             Drop a path from all history
  torii history filter-path --keep src --keep Cargo.toml  Keep only these paths
  torii history filter-path --subdirectory crates/lib     Make a subdir the new root
  torii history filter-path --rename old/:new/            Rename a path prefix")]
    FilterPath {
        /// Keep only these paths (repeatable). If set, everything else is dropped.
        #[arg(long, value_name = "PATH")]
        keep: Vec<String>,
        /// Remove these paths (repeatable).
        #[arg(long, value_name = "PATH")]
        remove: Vec<String>,
        /// Extract this subdirectory as the new repository root.
        #[arg(long, value_name = "DIR")]
        subdirectory: Option<String>,
        /// Rename a path prefix, `OLD:NEW` (repeatable).
        #[arg(long, value_name = "OLD:NEW")]
        rename: Vec<String>,
        #[arg(long, value_name = "REV")]
        since: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_snapshot: bool,
        #[arg(long)]
        allow_dirty: bool,
        #[arg(long)]
        prune_empty: bool,
    },

    /// Set the author + committer date of specific commits (per-commit, the
    /// counterpart to `history rewrite`'s range interpolation).
    #[command(after_help = "Examples:
  torii history redate <hash> --date \"2026-06-01 10:00:00 +0000\"
  torii history redate --map dates.txt        # '<hash> <date>' per line")]
    Redate {
        /// Commit to redate (omit with --map).
        #[arg(value_name = "COMMIT", required_unless_present = "map")]
        commit: Option<String>,
        /// New date in any git format.
        #[arg(long, value_name = "WHEN", conflicts_with = "map")]
        date: Option<String>,
        /// Batch map file: one `<hash> <date>` per line.
        #[arg(long, value_name = "FILE", conflicts_with = "commit")]
        map: Option<PathBuf>,
        #[arg(long, value_name = "REV")]
        since: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_snapshot: bool,
        #[arg(long)]
        allow_dirty: bool,
    },

    /// Run a shell command against a checkout of **every** commit's tree — the
    /// classic `filter-branch --tree-filter`. Slow and powerful; prefer the
    /// declarative `replace-text` / `filter-path` when they suffice.
    #[command(after_help = "Examples:
  torii history exec-filter 'rm -f secret.txt'            Delete a file everywhere
  torii history exec-filter 'sed -i s/foo/bar/g *.md'     Arbitrary transform

The command runs with the commit's tree materialized as the working directory.
Limitations: submodules aren't materialized; symlinks/exec-bits are Unix-only.")]
    ExecFilter {
        /// Shell command to run inside each commit's materialized tree.
        #[arg(value_name = "COMMAND")]
        command: String,
        #[arg(long, value_name = "REV")]
        since: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_snapshot: bool,
        #[arg(long)]
        allow_dirty: bool,
        #[arg(long)]
        prune_empty: bool,
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
        HistoryCommands::Reword {
            commit,
            message,
            file,
            map,
            since,
            dry_run,
            no_snapshot,
            allow_dirty,
            sign,
        } => {
            use crate::history_reword;
            // `--sign` forces signing through the same env-var override used by
            // `torii save -S`; commit_inner_split reads it for each recreated commit.
            let _sign_guard = SignOverrideGuard::new(if *sign { Some(true) } else { None });

            let entries: Vec<(String, String)> = if let Some(map_path) = map {
                history_reword::load_reword_map(map_path)?
            } else {
                let hash = commit
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("provide a commit (or --map)"))?;
                let msg = if let Some(m) = message {
                    m.clone()
                } else if let Some(f) = file {
                    std::fs::read_to_string(f)
                        .map_err(|e| anyhow::anyhow!("read {}: {}", f.display(), e))?
                } else {
                    return Err(anyhow::anyhow!(
                        "provide a new message with -m/--message or -F/--file"
                    ));
                };
                vec![(hash, msg)]
            };

            let opts = history_reword::RewordOptions {
                since: since.clone(),
                dry_run: *dry_run,
                no_snapshot: *no_snapshot,
                allow_dirty: *allow_dirty,
            };
            let stats = history_reword::reword(std::path::Path::new("."), &entries, &opts)?;
            history_reword::print_summary(&stats, *dry_run);
        }
        HistoryCommands::ReplaceText {
            rules,
            literal,
            regex,
            redact_secrets,
            paths,
            since,
            dry_run,
            no_snapshot,
            allow_dirty,
            prune_empty,
        } => {
            use crate::history_filter;
            let mut inline: Vec<String> = Vec::new();
            inline.extend(literal.iter().map(|s| format!("literal:{s}")));
            inline.extend(regex.iter().map(|s| format!("regex:{s}")));
            let path_filter = (!paths.is_empty()).then(|| paths.clone());
            let mut f = history_filter::ReplaceText::new(
                rules.as_deref(),
                &inline,
                *redact_secrets,
                path_filter,
            )?;
            let opts = history_filter::FilterOptions {
                since: since.clone(),
                dry_run: *dry_run,
                no_snapshot: *no_snapshot,
                allow_dirty: *allow_dirty,
                prune_empty: *prune_empty,
            };
            let stats = history_filter::run_filter(std::path::Path::new("."), &opts, &mut f)?;
            history_filter::print_summary(&stats, "replace-text", *dry_run);
        }
        HistoryCommands::FilterPath {
            keep,
            remove,
            subdirectory,
            rename,
            since,
            dry_run,
            no_snapshot,
            allow_dirty,
            prune_empty,
        } => {
            use crate::history_filter;
            let mut rename_pairs = Vec::new();
            for r in rename {
                match r.split_once(':') {
                    Some((a, b)) => rename_pairs.push((a.to_string(), b.to_string())),
                    None => return Err(anyhow::anyhow!("--rename expects OLD:NEW, got {:?}", r)),
                }
            }
            let mut f = history_filter::FilterPath::new(
                keep.clone(),
                remove.clone(),
                subdirectory.clone(),
                rename_pairs,
            )?;
            let opts = history_filter::FilterOptions {
                since: since.clone(),
                dry_run: *dry_run,
                no_snapshot: *no_snapshot,
                allow_dirty: *allow_dirty,
                prune_empty: *prune_empty,
            };
            let stats = history_filter::run_filter(std::path::Path::new("."), &opts, &mut f)?;
            history_filter::print_summary(&stats, "filter-path", *dry_run);
        }
        HistoryCommands::Redate {
            commit,
            date,
            map,
            since,
            dry_run,
            no_snapshot,
            allow_dirty,
        } => {
            use crate::history_filter;
            let entries: Vec<(String, String)> = if let Some(map_path) = map {
                // Same '<hash> <value>' format as the reword map; here value is a date.
                crate::history_reword::load_reword_map(map_path)?
            } else {
                let hash = commit
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("provide a commit (or --map)"))?;
                let when = date
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("provide a date with --date"))?;
                vec![(hash, when)]
            };
            let mut f = history_filter::Redate::new(&entries, std::path::Path::new("."))?;
            let opts = history_filter::FilterOptions {
                since: since.clone(),
                dry_run: *dry_run,
                no_snapshot: *no_snapshot,
                allow_dirty: *allow_dirty,
                prune_empty: false,
            };
            let stats = history_filter::run_filter(std::path::Path::new("."), &opts, &mut f)?;
            history_filter::print_summary(&stats, "redate", *dry_run);
        }
        HistoryCommands::ExecFilter {
            command,
            since,
            dry_run,
            no_snapshot,
            allow_dirty,
            prune_empty,
        } => {
            use crate::history_filter;
            let mut f = history_filter::ExecFilter::new(command.clone());
            let opts = history_filter::FilterOptions {
                since: since.clone(),
                dry_run: *dry_run,
                no_snapshot: *no_snapshot,
                allow_dirty: *allow_dirty,
                prune_empty: *prune_empty,
            };
            let stats = history_filter::run_filter(std::path::Path::new("."), &opts, &mut f)?;
            history_filter::print_summary(&stats, "exec-filter", *dry_run);
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
