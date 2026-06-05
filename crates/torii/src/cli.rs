//! CLI surface: clap definitions (`Cli`, `Commands`) and a thin
//! `execute()` dispatcher. Command logic lives in the sibling
//! submodules (`repo`, `tag`, `pr`, …) — one per command domain.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod auth;
mod clone;
mod config;
mod fsck;
mod history;
mod ignore;
mod issue;
mod mirror;
mod package;
mod pipeline;
mod platforms;
mod pr;
mod publish;
mod release;
mod remote;
mod repo;
mod runner;
mod scan;
mod sign;
mod snapshot;
mod tag;
mod tui;
mod workspace;
mod wrappers;

use auth::AuthCommands;
use config::ConfigCommands;
use history::HistoryCommands;
use ignore::IgnoreCommands;
use issue::IssueCommands;
use mirror::MirrorCommands;
use package::PackageCommands;
use pipeline::{JobCommands, PipelineCommands};
use platforms::PlatformsCommands;
use pr::PrCommands;
use release::ReleaseCommands;
use remote::RemoteCommands;
use runner::RunnerCommands;
use snapshot::SnapshotCommands;
use tag::TagCommands;
use workspace::WorkspaceCommands;
use wrappers::{
    BisectCommands, NotesCommands, PatchCommands, SubmoduleCommands, SubtreeCommands,
    WorktreeCommands,
};

#[derive(Parser)]
#[command(name = "torii")]
#[command(version, about = "A modern git client with simplified commands")]
#[command(after_help = "Examples — daily flow:
  torii status                          Show current state
  torii save -am \"feat: add login\"      Stage all and commit
  torii sync                            Pull and push
  torii sync main                       Integrate main into current branch
  torii diff --staged                   Review what will be committed

Branch & history:
  torii branch feature/auth -c          Create and switch to branch
  torii log --oneline --graph           Compact history graph
  torii history rebase main             Rebase current branch onto main
  torii history scan                    Scan staged files for secrets

Repos, remotes & identity:
  torii init                            Initialize a new repo
  torii clone github user/repo          Clone from GitHub
  torii mirror sync                     Push to all configured mirrors
  torii config set user.name \"Alice\"    Set git identity (name)
  torii auth login github               Authenticate with GitHub

Release & collaboration:
  torii tag create v1.0.0 -m \"Release\"  Create annotated tag
  torii pr create                       Open a pull request
  torii snapshot stash                  Stash work in progress
  torii workspace status                Status across all workspace repos
  torii worktree add -b hotfix          Spin up a sibling worktree on a new branch
  torii submodule add <url> vendor/lib  Embed another repo at a pinned commit
  torii subtree pull --prefix=vendor/x  Fetch upstream into a vendored subtree

Interactive UI:
  torii tui                             Launch terminal UI

Run 'torii <command> --help' for detailed usage of any command.")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new repository
    #[command(after_help = "Examples:
  torii init               Initialize in current directory
  torii init --path ~/projects/myrepo   Initialize in specific path")]
    Init {
        /// Path to initialize (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },

    /// Save current work (simplified commit)
    #[command(after_help = "Examples:
  torii save -m \"fix: null check\"              Commit staged changes
  torii save -am \"feat: add login\"             Stage all and commit
  torii save src/auth.rs -m \"fix: token\"       Stage specific file and commit
  torii save --amend -m \"fix: typo\"            Amend last commit message
  torii save --revert abc1234 -m \"revert\"      Revert a specific commit
  torii save --reset HEAD~1 --reset-mode soft  Undo last commit, keep changes
  torii save --unstage src/secret.rs            Remove a path from the index
  torii save --unstage --all                    Unstage everything")]
    Save {
        /// Commit message (required for commit/amend; ignored with --reset/--revert/--unstage)
        #[arg(short, long, required_unless_present_any = ["reset", "revert", "unstage"])]
        message: Option<String>,

        /// Stage all changes before committing (or, with --unstage, unstage all paths)
        #[arg(short, long)]
        all: bool,

        /// Specific files to stage before committing (or unstage with --unstage)
        #[arg(value_name = "FILES")]
        files: Vec<PathBuf>,

        /// Amend the previous commit
        #[arg(long)]
        amend: bool,

        /// Revert a specific commit by hash
        #[arg(long, value_name = "HASH")]
        revert: Option<String>,

        /// Reset to a specific commit (no commit message needed)
        #[arg(long, value_name = "HASH")]
        reset: Option<String>,

        /// Reset mode (default: mixed):
        ///   soft  — keep changes staged
        ///   mixed — keep changes in working tree, unstaged
        ///   hard  — discard all changes
        #[arg(long, default_value = "mixed", verbatim_doc_comment)]
        reset_mode: String,

        /// Unstage paths instead of committing (kept on disk). Use with FILES or --all.
        #[arg(long, conflicts_with_all = ["amend", "revert", "reset"])]
        unstage: bool,

        /// Skip pre-save / post-save hooks defined in .toriignore
        #[arg(long)]
        skip_hooks: bool,

        /// Force GPG-signing for this commit even if
        /// `git.sign_commits` is `false`. Requires `git.gpg_key` to
        /// be set (or `user.signingkey`).
        #[arg(short = 'S', long = "sign")]
        sign: bool,

        /// Force-disable GPG-signing for this commit, overriding a
        /// global `git.sign_commits = true`. Mutually exclusive with
        /// `--sign`.
        #[arg(long = "no-sign", conflicts_with = "sign")]
        no_sign: bool,
    },

    /// Sync with remote (pull+push) or integrate a branch
    #[command(after_help = "Examples:
  torii sync                       Pull from remote then push
  torii sync --pull                Pull only
  torii sync --push                Push only
  torii sync --force               Force push (rewrites remote history)
  torii sync --fetch               Fetch from the tracking remote
  torii sync --fetch upstream      Fetch from a specific remote
  torii sync --fetch --all         Fetch from every configured remote
  torii sync main                  Integrate main into current branch (smart merge/rebase)
  torii sync main --merge          Force merge strategy
  torii sync main --rebase         Force rebase strategy
  torii sync main --preview        Preview what would happen without executing")]
    Sync {
        /// When `--fetch` is present: name of the remote to fetch from
        /// (e.g. `upstream`). Without `--fetch`: branch to integrate
        /// (smart merge/rebase). If omitted, syncs with the tracking remote.
        branch: Option<String>,

        /// Pull only
        #[arg(short, long)]
        pull: bool,

        /// Push only
        #[arg(short = 'P', long)]
        push: bool,

        /// Force push (rewrites remote history — use with caution)
        #[arg(short, long)]
        force: bool,

        /// Fetch remote refs without merging
        #[arg(long)]
        fetch: bool,

        /// With `--fetch`, fetch from every configured remote (not just
        /// the tracking remote). Mutually exclusive with a named remote.
        #[arg(long, requires = "fetch", conflicts_with = "branch")]
        all: bool,

        /// Force merge strategy when integrating a branch
        #[arg(long)]
        merge: bool,

        /// Force rebase strategy when integrating a branch
        #[arg(long)]
        rebase: bool,

        /// Preview integration without executing
        #[arg(long)]
        preview: bool,

        /// Verify local vs remote head without pulling/pushing
        #[arg(long)]
        verify: bool,

        /// Skip pre-sync / post-sync hooks defined in .toriignore
        #[arg(long)]
        skip_hooks: bool,
    },

    /// Show repository status
    #[command(after_help = "Examples:
  torii status              Show staged, unstaged, and untracked files
  torii status --tracked    List every tracked file (≡ git ls-files)
  torii status --tracked -z Null-separated output (scripting)")]
    Status {
        /// Instead of the normal status, print every tracked file in the
        /// index, one per line. Equivalent to `git ls-files`. Useful for
        /// piping into other tools.
        #[arg(long)]
        tracked: bool,

        /// With --tracked, separate entries by NUL instead of newline.
        /// Same semantics as `git ls-files -z`. Safe for paths with
        /// embedded newlines.
        #[arg(short = 'z', long, requires = "tracked")]
        null: bool,
    },

    /// Show commit history
    #[command(after_help = "Examples:
  torii log                          Last 10 commits
  torii log -n 50                    Last 50 commits
  torii log --oneline                One line per commit
  torii log --graph                  Branch graph
  torii log --oneline --graph        Compact graph view
  torii log --author \"Alice\"         Filter by author
  torii log --since 2024-01-01       Commits after date
  torii log --until 2024-12-31       Commits before date
  torii log --grep \"feat\"            Filter by message pattern
  torii log --stat                   Show file change stats per commit")]
    Log {
        /// Number of commits to show (default: 10)
        #[arg(short = 'n', long)]
        count: Option<usize>,

        /// Show one line per commit
        #[arg(long)]
        oneline: bool,

        /// Show branch graph
        #[arg(long)]
        graph: bool,

        /// Filter by author name or email
        #[arg(long)]
        author: Option<String>,

        /// Show commits after this date (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,

        /// Show commits before this date (YYYY-MM-DD)
        #[arg(long)]
        until: Option<String>,

        /// Filter commits whose message matches this pattern
        #[arg(long)]
        grep: Option<String>,

        /// Show file change statistics per commit
        #[arg(long)]
        stat: bool,

        /// Show reflog (HEAD movement history) instead of commit log
        #[arg(long)]
        reflog: bool,

        /// Add a column showing each commit's GPG signature status
        /// (G=good, U=unknown signer, B=bad, N=none). Verification
        /// runs against the local keyring via `gpg --verify`.
        #[arg(long)]
        signatures: bool,
    },

    /// Show unstaged or staged changes
    #[command(after_help = "Examples:
  torii diff            Show unstaged changes
  torii diff --staged   Show staged changes (ready to commit)
  torii diff --last     Show changes in last commit")]
    Diff {
        /// Show staged changes
        #[arg(long)]
        staged: bool,

        /// Show last commit diff
        #[arg(long)]
        last: bool,
    },

    /// **Deprecated** alias — use `torii show <file> --blame` instead.
    /// Will be removed in 0.8.
    #[command(hide = true)]
    Blame {
        /// File to blame
        file: String,

        /// Line range (e.g., 10,20)
        #[arg(short = 'L', long)]
        lines: Option<String>,
    },

    /// Scan for sensitive data (secrets, tokens, keys)
    #[command(after_help = "Examples:
  torii scan                       Scan staged files for secrets
  torii scan --history             Scan entire git history for secrets
  torii scan --commits             Scan commits against policies/commits.toml
  torii scan --commits --limit 50  Limit how many commits to evaluate
  torii scan --commits --policy-file path/to/commits.toml")]
    Scan {
        /// Scan the entire git history instead of only staged files
        #[arg(long)]
        history: bool,
        /// Evaluate commits against policies/commits.toml by default
        #[arg(long)]
        commits: bool,
        /// Path to the policy file (default: <repo>/policies/commits.toml)
        #[arg(long, value_name = "PATH")]
        policy_file: Option<PathBuf>,
        /// Max commits to scan when --commits is set (default: 200)
        #[arg(long, default_value = "200")]
        limit: usize,
    },

    /// Apply a commit from another branch to the current branch
    #[command(
        name = "cherry-pick",
        after_help = "Examples:
  torii cherry-pick abc1234           Apply a commit
  torii cherry-pick --continue        Resume after resolving conflicts
  torii cherry-pick --abort           Abort an in-progress cherry-pick"
    )]
    CherryPick {
        /// Commit hash to cherry-pick
        commit: Option<String>,

        /// Continue after resolving conflicts
        #[arg(long)]
        r#continue: bool,

        /// Abort cherry-pick
        #[arg(long)]
        abort: bool,
    },

    /// Manage branches
    #[command(after_help = "Examples:
  torii branch                      List local branches
  torii branch --all                List local and remote branches
  torii branch feature/auth -c      Create and switch to branch
  torii branch gh-pages -c --orphan Create orphan branch (no history)
  torii branch main                 Switch to existing branch
  torii branch -d feature/auth              Delete local branch
  torii branch -d feature/auth --force      Force delete (not merged)
  torii branch --delete-remote feature/auth Delete branch on all remotes
  torii branch --rename new-name            Rename current branch")]
    Branch {
        /// Branch name to switch to or create with -c
        name: Option<String>,

        /// Create new branch and switch to it
        #[arg(short, long)]
        create: bool,

        /// Create the branch with no parents/history (requires -c)
        #[arg(long)]
        orphan: bool,

        /// Delete local branch by name
        #[arg(short, long)]
        delete: Option<String>,

        /// Force delete local branch even if not merged
        #[arg(long)]
        force: bool,

        /// Delete branch on all configured remotes
        #[arg(long)]
        delete_remote: Option<String>,

        /// List local branches
        #[arg(short, long)]
        list: bool,

        /// Rename current branch to this name
        #[arg(short, long)]
        rename: Option<String>,

        /// Show all branches including remote
        #[arg(short, long)]
        all: bool,
    },

    /// Clone a repository
    #[command(after_help = "Examples:
  torii clone github user/repo                Clone from GitHub (auto SSH/HTTPS)
  torii clone gitlab user/repo                Clone from GitLab
  torii clone github user/repo /tmp/foo       Clone into /tmp/foo (positional dest)
  torii clone github user/repo -d my-dir      Same, with -d flag
  torii clone github user/repo --protocol https   Force HTTPS
  torii clone https://github.com/user/repo.git    Clone from full URL
  torii clone https://github.com/user/repo.git -d /tmp/foo
  torii clone git@github.com:user/repo.git        Clone via SSH URL

Supported platforms: github, gitlab, codeberg, bitbucket, gitea, forgejo

Protocol is auto-detected: SSH if keys are configured, HTTPS otherwise.
Override with --protocol or set default: torii config set mirror.default_protocol https")]
    Clone {
        /// Platform (github, gitlab, ...) or full URL (https://... / git@...)
        source: String,

        /// Repository as user/repo (when using platform shorthand)
        args: Vec<String>,

        /// Target directory name
        #[arg(short = 'd', long)]
        directory: Option<String>,

        /// Protocol to use: ssh or https (default: auto-detect)
        #[arg(long)]
        protocol: Option<String>,
    },

    /// Manage tags and releases
    #[command(after_help = "Examples:
  torii tag list                      List all tags
  torii tag create v1.2.0 -m \"Release\"   Create annotated tag
  torii tag delete v1.0.0             Delete a tag
  torii tag push v1.2.0               Push specific tag to remote
  torii tag push                      Push all tags to remote
  torii tag show v1.2.0               Show tag details
  torii tag release                   Auto-bump version from conventional commits
  torii tag release --bump minor      Force minor bump
  torii tag release --dry-run         Preview without creating tag

Auto-bump rules (Conventional Commits):
  feat:        → minor bump (0.1.0 → 0.2.0)
  fix: / perf: → patch bump (0.1.0 → 0.1.1)
  feat!:       → major bump (0.1.0 → 1.0.0)")]
    Tag {
        #[command(subcommand)]
        action: TagCommands,
    },

    /// Save and restore work-in-progress snapshots
    #[command(after_help = "Examples:
  torii snapshot create -n \"before-refactor\"   Create named snapshot
  torii snapshot list                           List all snapshots
  torii snapshot restore <id>                   Restore a snapshot
  torii snapshot delete <id>                    Delete a snapshot
  torii snapshot stash                          Stash current work
  torii snapshot stash -u                       Stash including untracked files
  torii snapshot unstash                        Restore latest stash
  torii snapshot unstash <id> --keep            Restore stash but keep it
  torii snapshot undo                           Undo last operation")]
    Snapshot {
        #[command(subcommand)]
        action: SnapshotCommands,
    },

    /// Mirror repository across multiple platforms
    #[command(after_help = "Examples:
  torii mirror add gitlab user paskidev myrepo --primary  Set GitLab as primary (source of truth)
  torii mirror add github user paskidev myrepo           Add GitHub as a replica mirror
  torii mirror promote github paskidev                   Promote a mirror to primary
  torii mirror sync                                      Push to all replica mirrors
  torii mirror sync --force                              Force push to all mirrors
  torii mirror list                                      List configured mirrors
  torii mirror remove github paskidev                    Remove a mirror
  torii mirror autofetch --enable --interval 30m         Auto-fetch every 30 min
  torii mirror autofetch --disable                       Disable auto-fetch
  torii mirror autofetch --status                        Show autofetch status

Supported platforms: github, gitlab, codeberg, bitbucket, gitea, forgejo")]
    Mirror {
        #[command(subcommand)]
        action: MirrorCommands,
    },

    /// Show commit, tag, or file details
    #[command(after_help = "Examples:
  torii show                      Show HEAD commit with diff
  torii show abc1234              Show specific commit
  torii show v1.0.0               Show tag details
  torii show src/main.rs --blame  Show line-by-line change history
  torii show src/main.rs --blame -L 10,20   Blame specific line range")]
    Show {
        /// Commit hash, tag name, ref, or file path (defaults to HEAD)
        object: Option<String>,

        /// Show blame for a file (who changed each line)
        #[arg(long)]
        blame: bool,

        /// Line range for blame (e.g., 10,20)
        #[arg(short = 'L', long, requires = "blame")]
        lines: Option<String>,

        /// Print the commit's GPG signature (ASCII armor) and the
        /// verification verdict. Implies the object is a commit;
        /// errors if the commit is unsigned.
        #[arg(long, conflicts_with = "blame")]
        signature: bool,
    },

    /// Re-sign one or more commits with the configured GPG key.
    ///
    /// Rewrites the commit objects to include (or replace) the
    /// `gpgsig` header. The commit OIDs CHANGE (a signed commit is a
    /// different object than an unsigned one); any branch / tag /
    /// child commit pointing at the old OID gets rewritten to the new
    /// one. Equivalent to a tiny `git filter-branch --commit-filter
    /// 'git commit-tree -S …'` on a range, but driven from torii.
    ///
    /// Examples:
    ///   torii sign HEAD              Re-sign HEAD
    ///   torii sign abc1234           Re-sign a specific commit
    ///   torii sign HEAD~5..HEAD      Re-sign the last 5 commits
    #[command(after_help = "Notes:
  - Refuses to run on commits that aren't reachable from HEAD —
    rewriting unreachable history is rarely what you want.
  - Refuses to run with a dirty working tree.
  - Use `--print-only` to inspect the resulting armor without
    actually mutating any refs.")]
    Sign {
        /// Single commit, range (`A..B`), or `HEAD`. Defaults to `HEAD`.
        target: Option<String>,

        /// Print the would-be signature without rewriting the
        /// commit. Useful for sanity-checking that gpg + the
        /// configured key produce something before committing to a
        /// history rewrite.
        #[arg(long)]
        print_only: bool,

        /// Skip the confirmation prompt when rewriting history.
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Manage commit history (rebase, cherry-pick, blame, scan)
    #[command(after_help = "Examples:
  torii history reflog                        Show HEAD movement history
  torii history rebase main                   Rebase current branch onto main
  torii history rebase -i HEAD~5              Interactive rebase last 5 commits
  torii history rebase --continue             Continue after resolving conflicts
  torii history rebase --abort                Abort current rebase
  torii history cherry-pick abc1234           Apply a commit to current branch
  torii history blame src/main.rs             Line-by-line change history
  torii history blame src/main.rs -L 10,20    Specific line range
  torii history scan                          Scan staged files for secrets
  torii history scan --history                Scan entire git history for secrets
  torii history remove-file secrets.txt       Purge file from all commits
  torii history rewrite \"2024-01-01\" \"2024-12-31\"  Rewrite commit dates
  torii history clean                         GC and expire reflog")]
    History {
        #[command(subcommand)]
        action: HistoryCommands,
    },

    /// Manage Torii configuration
    #[command(after_help = "Examples:
  torii config list                              Show all config values
  torii config list --local                      Show local repo config
  torii config get user.name                     Get a value
  torii config set user.name \"Alice\"             Set a global value
  torii config set user.email \"a@b.com\" --local  Set a local value
  torii config set auth.github_token ghp_xxx     Set GitHub token
  torii config set auth.gitlab_token glpat-xxx   Set GitLab token
  torii config set mirror.default_protocol https Use HTTPS by default
  torii config edit                              Open config in editor
  torii config reset                             Reset to defaults

Available keys:
  user.name, user.email, user.editor
  auth.github_token, auth.gitlab_token, auth.gitea_token
  auth.forgejo_token, auth.codeberg_token
  git.default_branch, git.sign_commits, git.pull_rebase
  mirror.default_protocol, mirror.autofetch_enabled
  snapshot.auto_enabled, snapshot.auto_interval_minutes
  ui.colors, ui.emoji, ui.verbose, ui.date_format")]
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },

    /// Manage credentials — gitorii.com cloud API key AND per-platform
    /// tokens (github, gitlab, gitea, forgejo, codeberg, cargo, …).
    #[command(after_help = "Examples — cloud key:
  torii auth login                  Prompt for the gitorii.com API key
  torii auth login --key gitorii_sk_…   Save a key non-interactively
  torii auth status                 Show org / plan tied to the key
  torii auth logout                 Forget the local key

Examples — platform tokens:
  torii auth set github ghp_xxx     Save a GitHub token globally
  torii auth set cargo cio_xxx       Save a crates.io token
  torii auth set gitlab glpat-xxx --local  Per-repo token (.torii/auth.toml)
  torii auth list                    Every provider's state, masked
  torii auth get github              Just one, masked
  torii auth remove gitea            Drop it
  torii auth doctor                  Where does each token come from?

Resolution order: env vars (GITHUB_TOKEN, GITLAB_TOKEN, CARGO_REGISTRY_TOKEN, …)
                  > .torii/auth.toml (per-repo) > ~/.config/torii/auth.toml (global)")]
    Auth {
        #[command(subcommand)]
        action: AuthCommands,
    },

    /// Publish the current crate to crates.io. Thin wrapper over
    /// `cargo publish` that injects `auth.cargo` from `torii auth` so you
    /// don't need to keep CARGO_REGISTRY_TOKEN in `.env` or your shell.
    #[command(after_help = "Examples:
  torii publish                       Validate + upload (uses auth.cargo)
  torii publish --dry-run             Validate without uploading
  torii publish --no-verify           Skip the local build step
  torii publish --token cio_xxx       Override the token for this invocation

Set the persistent token once with:
  torii auth set cargo <token>")]
    Publish {
        /// Don't actually upload to crates.io — just package and verify.
        #[arg(long)]
        dry_run: bool,
        /// Skip the verify-build step (faster but riskier — crates.io
        /// rebuilds server-side anyway and yanks bad uploads).
        #[arg(long)]
        no_verify: bool,
        /// Use this token for this invocation only (overrides auth.cargo).
        #[arg(long)]
        token: Option<String>,
        /// Pass `--allow-dirty` through to cargo (uncommitted changes).
        #[arg(long)]
        allow_dirty: bool,
    },

    /// Manage remote repositories (create, delete, configure)
    #[command(after_help = "Examples:
  torii remote create github myrepo --public          Create public repo on GitHub
  torii remote create gitlab myrepo --private         Create private repo on GitLab
  torii remote create github myrepo --private --push  Create and push current branch
  torii remote delete github owner myrepo --yes        Delete repo (no confirmation)
  torii remote visibility github owner myrepo --public Make repo public
  torii remote visibility codeberg user myrepo --private Codeberg/Gitea/Forgejo via shared API
  torii remote visibility bitbucket workspace myrepo --private Bitbucket Cloud
  torii remote visibility sourcehut ~user myrepo --private  Sourcehut (PUBLIC/UNLISTED/PRIVATE)
  torii remote configure github owner myrepo --default-branch main
  torii remote info github owner myrepo               Show repo details
  torii remote list github                            List all your GitHub repos

Supported platforms: github, gitlab, codeberg, gitea, forgejo, bitbucket, sourcehut, azure, radicle

Visibility availability:
  github, gitlab, codeberg, gitea, forgejo, bitbucket, sourcehut — fully wired (0.7.19+)
  azure   — visibility is per-project on Azure DevOps, not per-repo. Error directs to the project settings.
  radicle — peer-to-peer; reachability is governed by seeding, not by a flag. Error directs to `rad node`.")]
    Remote {
        #[command(subcommand)]
        action: RemoteCommands,
    },

    /// Manage multi-repo workspaces
    #[command(after_help = "Examples:
  torii workspace add work ~/repos/api   Add repo to workspace
  torii workspace list                   List all workspaces
  torii workspace status work            Show status of all repos
  torii workspace save work -m \"wip\"    Commit across all repos
  torii workspace sync work              Pull+push all repos")]
    Workspace {
        #[command(subcommand)]
        action: WorkspaceCommands,
    },

    /// Manage pull requests / merge requests
    #[command(after_help = "Examples:
  torii pr list                          List open PRs
  torii pr list --state closed           List closed PRs
  torii pr create -t \"feat: login\" -b main
  torii pr merge 42                      Merge PR #42
  torii pr merge 42 --method squash      Squash merge
  torii pr close 42                      Close PR #42
  torii pr checkout 42                   Checkout PR branch
  torii pr open 42                       Open PR in browser")]
    Pr {
        #[command(subcommand)]
        action: PrCommands,
    },

    /// Manage issues
    #[command(after_help = "Examples:
  torii issue list                        List open issues
  torii issue list --state closed         List closed issues
  torii issue create -t \"bug: crash\"      Create issue
  torii issue create -t \"title\" -d \"desc\" Create with description
  torii issue close 42                    Close issue #42
  torii issue comment 42 -m \"Fixed in v2\" Add a comment")]
    Issue {
        #[command(subcommand)]
        action: IssueCommands,
    },

    /// Manage CI pipelines (GitLab Pipelines / GitHub Actions workflow runs)
    #[command(after_help = "Examples — basic ops on the default (`origin`) remote:
  torii pipeline list                                       Recent pipelines
  torii pipeline list --status failed                       Only failed
  torii pipeline list --limit 50                            Up to 50 entries
  torii pipeline cancel 12345                               Cancel one
  torii pipeline retry 12345                                Re-run failed jobs
  torii pipeline delete 12345                                Delete one
  torii pipeline delete --status failed --yes               Batch: every failed
  torii pipeline delete --status failed --older-than 7d --yes

Examples — multi-platform with `--remote NAME`:
  torii pipeline list --remote origin                       Same as default
  torii pipeline list --remote github-paskidev              GitHub mirror's pipelines
  torii pipeline retry 8421 --remote github-paskidev        Retry on GitHub side
  torii pipeline delete --status canceled --remote origin --yes

  By default the platform (github / gitlab / gitea) is auto-detected
  from the `origin` remote URL. For repos mirrored across platforms
  each backend has its own pipeline runs — use `--remote NAME` to
  target a specific remote. The flag is global within the command so
  it can appear before or after the subcommand verb. Each platform
  has its own auth token via `torii auth set <platform>`. See
  `README.md` for the full multi-platform doc.

  Gitea / Codeberg / Forgejo: detected from `codeberg.org` URLs
  automatically (added in 0.7.13); self-hosted instances require
  explicit declaration via `~/.config/torii/platforms.toml` (0.8.0).

`--status` accepts: success | failed | running | canceled | pending.
`delete` requires either an explicit ID or at least one filter; `--yes`
skips the confirmation prompt.")]
    Pipeline {
        #[command(subcommand)]
        action: PipelineCommands,
        /// Which git remote to use for platform detection. Default is
        /// `origin`. Set to e.g. `github-paskidev` to manage the
        /// pipeline on the GitHub mirror of a multi-platform project.
        #[arg(long, default_value = "origin", global = true)]
        remote: String,
    },

    /// Drill into individual CI jobs (GitLab Pipelines / GitHub Actions workflow_runs jobs)
    #[command(after_help = "Examples — basic ops on the default (`origin`) remote:
  torii job list --pipeline 1234                      Jobs in a pipeline
  torii job list --pipeline 1234 --status failed      Only failed jobs
  torii job log 5678                                  Print full log
  torii job log 5678 --tail 50                        Last 50 lines (failure post-mortem)
  torii job retry 5678                                Re-run one job  (GitLab only)
  torii job cancel 5678                               Cancel a job    (GitLab only)
  torii job artifacts 5678 -o artifacts.zip           Per-job download (GitLab only)
  torii job erase 5678                                Clear log + artifacts, keep entry (GitLab only)

Examples — multi-platform with `--remote NAME`:
  torii job list --pipeline 9876 --remote github-paskidev   Jobs in the GitHub run
  torii job log 87654 --remote github-paskidev --tail 30    Last 30 lines from GitHub
  torii job retry 5678 --remote origin                       Default is origin, equivalent

  Default platform is auto-detected from the `origin` remote. For
  multi-platform repos use `--remote NAME` to target a specific
  remote. See `README.md` (CI / platform management section) for
  the full multi-platform doc.

Platform notes — GitHub Actions:
  Some operations (`retry`, `cancel`, `artifacts`, `erase`) are scoped to the
  workflow run on GitHub, not individual jobs. Those subcommands return an
  error pointing at the equivalent `torii pipeline` operation.")]
    Job {
        #[command(subcommand)]
        action: JobCommands,
        /// Which git remote to use for platform detection. Default
        /// is `origin`. See `torii pipeline --help` for context.
        #[arg(long, default_value = "origin", global = true)]
        remote: String,
    },

    /// Manage CI runners (self-hosted agents on GitLab / GitHub Actions)
    #[command(after_help = "Examples — basic ops on the default (`origin`) remote:
  torii runner list                              List project's runners
  torii runner show 42                           Detail (status, IP, tags, version)
  torii runner remove 42 -y                      Delete a runner
  torii runner reset-token 42                    Print new auth token (GitLab only)
  torii runner pause 42                          Pause (GitLab only)
  torii runner resume 42                         Resume

Examples — multi-platform:
  torii runner list --remote github-paskidev     GitHub self-hosted runners

Platform support:
  - GitLab:  list + show + remove + reset-token + pause + resume
  - GitHub:  list + show + remove (no token reset, no pause — see error
             messages for the documented workaround)
  - Others:  not implemented yet (future: Bitbucket Pipelines, Azure agents)")]
    Runner {
        #[command(subcommand)]
        action: RunnerCommands,
        /// Which git remote to use for platform detection. Default `origin`.
        #[arg(long, default_value = "origin", global = true)]
        remote: String,
    },

    /// Manage the platforms registry (self-hosted GitLab, Gitea /
    /// Forgejo, GitHub Enterprise, Bitbucket Data Center).
    ///
    /// Entries are loaded from `~/.config/torii/platforms.toml` and
    /// `<repo>/.torii/platforms.toml`. Local overrides global by
    /// `name`. Builtins (github.com, gitlab.com, codeberg.org,
    /// bitbucket.org) live in code and can be shadowed by an entry
    /// with the same name.
    #[command(after_help = "Examples:
  torii platforms list                                   Builtins + custom
  torii platforms add work-gitlab \\
      --kind gitlab --domain gitlab.work.io \\
      --api https://gitlab.work.io/api/v4 \\
      --web https://gitlab.work.io
  torii platforms add ghe --kind github_enterprise \\
      --domain ghe.work.io --api https://ghe.work.io/api/v3 \\
      --web https://ghe.work.io
  torii platforms remove work-gitlab                     Drop custom entry
  torii platforms test work-gitlab                       Ping the API with the stored token")]
    Platforms {
        #[command(subcommand)]
        action: PlatformsCommands,
    },

    /// Manage the Package Registry — release binaries / artifacts stored on the platform.
    #[command(after_help = "Examples — basic ops on the default (`origin`) remote:
  torii package list                                       List packages
  torii package list --type generic                        Filter by package type
  torii package list --name gitorii                        Substring search on name
  torii package files 12345                                Files inside a package
  torii package delete 12345                                Delete one
  torii package delete --version v0.7.0 --yes              Batch delete all v0.7.0
  torii package delete --older-than 90d --yes              Batch delete > 90 days old

Examples — multi-platform with `--remote NAME`:
  torii package list --remote origin                       Same as default
  torii package list --remote github-paskidev              GitHub side (returns error
                                                            because GitHub has no
                                                            Generic Package Registry)
  torii package delete --version v0.7.0 --remote origin --yes

  Default platform is auto-detected from the `origin` remote. See
  `README.md` for the full multi-platform doc.

Platform notes:
  gitlab-only. On GitHub, binary release assets are managed through
  `torii release` since GitHub doesn't expose a standalone package
  registry equivalent. Using `--remote NAME` on a github-pointing remote
  returns an error suggesting `torii release` instead.")]
    Package {
        #[command(subcommand)]
        action: PackageCommands,
        /// Which git remote to use for platform detection. Default `origin`.
        #[arg(long, default_value = "origin", global = true)]
        remote: String,
    },

    /// Manage Release pages (GitLab Releases / GitHub Releases)
    #[command(after_help = "Examples — basic ops on the default (`origin`) remote:
  torii release list                                       Recent releases
  torii release show v0.7.9                                One release's details
  torii release edit v0.7.9 --name 'New title'             Rename
  torii release edit v0.7.9 --notes notes.md               Replace description (file)
  torii release edit v0.7.9 --notes - <<< 'inline'         Replace description (stdin)
  torii release delete v0.7.9 --yes                        Delete release entity (tag stays)

Examples — multi-platform with `--remote NAME`:
  torii release list --remote origin                       Same as default
  torii release list --remote github-paskidev              GitHub releases
  torii release edit v0.7.9 --notes new.md --remote github-paskidev
  torii release delete v0.7.9 --remote origin --yes        Only the gitlab side

  Each platform stores releases independently — editing the description
  on gitlab doesn't sync to github (yet — that's torii-cloud territory).
  Default platform is auto-detected from the `origin` remote URL. See
  `README.md` for the full multi-platform doc.

The release identifier is the tag name (`v0.7.9`), not a numeric id —
matches how both GitLab and GitHub address releases in their UIs.")]
    Release {
        #[command(subcommand)]
        action: ReleaseCommands,
        /// Which git remote to use for platform detection. Default `origin`.
        #[arg(long, default_value = "origin", global = true)]
        remote: String,
    },

    /// Manage .toriignore rules (paths, secrets, size, hooks)
    #[command(after_help = "Examples:
  torii ignore add 'build/'                         Add path to public .toriignore
  torii ignore add --local 'internal/billing/'      Add path to .toriignore.local (not committed)
  torii ignore secret 'AKIA[0-9A-Z]{16}' --name AWS Add secret regex to .local (private by default)
  torii ignore list                                 Show effective rules (public + local merged)

The .toriignore.local file is machine-private — it is auto-excluded from git
and never committed. Use it for rules whose existence would aid recon if the
public repo leaked (proprietary secret formats, internal paths, etc).")]
    Ignore {
        #[command(subcommand)]
        action: IgnoreCommands,
    },

    /// Open the interactive TUI dashboard
    #[command(after_help = "Examples:
  torii tui   Open dashboard (status, log, file navigation)")]
    Tui,

    /// Manage worktrees — multiple working copies of the same repo, each on
    /// its own branch, sharing the underlying objects.
    #[command(after_help = "Examples:
  torii worktree add -b feature/auth                  Create branch + worktree at ../<repo>-feature-auth/
  torii worktree add ../hotfix -b release/0.7         Create branch at explicit path
  torii worktree add ../hotfix release/0.7            Check out existing branch in worktree
  torii worktree list                                 Show every worktree + status
  torii worktree remove ../hotfix                     Remove worktree (snapshot taken automatically)
  torii worktree remove ../hotfix --force             Remove even if dirty
  torii worktree prune                                Clean up metadata of deleted worktrees
  torii worktree open ../hotfix                       Launch $SHELL in that worktree

The default path (when omitted) is derived from worktree.base_dir config:
  torii config set worktree.base_dir ~/worktrees    # default is '..' (sibling dirs)
  torii config set worktree.base_dir ..             # restore default")]
    Worktree {
        #[command(subcommand)]
        action: Option<WorktreeCommands>,
    },

    /// Manage submodules — embed another git repo at a path and commit
    /// inside this one. The embedded repo's history stays separate.
    #[command(after_help = "Examples:
  torii submodule add git@github.com:owner/lib.git vendor/lib            Add at vendor/lib
  torii submodule add git@.../lib.git vendor/lib --branch main           Pin a tracked branch
  torii submodule status                                                 List submodules + state
  torii submodule init                                                   Copy .gitmodules URLs to .git/config
  torii submodule update --init                                          Init missing + fetch+checkout pinned commit
  torii submodule sync                                                   Re-copy URLs (after upstream URL change)
  torii submodule foreach 'cargo build'                                  Run a command in each submodule
  torii submodule remove vendor/lib                                       Deregister + clean up

Recursion (--recursive) is not yet implemented; nested submodules need a
manual loop for now.")]
    Submodule {
        #[command(subcommand)]
        action: Option<SubmoduleCommands>,
    },

    /// Manage subtrees — merge another project's history into a
    /// subdirectory of this repo, no second clone, no .gitmodules. Thin
    /// wrapper over `git subtree` (which must be installed).
    #[command(after_help = "Examples:
  torii subtree add    --prefix=vendor/lib git@... main --squash       Initial import
  torii subtree pull   --prefix=vendor/lib git@... main --squash       Fetch upstream changes
  torii subtree push   --prefix=vendor/lib git@... main                Push subtree back
  torii subtree split  --prefix=vendor/lib -b lib-split                Extract history to a branch
  torii subtree merge  --prefix=vendor/lib some-ref                    Finish a manual merge

Pass --squash on add/pull/merge to flatten upstream history into a single
merge commit. Without it the full upstream graph is brought in.")]
    Subtree {
        #[command(subcommand)]
        action: SubtreeCommands,
    },

    /// Binary search for the commit that introduced a regression.
    /// State-machine wrapper over `git bisect`.
    #[command(after_help = "Examples:
  torii bisect start                 Enter bisect mode
  torii bisect bad                   Current HEAD is bad
  torii bisect good v0.6.0           v0.6.0 was good
  torii bisect skip                  Current commit unbuildable, skip
  torii bisect run cargo test        Auto-run test on each candidate
  torii bisect log                   Print the search log
  torii bisect reset                 Exit bisect mode, restore HEAD")]
    Bisect {
        #[command(subcommand)]
        action: BisectCommands,
    },

    /// Pretty name for HEAD based on the nearest tag (≡ git describe).
    /// Format: `<tag>-<n>-g<short>` or just `<tag>` if HEAD is on a tag.
    Describe {
        /// Include lightweight tags (default: annotated only).
        #[arg(long)]
        tags: bool,
        /// Always use the long format even if HEAD is on a tag.
        #[arg(long)]
        long: bool,
        /// Append `-dirty` if the working tree has uncommitted changes.
        #[arg(long)]
        dirty: bool,
        /// How many candidate tags to consider (default: 10).
        #[arg(long, default_value = "10")]
        candidates: u32,
    },

    /// Export a tree or commit as a tarball/zip (wrapper over `git archive`).
    #[command(after_help = "Examples:
  torii archive HEAD -o release.tar.gz
  torii archive v0.6.9 --prefix=gitorii-0.6.9/ -o gitorii-0.6.9.tar.gz
  torii archive HEAD --format=zip -o release.zip")]
    Archive {
        /// Revision (HEAD, tag, branch, commit) to archive.
        revision: String,
        /// Output file path. Without it, writes to stdout.
        #[arg(short = 'o', long)]
        output: Option<String>,
        /// Force format (tar/zip/tar.gz/tgz). Otherwise inferred from extension.
        #[arg(long)]
        format: Option<String>,
        /// Prepend each entry with this prefix (e.g. `myproj-1.0/`).
        #[arg(long)]
        prefix: Option<String>,
    },

    /// Remove tracked files from index and working tree.
    #[command(
        alias = "rm",
        after_help = "Examples:
  torii remove src/old.rs                 Remove + untrack
  torii remove src/old.rs --cached        Untrack only (keep on disk)
  torii remove -r vendor/legacy/          Recursive
  torii remove --force src/dirty.rs       Drop local changes

`torii rm` works too — alias kept for users coming from git."
    )]
    Remove {
        /// One or more paths to remove.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Don't delete from disk, just untrack.
        #[arg(long)]
        cached: bool,
        /// Allow removing directories recursively.
        #[arg(short = 'r', long)]
        recursive: bool,
        /// Proceed even if the file has uncommitted modifications.
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// Rename (or move) a tracked file/directory.
    #[command(
        alias = "mv",
        after_help = "Examples:
  torii rename old.rs new.rs              Stage a rename
  torii rename src/a.rs src/b.rs --force  Overwrite if target exists

`torii mv` works too — alias kept for users coming from git."
    )]
    Rename {
        /// Source path.
        from: PathBuf,
        /// Destination path.
        to: PathBuf,
        /// Overwrite target if it already exists.
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// Search tracked content for a pattern (wrapper over `git grep`).
    #[command(after_help = "Examples:
  torii grep TODO                     Search for TODO in tracked files
  torii grep -i \"fix me\"               Case-insensitive
  torii grep -l unsafe                List files containing 'unsafe'
  torii grep -w main src/             Word-boundary match, in src/ only")]
    Grep {
        /// Pattern (regex by default — pass --fixed-string for literal).
        pattern: String,
        /// Restrict search to these paths.
        #[arg(value_name = "PATH")]
        paths: Vec<String>,
        /// Case-insensitive.
        #[arg(short = 'i', long)]
        ignore_case: bool,
        /// Match whole words only.
        #[arg(short = 'w', long)]
        word_regexp: bool,
        /// Print only file names that contain a match.
        #[arg(short = 'l', long)]
        files_with_matches: bool,
        /// Suppress line numbers (which are on by default in torii).
        #[arg(long)]
        no_line_number: bool,
    },

    /// Annotations attached to commits (wrapper over `git notes`).
    /// Stored in `refs/notes/commits` so commit OIDs stay stable.
    #[command(after_help = "Examples:
  torii notes                              List commits with notes
  torii notes add HEAD -m \"reviewed by X\"  Add a note to HEAD
  torii notes append HEAD -m \"and also Y\"  Append to an existing note
  torii notes show HEAD                    Show the note attached to HEAD
  torii notes edit HEAD                    Open $EDITOR on it
  torii notes copy v0.6.8 v0.6.9           Copy notes between commits
  torii notes remove HEAD                  Drop the note")]
    Notes {
        #[command(subcommand)]
        action: Option<NotesCommands>,
    },

    /// Export commits as patch files / apply patches as new commits.
    /// Wrapper over `git format-patch` and `git am`.
    #[command(after_help = "Examples:
  torii patch export HEAD~3..HEAD                Export last 3 commits
  torii patch export v0.6.8..HEAD -o /tmp/p/      Into a directory
  torii patch export HEAD~1..HEAD --stdout       To stdout
  torii patch apply 0001-fix.patch                Apply a single patch
  torii patch apply *.patch                        Apply a series
  torii patch apply --continue                    After resolving conflicts")]
    Patch {
        #[command(subcommand)]
        action: PatchCommands,
    },

    /// Remove untracked files from the working tree (≡ `git clean`).
    /// Defaults to a dry-run for safety; pass -f to actually delete.
    #[command(after_help = "Examples:
  torii clean             Dry-run, list what would go
  torii clean -f          Actually delete untracked files
  torii clean -f -d       Include untracked directories
  torii clean -f -x       Also remove .gitignore-matched files
  torii clean -f -X       ONLY remove .gitignore-matched files")]
    Clean {
        /// Actually delete (otherwise dry-run).
        #[arg(short = 'f', long)]
        force: bool,
        /// Recurse into untracked directories.
        #[arg(short = 'd', long)]
        dirs: bool,
        /// Also remove ignored files.
        #[arg(short = 'x', long)]
        include_ignored: bool,
        /// Only remove ignored files.
        #[arg(short = 'X', long)]
        only_ignored: bool,
    },
}

impl Cli {
    pub fn execute(&self) -> Result<()> {
        match &self.command {
            Commands::Init { path } => repo::init(path),
            Commands::Save {
                message,
                all,
                files,
                amend,
                revert,
                reset,
                reset_mode,
                unstage,
                skip_hooks,
                sign,
                no_sign,
            } => repo::save(
                message, all, files, amend, revert, reset, reset_mode, unstage, skip_hooks, sign,
                no_sign,
            ),
            Commands::Sync {
                branch,
                pull,
                push,
                force,
                fetch,
                all,
                merge,
                rebase,
                preview,
                verify,
                skip_hooks,
            } => repo::sync(
                branch, pull, push, force, fetch, all, merge, rebase, preview, verify, skip_hooks,
            ),
            Commands::Status { tracked, null } => repo::status(tracked, null),
            Commands::Log {
                count,
                oneline,
                graph,
                author,
                since,
                until,
                grep,
                stat,
                reflog,
                signatures,
            } => repo::log(
                count, oneline, graph, author, since, until, grep, stat, reflog, signatures,
            ),
            Commands::Diff { staged, last } => repo::diff(staged, last),
            Commands::Blame { file, lines } => repo::blame(file, lines),
            Commands::Scan {
                history,
                commits,
                policy_file,
                limit,
            } => scan::run(history, commits, policy_file, limit),
            Commands::CherryPick {
                commit,
                r#continue,
                abort,
            } => repo::cherry_pick(commit, r#continue, abort),
            Commands::Branch {
                name,
                create,
                orphan,
                delete,
                force,
                delete_remote,
                list,
                rename,
                all,
            } => repo::branch(
                name,
                create,
                orphan,
                delete,
                force,
                delete_remote,
                list,
                rename,
                all,
            ),
            Commands::Clone {
                source,
                args,
                directory,
                protocol,
            } => clone::run(source, args, directory, protocol),
            Commands::Tag { action } => tag::run(action),
            Commands::Snapshot { action } => snapshot::run(action),
            Commands::Mirror { action } => mirror::run(action),
            Commands::Auth { action } => auth::run_auth(action),
            Commands::Publish {
                dry_run,
                no_verify,
                token,
                allow_dirty,
            } => publish::run(dry_run, no_verify, token, allow_dirty),
            Commands::Config { action } => config::run(action),
            Commands::Remote { action } => remote::run(action),
            Commands::Show {
                object,
                blame,
                lines,
                signature,
            } => repo::show(object, blame, lines, signature),
            Commands::Sign {
                target,
                print_only,
                yes,
            } => sign::run_sign(target.as_deref(), *print_only, *yes),
            Commands::History { action } => history::run(action),
            Commands::Workspace { action } => workspace::run(action),
            Commands::Pr { action } => pr::run(action),
            Commands::Issue { action } => issue::run(action),
            Commands::Pipeline { action, remote } => pipeline::run(action, remote),
            Commands::Job { action, remote } => pipeline::run_job(action, remote),
            Commands::Platforms { action } => platforms::run(action),
            Commands::Runner { action, remote } => runner::run(action, remote),
            Commands::Package { action, remote } => package::run(action, remote),
            Commands::Release { action, remote } => release::run(action, remote),
            Commands::Ignore { action } => ignore::handle_ignore(action),
            Commands::Tui => tui::run(),
            Commands::Worktree { action } => wrappers::worktree(action),
            Commands::Submodule { action } => wrappers::submodule(action),
            Commands::Subtree { action } => wrappers::subtree(action),
            Commands::Bisect { action } => wrappers::bisect(action),
            Commands::Describe {
                tags,
                long,
                dirty,
                candidates,
            } => wrappers::describe(tags, long, dirty, candidates),
            Commands::Archive {
                revision,
                output,
                format,
                prefix,
            } => wrappers::archive(revision, output, format, prefix),
            Commands::Remove {
                paths,
                cached,
                recursive,
                force,
            } => wrappers::remove(paths, cached, recursive, force),
            Commands::Rename { from, to, force } => wrappers::rename(from, to, force),
            Commands::Grep {
                pattern,
                paths,
                ignore_case,
                word_regexp,
                files_with_matches,
                no_line_number,
            } => wrappers::grep(
                pattern,
                paths,
                ignore_case,
                word_regexp,
                files_with_matches,
                no_line_number,
            ),
            Commands::Notes { action } => wrappers::notes(action),
            Commands::Patch { action } => wrappers::patch(action),
            Commands::Clean {
                force,
                dirs,
                include_ignored,
                only_ignored,
            } => wrappers::clean(force, dirs, include_ignored, only_ignored),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut argv = vec!["torii"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv)
    }

    #[test]
    fn save_stage_all_with_message() {
        let cli = parse(&["save", "-am", "feat: x"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Save { all: true, ref message, amend: false, .. }
                if message.as_deref() == Some("feat: x")
        ));
    }

    #[test]
    fn save_requires_message_unless_reset_revert_or_unstage() {
        assert!(parse(&["save"]).is_err());
        assert!(parse(&["save", "--reset", "HEAD~1"]).is_ok());
        assert!(parse(&["save", "--revert", "abc1234"]).is_ok());
        assert!(parse(&["save", "--unstage", "--all"]).is_ok());
    }

    #[test]
    fn save_conflicting_flags_rejected() {
        // --unstage excludes amend/revert/reset; --sign excludes --no-sign
        assert!(parse(&["save", "--unstage", "--amend", "-m", "x"]).is_err());
        assert!(parse(&["save", "-m", "x", "--sign", "--no-sign"]).is_err());
    }

    #[test]
    fn sync_fetch_flag_combinations() {
        assert!(parse(&["sync", "--fetch", "--all"]).is_ok());
        // --all requires --fetch
        assert!(parse(&["sync", "--all"]).is_err());
        // --all conflicts with a positional remote/branch
        assert!(parse(&["sync", "upstream", "--fetch", "--all"]).is_err());
        let cli = parse(&["sync", "main", "--rebase"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Sync { ref branch, rebase: true, merge: false, .. }
                if branch.as_deref() == Some("main")
        ));
    }

    #[test]
    fn status_null_requires_tracked() {
        assert!(parse(&["status", "--tracked", "-z"]).is_ok());
        assert!(parse(&["status", "-z"]).is_err());
    }

    #[test]
    fn branch_create_and_orphan() {
        let cli = parse(&["branch", "feature/auth", "-c"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Branch { ref name, create: true, orphan: false, .. }
                if name.as_deref() == Some("feature/auth")
        ));
        // --orphan is only meaningful with -c, but the cross-check lives in
        // the handler; clap accepts it — just ensure it parses.
        assert!(parse(&["branch", "gh-pages", "-c", "--orphan"]).is_ok());
    }

    #[test]
    fn rm_and_mv_aliases_work() {
        assert!(matches!(
            parse(&["rm", "old.rs"]).unwrap().command,
            Commands::Remove { .. }
        ));
        assert!(matches!(
            parse(&["mv", "a.rs", "b.rs"]).unwrap().command,
            Commands::Rename { .. }
        ));
        assert!(matches!(
            parse(&["remove", "old.rs"]).unwrap().command,
            Commands::Remove { .. }
        ));
        assert!(matches!(
            parse(&["rename", "a.rs", "b.rs"]).unwrap().command,
            Commands::Rename { .. }
        ));
    }

    #[test]
    fn log_count_and_filters() {
        let cli = parse(&[
            "log",
            "-n",
            "50",
            "--oneline",
            "--graph",
            "--author",
            "Alice",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Log { count: Some(50), oneline: true, graph: true, ref author, .. }
                if author.as_deref() == Some("Alice")
        ));
    }

    #[test]
    fn cherry_pick_continue_and_abort() {
        let cli = parse(&["cherry-pick", "--continue"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::CherryPick {
                r#continue: true,
                ..
            }
        ));
        assert!(parse(&["cherry-pick", "--abort"]).is_ok());
    }

    #[test]
    fn show_lines_requires_blame() {
        assert!(parse(&["show", "src/main.rs", "--blame", "-L", "10,20"]).is_ok());
        assert!(parse(&["show", "src/main.rs", "-L", "10,20"]).is_err());
        // --signature conflicts with --blame
        assert!(parse(&["show", "abc", "--signature", "--blame"]).is_err());
    }

    #[test]
    fn pipeline_remote_flag_is_global() {
        // Global flag works before or after the subcommand verb.
        let a = parse(&["pipeline", "list", "--remote", "github-mirror"]).unwrap();
        let b = parse(&["pipeline", "--remote", "github-mirror", "list"]).unwrap();
        for cli in [a, b] {
            assert!(matches!(
                cli.command,
                Commands::Pipeline { ref remote, .. } if remote == "github-mirror"
            ));
        }
    }

    #[test]
    fn clean_defaults_to_dry_run() {
        let cli = parse(&["clean"]).unwrap();
        assert!(matches!(cli.command, Commands::Clean { force: false, .. }));
    }
}
