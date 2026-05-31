use clap::{Parser, Subcommand};
use anyhow::Result;
use std::path::PathBuf;
use dirs;
use crate::config::ToriiConfig;
use crate::core::GitRepo;
use crate::remote::{get_platform_client, Visibility, RepoSettings, RepoFeatures};
use crate::snapshot::SnapshotManager;
use crate::mirror::{MirrorManager, AccountType, Protocol};
use crate::ssh::SshHelper;
use crate::duration::parse_duration;
use crate::versioning::AutoTagger;
use crate::scanner;
use crate::issue::{get_issue_client, CreateIssueOptions};
use crate::pr::{detect_platform_from_remote, detect_platform_from_remote_named};
use crate::pipeline::{get_pipeline_client, ListFilters, filter_older_than};
use crate::package::{get_package_client, PackageListFilters, filter_older_than as pkg_filter_older_than, filter_by_version as pkg_filter_by_version};
use crate::release::get_release_client;

/// Template `policies/commits.toml` written by `torii init`. Conservative
/// defaults so a fresh repo doesn't fail every save out of the box — users
/// uncomment / extend rules they want enforced.
const DEFAULT_COMMITS_POLICY: &str = r#"# torii commit policy — written by `torii init`.
# Edit / extend; run `torii scan --commits` to evaluate.
# Docs: https://gitorii.com/docs/policies/commits

# Block AI-tooling co-author trailers from leaking into history.
forbid_trailers = [
    "Co-Authored-By:.*Claude",
    "Co-Authored-By:.*Copilot",
    "Co-Authored-By:.*GPT",
]

# Reject lazy / temp subjects.
forbid_subjects = ["^(wip|tmp|temp|misc|asdf|update|fix)$"]

# Subject sanity.
subject_min_length = 8
subject_max_length = 72

# Conventional Commits — uncomment to enforce.
# require_conventional = true

# Pin commits to your domain (uncomment + adjust):
# author_email_matches = ".*@example\\.com$"

# DCO sign-off (uncomment to require):
# require_trailers = ["Signed-off-by:"]
"#;

/// True when the string looks like something `git clone` would accept as
/// a URL or local path, distinguishing it from a platform shorthand
/// (`github`, `gitlab`, …) used in `torii clone <plat> <user/repo>`.
///
/// Accepted shapes:
///   http://… https://… git://… ssh://… ftp(s)://… file://…
///   git@host:owner/repo.git           (scp-like SSH)
///   user@host:owner/repo.git          (any scp-like)
///   /absolute/path/to/repo            (Unix abs)
///   ./relative/path  ../sibling       (relative explicit)
///   C:\… or C:/…                      (Windows abs)
fn looks_like_clone_url(s: &str) -> bool {
    // Explicit scheme — anything before `://` and at least one alphanum.
    if let Some(idx) = s.find("://") {
        if idx > 0 && s[..idx].chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return true;
        }
    }
    // Local paths.
    if s.starts_with('/') || s.starts_with("./") || s.starts_with("../") {
        return true;
    }
    // Windows drive (C:\ or C:/).
    let bytes = s.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
    {
        return true;
    }
    // scp-like: <user>@<host>:<path>. Requires '@' before ':' with both
    // sides non-empty. Excludes IPv6-ish patterns.
    if let Some(at) = s.find('@') {
        if at > 0 {
            if let Some(colon) = s[at + 1..].find(':') {
                let host = &s[at + 1..at + 1 + colon];
                let path = &s[at + 1 + colon + 1..];
                if !host.is_empty() && !path.is_empty()
                    && !host.contains('/') && !host.contains('\\')
                {
                    return true;
                }
            }
        }
    }
    false
}

fn parse_account_type(s: &str) -> Result<AccountType> {
    match s.to_lowercase().as_str() {
        "user" | "u" => Ok(AccountType::User),
        "org" | "organization" | "o" => Ok(AccountType::Organization),
        _ => Err(anyhow::anyhow!("Invalid account type. Use 'user' or 'org'")),
    }
}

fn parse_protocol(s: Option<&String>) -> Protocol {
    match s.map(|s| s.to_lowercase()) {
        Some(p) if p == "https" || p == "http" => Protocol::HTTPS,
        Some(p) if p == "ssh" => Protocol::SSH,
        None => {
            // Auto-detect: use SSH if keys available, otherwise HTTPS
            if SshHelper::has_ssh_keys() {
                Protocol::SSH
            } else {
                println!("⚠️  No SSH keys detected. Using HTTPS protocol.");
                println!("   Run 'torii config check-ssh' for SSH setup instructions.\n");
                Protocol::HTTPS
            }
        }
        _ => Protocol::SSH,
    }
}

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
    #[command(name = "cherry-pick", after_help = "Examples:
  torii cherry-pick abc1234           Apply a commit
  torii cherry-pick --continue        Resume after resolving conflicts
  torii cherry-pick --abort           Abort an in-progress cherry-pick")]
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
    #[command(alias = "rm", after_help = "Examples:
  torii remove src/old.rs                 Remove + untrack
  torii remove src/old.rs --cached        Untrack only (keep on disk)
  torii remove -r vendor/legacy/          Recursive
  torii remove --force src/dirty.rs       Drop local changes

`torii rm` works too — alias kept for users coming from git.")]
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
    #[command(alias = "mv", after_help = "Examples:
  torii rename old.rs new.rs              Stage a rename
  torii rename src/a.rs src/b.rs --force  Overwrite if target exists

`torii mv` works too — alias kept for users coming from git.")]
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

#[derive(Subcommand)]
enum BisectCommands {
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
enum NotesCommands {
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
enum PatchCommands {
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
enum SubmoduleCommands {
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
enum SubtreeCommands {
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
enum WorktreeCommands {
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

#[derive(Subcommand)]
enum IgnoreCommands {
    /// Add a path pattern to .toriignore (or .toriignore.local with --local)
    Add {
        /// Glob/path pattern (e.g. `build/`, `*.log`, `/internal/`)
        pattern: String,
        /// Write to .toriignore.local instead of .toriignore (private, not committed)
        #[arg(long)]
        local: bool,
    },
    /// Add a secret regex rule. Defaults to .toriignore.local (private).
    /// Pass --public to put the rule in the committed .toriignore instead.
    Secret {
        /// Regex pattern matching the secret
        pattern: String,
        /// Optional human name shown when the rule fires
        #[arg(long)]
        name: Option<String>,
        /// Write to public .toriignore instead of .toriignore.local
        #[arg(long)]
        public: bool,
    },
    /// List effective rules (public + local merged)
    List,
}

#[derive(Subcommand)]
enum PrCommands {
    /// List pull requests
    List {
        /// State: open, closed, merged, all (default: open)
        #[arg(long, default_value = "open")]
        state: String,
    },
    /// Create a pull request
    Create {
        /// PR title
        #[arg(short, long)]
        title: String,
        /// Base branch (default: main)
        #[arg(short, long, default_value = "main")]
        base: String,
        /// Head branch (default: current branch)
        #[arg(long)]
        head: Option<String>,
        /// PR description
        #[arg(short, long)]
        description: Option<String>,
        /// Mark as draft
        #[arg(long)]
        draft: bool,
    },
    /// Merge a pull request
    Merge {
        /// PR number
        number: u64,
        /// Merge method: merge, squash, rebase (default: merge)
        #[arg(long, default_value = "merge")]
        method: String,
    },
    /// Close a pull request
    Close {
        /// PR number
        number: u64,
    },
    /// Checkout the branch of a pull request
    Checkout {
        /// PR number
        number: u64,
    },
    /// Open a pull request in the browser
    Open {
        /// PR number
        number: u64,
    },
}

#[derive(Subcommand)]
enum IssueCommands {
    /// List issues
    List {
        #[arg(long, default_value = "open")]
        state: String,
    },
    /// Create an issue
    Create {
        #[arg(short, long)]
        title: String,
        #[arg(short = 'd', long)]
        description: Option<String>,
    },
    /// Close an issue
    Close {
        number: u64,
    },
    /// Add a comment to an issue
    Comment {
        number: u64,
        #[arg(short, long)]
        message: String,
    },
}

#[derive(Subcommand)]
enum PipelineCommands {
    /// List recent pipelines on the auto-detected platform
    List {
        /// Filter by normalized status: success|failed|running|canceled|pending
        #[arg(long)]
        status: Option<String>,
        /// Max entries to return (clamped to [1, 100]).
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Cancel a running pipeline by id
    Cancel { id: String },
    /// Retry a pipeline (re-run failed/canceled jobs) by id
    Retry { id: String },
    /// Delete one pipeline (`<id>`) or many (use `--status` / `--older-than`).
    /// Requires either an explicit id or at least one filter — never both.
    Delete {
        /// Explicit pipeline id. Mutually exclusive with the filter flags.
        id: Option<String>,
        /// Delete every pipeline matching this normalized status. Required
        /// (alongside or instead of `--older-than`) when no id is given.
        #[arg(long, conflicts_with = "id")]
        status: Option<String>,
        /// Delete only pipelines older than this duration (e.g. `7d`,
        /// `12h`, `30m`). Combine with `--status` to narrow further.
        #[arg(long, conflicts_with = "id")]
        older_than: Option<String>,
        /// Skip the confirmation prompt. Required for non-interactive use.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum JobCommands {
    /// List jobs in a pipeline / workflow run
    List {
        /// Pipeline (GitLab) / workflow-run (GitHub) id to list jobs from.
        #[arg(long)]
        pipeline: String,
        /// Optional status filter: success|failed|running|canceled|pending|other
        #[arg(long)]
        status: Option<String>,
    },
    /// Print a job's log/trace. The killer feature — replaces
    /// "open browser, find job, click logs, scroll".
    Log {
        id: String,
        /// Show only the last N lines of the log (good for failure
        /// post-mortems, since the actual error usually lives at the tail).
        #[arg(long)]
        tail: Option<usize>,
    },
    /// Re-run a single job. GitLab only — GitHub Actions doesn't support
    /// per-job retry; use `torii pipeline retry <run-id>` there.
    Retry { id: String },
    /// Cancel a running job. GitLab only — GitHub Actions cancels at the
    /// workflow-run level; use `torii pipeline cancel <run-id>`.
    Cancel { id: String },
    /// Download a job's artifacts archive to a local path. GitLab only —
    /// GitHub artifacts are scoped to the workflow run, not the job.
    Artifacts {
        id: String,
        /// Output path for the artifacts zip. Defaults to `./<job-id>-artifacts.zip`.
        #[arg(short = 'o', long)]
        output: Option<String>,
    },
    /// Erase a job's log + artifacts but keep the job entry in the UI.
    /// GitLab only.
    Erase {
        id: String,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum RunnerCommands {
    /// List the project's runners
    List,
    /// Show one runner's details
    Show { id: String },
    /// Delete a runner (the agent on the host still needs uninstalling)
    Remove {
        id: String,
        /// Skip confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Reset the runner's authentication token (GitLab only). Prints
    /// the new token — paste it into the runner's `config.toml`.
    ResetToken { id: String },
    /// Pause a runner (GitLab only). The runner stays connected but
    /// stops picking up jobs until you `resume` it.
    Pause { id: String },
    /// Resume a previously paused runner (GitLab only).
    Resume { id: String },

    /// Register a self-hosted runner against the active platform.
    ///
    /// Fetches a registration token from the platform's API and wraps
    /// the platform's native register CLI:
    ///   - GitLab: `gitlab-runner register` (binary must be on PATH).
    ///   - GitHub: `./config.sh` inside the runner directory (use
    ///     `--runner-dir` to point at the unpacked actions-runner).
    ///
    /// The actual agent install (downloading the binary, setting up
    /// the systemd service, etc.) is platform/distro-specific and is
    /// NOT done by torii — install the runner first via your package
    /// manager or the platform's docs, then run this to register it.
    Register {
        /// Human-readable description for the runner (GitLab) / name (GitHub).
        #[arg(long)]
        description: Option<String>,
        /// Comma-separated tag list (GitLab) / labels (GitHub).
        #[arg(long)]
        tags: Option<String>,
        /// Executor for GitLab runners: shell, docker, kubernetes, …
        /// Defaults to `shell`. Ignored on GitHub (Actions runners use
        /// a single execution model).
        #[arg(long, default_value = "shell")]
        executor: String,
        /// Docker image when `--executor docker` is used.
        #[arg(long, default_value = "alpine:latest")]
        docker_image: String,
        /// For GitHub: directory where `./config.sh` lives (the
        /// unpacked `actions-runner-*` tarball). Defaults to
        /// `./actions-runner` in the current dir.
        #[arg(long)]
        runner_dir: Option<String>,
        /// Skip the confirmation prompt that shows the resolved
        /// command before running it. Useful for scripts.
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Scaffold a runner config (`gitlab-runner` only for now).
    /// Writes a starter `~/.gitlab-runner/config.toml` if absent so
    /// `torii runner register` has a place to land its block. Does
    /// NOT install the binary.
    Init,

    /// Spawn a Dockerized runner against the current platform.
    ///
    /// Pulls the upstream image, runs it as a detached container
    /// with the Docker socket mounted (so the runner can launch its
    /// own job containers), then runs `gitlab-runner register`
    /// inside to attach it to the project.
    ///
    /// Container is named `torii-runner-<slug>` so the rest of
    /// `torii runner status / start / stop / logs / destroy` can
    /// list and drive it without any local state file. GitLab only
    /// for now — GitHub Actions self-hosted runners use a different
    /// container shape (ephemeral tokens, JIT config) we haven't
    /// wired yet.
    #[command(after_help = "Examples:
  torii runner spawn                                  GitLab project, docker executor
  torii runner spawn --name void-torii                Custom container name suffix
  torii runner spawn --executor shell                 Use the shell executor inside the container
  torii runner spawn --image rust:1.94                Run jobs inside this Docker image
  torii runner spawn --tags torii,docker              Tag list passed to register
  torii runner spawn --remote github-paskidev         (rejected — GitHub spawn not implemented)")]
    Spawn {
        /// Human-readable suffix appended to the container name. Final
        /// name is `torii-runner-<name>`. Defaults to a slug derived
        /// from the owner/repo.
        #[arg(long)]
        name: Option<String>,
        /// Image used for the runner container itself. Defaults to
        /// the upstream `gitlab/gitlab-runner:latest`.
        #[arg(long, default_value = "gitlab/gitlab-runner:latest")]
        runner_image: String,
        /// Executor passed to `gitlab-runner register`.
        #[arg(long, default_value = "docker")]
        executor: String,
        /// Image used for the jobs the runner picks up (only meaningful
        /// when `--executor docker`).
        #[arg(long, default_value = "alpine:latest")]
        image: String,
        /// Tag list for the runner. Comma-separated.
        #[arg(long)]
        tags: Option<String>,
        /// Description shown in the platform's runner list.
        #[arg(long)]
        description: Option<String>,
        /// Skip confirmation prompt for the resolved docker commands.
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// List dockerized runners managed by torii (`torii-runner-*`
    /// containers) and their status.
    Status,

    /// Start a stopped torii-managed runner container.
    Start { name: String },

    /// Stop a running torii-managed runner container.
    Stop { name: String },

    /// Tail the logs of a torii-managed runner container.
    Logs {
        name: String,
        /// Follow the log stream (`docker logs -f`). Press Ctrl-C to
        /// detach; the container keeps running.
        #[arg(short = 'f', long)]
        follow: bool,
        /// Show only the last N lines of historical logs.
        #[arg(long)]
        tail: Option<usize>,
    },

    /// Remove a torii-managed runner container completely. Stops it
    /// first if needed. Doesn't touch the platform-side runner
    /// registration — use `torii runner remove <id>` for that.
    Destroy {
        name: String,
        /// Skip confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum PackageCommands {
    /// List packages in the project registry
    List {
        /// Filter by package type (e.g. "generic")
        #[arg(long = "type")]
        package_type: Option<String>,
        /// Substring search on package name
        #[arg(long)]
        name: Option<String>,
        /// Max entries to return (1..=100)
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List files inside a package
    Files { id: String },
    /// Delete one package (`<id>`) or many (use `--version` / `--older-than`).
    Delete {
        /// Explicit package id. Mutually exclusive with the filter flags.
        id: Option<String>,
        /// Delete every package matching this version
        #[arg(long, conflicts_with = "id")]
        version: Option<String>,
        /// Delete only packages older than this duration (e.g. `90d`, `7d`)
        #[arg(long, conflicts_with = "id")]
        older_than: Option<String>,
        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum ReleaseCommands {
    /// List recent releases
    List {
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Show one release's full details (description, web URL, etc.)
    Show { tag: String },
    /// Edit release metadata. Pass `--name` and/or `--notes`.
    Edit {
        tag: String,
        /// New release name/title.
        #[arg(long)]
        name: Option<String>,
        /// Path to a markdown file with the new description. Use `-` for stdin.
        #[arg(long)]
        notes: Option<String>,
    },
    /// Delete the release entity (leaves the tag intact).
    Delete {
        tag: String,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum WorkspaceCommands {
    /// Add a repository to a workspace
    Add {
        /// Workspace name
        workspace: String,
        /// Repository path
        path: String,
    },
    /// Remove a repository from a workspace
    Remove {
        /// Workspace name
        workspace: String,
        /// Repository path
        path: String,
    },
    /// Delete a workspace entirely
    Delete {
        /// Workspace name
        workspace: String,
    },
    /// List all workspaces and their repos
    List,
    /// Show git status across all repos in a workspace
    Status {
        /// Workspace name
        workspace: String,
    },
    /// Commit changes across all repos in a workspace
    Save {
        /// Workspace name
        workspace: String,
        /// Commit message
        #[arg(short, long)]
        message: String,
        /// Stage all changes before committing
        #[arg(short, long)]
        all: bool,
    },
    /// Pull and push all repos in a workspace
    Sync {
        /// Workspace name
        workspace: String,
        /// Force push
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Save a gitorii.com API key locally and validate it against the backend.
    Login {
        /// API key (gitorii_sk_…). If omitted, prompts on stdin.
        #[arg(long)]
        key: Option<String>,
        /// Custom API endpoint (default: https://api.gitorii.com).
        /// Useful for self-hosted / local dev.
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Show the org / plan / seats tied to the active gitorii.com key.
    Status,
    /// Alias of `status`.
    Whoami,
    /// Delete the local gitorii.com API key (env var TORII_API_KEY still wins if set).
    Logout,

    /// Save a platform token (github, gitlab, gitea, forgejo, codeberg,
    /// bitbucket, sourcehut, cargo). Goes into `~/.config/torii/auth.toml`
    /// (chmod 600); use `--local` for per-repo `.torii/auth.toml`.
    Set {
        /// Provider name. One of: github, gitlab, gitea, forgejo,
        /// codeberg, bitbucket, sourcehut, cargo.
        provider: String,
        /// Token value. Use `-` to read from stdin (recommended for CI
        /// so the token never lands in shell history).
        token: String,
        /// Save in the per-repo store instead of the global one.
        #[arg(long)]
        local: bool,
        /// Record an expiration: `30d`, `2h`, `7d12h`, etc. Stored in
        /// `~/.config/torii/auth.toml [token_expires]`; `torii auth
        /// doctor` warns when it's close. Pure metadata — torii does
        /// not auto-rotate.
        #[arg(long)]
        ttl: Option<String>,
    },

    /// Print a stored token, value masked (`ghp_xxxx****`).
    Get {
        provider: String,
        /// Show the raw token (you very rarely want this — it goes
        /// straight to stdout / shell history). Off by default.
        #[arg(long)]
        unsafe_show: bool,
    },

    /// Show every provider's token state (set / not set / from env)
    /// with masked values.
    List,

    /// Delete a stored token.
    Remove {
        provider: String,
        /// Remove from per-repo store instead of global.
        #[arg(long)]
        local: bool,
    },

    /// Diagnose where each provider's token comes from (env var name,
    /// local config, global config, or missing). Use this when "torii
    /// auth doesn't seem to use my token".
    Doctor,

    /// Run an OAuth device-flow login against a platform and save the
    /// resulting access token to `~/.config/torii/auth.toml`.
    ///
    /// Avoids having to create a Personal Access Token in the
    /// platform's web UI: you authorise torii in your browser, torii
    /// receives the token, done.
    ///
    /// Supported (0.7.20): github, gitlab, codeberg. Bitbucket and
    /// Azure DevOps use Authorization Code flow with a localhost
    /// callback — wired in a future release; for now `torii auth set
    /// bitbucket USERNAME:APP_PASSWORD` is still the path there.
    Oauth {
        /// Provider name. One of: github, gitlab, codeberg.
        provider: String,
        /// Save in the per-repo store instead of the global one.
        #[arg(long)]
        local: bool,
        /// Record an expiration: `30d`, `2h`, `7d12h`, etc. See
        /// `auth set --ttl`.
        #[arg(long)]
        ttl: Option<String>,
    },

    /// Rotate a stored token: obtain a fresh one, replace the saved
    /// value, and best-effort revoke the old one so a leaked copy
    /// stops working immediately.
    ///
    /// Default flow (OAuth): re-runs the device flow, swaps in the new
    /// access token, then POSTs to the platform's revoke endpoint. The
    /// old token stops working as soon as the revoke succeeds.
    ///
    /// `--pat` (GitLab only): uses the native
    /// `POST /personal_access_tokens/self/rotate` endpoint, which
    /// generates a new PAT with the same scopes and invalidates the
    /// old one atomically — no OAuth round-trip, no browser. Requires
    /// the current token to have the `api` scope.
    Rotate {
        /// Provider name. OAuth path: github, gitlab, codeberg.
        /// PAT path (`--pat`): gitlab.
        provider: String,
        /// Rotate the PAT in place via the platform's native rotate
        /// endpoint instead of running OAuth. GitLab only.
        #[arg(long)]
        pat: bool,
        /// Save in the per-repo store instead of the global one.
        #[arg(long)]
        local: bool,
        /// Record an expiration for the new token: `30d`, `2h`, etc.
        #[arg(long)]
        ttl: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Set a configuration value
    Set {
        /// Configuration key (e.g., user.name, snapshot.auto_enabled)
        key: String,
        
        /// Configuration value
        value: String,
        
        /// Set in local repository config instead of global
        #[arg(long)]
        local: bool,
    },
    
    /// Get a configuration value
    Get {
        /// Configuration key (e.g., user.name, snapshot.auto_enabled)
        key: String,
        
        /// Get from local repository config
        #[arg(long)]
        local: bool,
    },
    
    /// List all configuration values
    List {
        /// Show local repository config
        #[arg(long)]
        local: bool,
    },
    
    /// Edit configuration file in editor
    Edit {
        /// Edit local repository config instead of global
        #[arg(long)]
        local: bool,
    },
    
    /// Reset configuration to defaults
    Reset {
        /// Reset local repository config instead of global
        #[arg(long)]
        local: bool,
    },

    /// Check SSH configuration and show setup instructions
    #[command(name = "check-ssh")]
    CheckSsh,
}

#[derive(Subcommand)]
enum RemoteCommands {
    /// Create a new remote repository on one or more platforms
    #[command(after_help = "Examples:
  torii remote create github myrepo                       User repo (your account)
  torii remote create github acme/widget                  Org repo: acme/widget
  torii remote create gitlab syrakon/svitrio-turso        GitLab group repo
  torii remote create gitlab engineering/web/api          GitLab subgroup repo
  torii remote create github,gitlab acme/myrepo --push    Same owner on both
  torii remote create github acme/myrepo --private --push

`<NAME>` accepts either `repo` (creates in your personal namespace) or
`owner/repo` (creates in the named org / group / subgroup). The
`--namespace <owner>` flag is the equivalent if you prefer keeping
NAME bare.")]
    Create {
        /// Platform (or comma-separated list): github, gitlab, codeberg, bitbucket, gitea, forgejo
        #[arg(value_delimiter = ',')]
        platforms: String,
        /// Repository name. Supports `repo` (personal) or `owner/repo`
        /// (organization / GitLab group / subgroup path). Slashes select
        /// the namespace.
        name: String,
        #[arg(short, long)]
        description: Option<String>,
        #[arg(long)]
        public: bool,
        #[arg(long)]
        private: bool,
        #[arg(long)]
        push: bool,
        /// Override namespace explicitly. Equivalent to passing
        /// `<namespace>/<name>` as NAME. Useful when the repo name itself
        /// contains a slash you don't want parsed as a namespace.
        #[arg(long, value_name = "OWNER")]
        namespace: Option<String>,
    },
    /// Delete a remote repository on one or more platforms
    Delete {
        /// Platform (or comma-separated list)
        platforms: String,
        owner: String,
        repo: String,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    Visibility {
        platform: String,
        owner: String,
        repo: String,
        #[arg(long, conflicts_with = "private")]
        public: bool,
        #[arg(long, conflicts_with = "public")]
        private: bool,
    },
    Configure {
        platform: String,
        owner: String,
        repo: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        homepage: Option<String>,
        #[arg(long)]
        default_branch: Option<String>,
        #[arg(long)]
        enable_issues: bool,
        #[arg(long, conflicts_with = "enable_issues")]
        disable_issues: bool,
        #[arg(long)]
        enable_wiki: bool,
        #[arg(long, conflicts_with = "enable_wiki")]
        disable_wiki: bool,
        #[arg(long)]
        enable_projects: bool,
        #[arg(long, conflicts_with = "enable_projects")]
        disable_projects: bool,
    },
    Info {
        platform: String,
        owner: String,
        repo: String,
    },
    List {
        platform: String,
    },
    /// List remotes configured in the current repository
    Local,

    /// Link an existing remote repo to local (writes origin without touching the platform)
    #[command(after_help = "Examples:
  torii remote link github user/repo            Link via SSH (default)
  torii remote link gitlab user/repo --https    Link via HTTPS
  torii remote link --url git@host:owner/repo.git
  torii remote link my-fork github user/repo    Use a remote name other than 'origin'")]
    Link {
        /// Optional remote name (default: origin)
        #[arg(long, default_value = "origin")]
        name: String,

        /// Platform shortcut: github, gitlab, codeberg, bitbucket, gitea, forgejo, sourcehut
        platform: Option<String>,

        /// owner/repo on the platform
        repo: Option<String>,

        /// Use HTTPS instead of SSH
        #[arg(long)]
        https: bool,

        /// Provide a full URL directly (bypasses platform/repo)
        #[arg(long, value_name = "URL")]
        url: Option<String>,

        /// Replace existing remote with the same name
        #[arg(long)]
        force: bool,
    },

    /// Remove a local remote alias from .git/config — does NOT touch the
    /// platform. Inverse of `link`.
    #[command(after_help = "Examples:
  torii remote unlink origin           Drop the default origin alias
  torii remote unlink upstream         Drop a custom-named remote
  torii remote unlink old --yes        Skip confirmation prompt")]
    Unlink {
        /// Name of the local remote alias to remove (e.g. origin, upstream)
        name: String,

        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// List refs the remote currently advertises (≡ `git ls-remote`).
    /// Hits the network — uses your configured auth.
    #[command(after_help = "Examples:
  torii remote refs origin              List all refs on origin
  torii remote refs origin --heads      Branch heads only
  torii remote refs origin --tags       Tags only
  torii remote refs https://...         Ad-hoc URL (no need to add as remote first)")]
    Refs {
        /// Local remote alias OR a full URL.
        target: String,
        /// Only print branch heads (`refs/heads/*`).
        #[arg(long)]
        heads: bool,
        /// Only print tag refs (`refs/tags/*`).
        #[arg(long)]
        tags: bool,
    },
}

#[derive(Subcommand)]
enum HistoryCommands {
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
    #[command(alias = "fsck", after_help = "Examples:
  torii history orphans                              List unreachable objects
  torii history orphans --show abc1234               Print object content (commit/blob)
  torii history orphans --restore abc1234 --to f.txt Recover a blob to disk

`torii history fsck` works too — alias kept for users coming from git.")]
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
enum MailmapCommands {
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


#[derive(Subcommand)]
enum SnapshotCommands {
    /// Create a new snapshot
    Create {
        /// Optional snapshot name/description
        #[arg(short, long)]
        name: Option<String>,
    },

    /// List all snapshots
    List,

    /// Restore from a snapshot
    Restore {
        /// Snapshot ID to restore
        id: String,
    },

    /// Delete a snapshot
    Delete {
        /// Snapshot ID to delete
        id: String,
    },

    /// Auto-snapshot configuration
    Config {
        /// Enable auto-snapshots
        #[arg(long)]
        enable: bool,

        /// Snapshot interval (e.g., 1h, 30m)
        #[arg(long)]
        interval: Option<String>,
    },

    /// Save work temporarily (like git stash)
    Stash {
        /// Name for the stash
        #[arg(short, long)]
        name: Option<String>,

        /// Include untracked files
        #[arg(short = 'u', long)]
        include_untracked: bool,
    },

    /// Restore stashed work
    Unstash {
        /// Stash ID to restore (latest if not specified)
        id: Option<String>,

        /// Keep the stash after restoring
        #[arg(short, long)]
        keep: bool,
    },

    /// `git stash apply` alias — restore without dropping the stash.
    /// Equivalent to `torii snapshot unstash --keep [<id>]`.
    Apply {
        /// Snapshot/stash ID (latest if not specified).
        id: Option<String>,
    },

    /// `git stash pop` alias — restore and drop the stash.
    /// Equivalent to `torii snapshot unstash [<id>]`.
    Pop {
        /// Snapshot/stash ID (latest if not specified).
        id: Option<String>,
    },

    /// `git stash drop` alias — delete a specific snapshot.
    /// Equivalent to `torii snapshot delete <id>`.
    Drop {
        /// Snapshot/stash ID to drop.
        id: String,
    },

    /// Delete every snapshot/stash in this repo. Asks for confirmation
    /// unless `--yes` is given.
    Clear {
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show what's inside a snapshot — branch, commit, timestamp,
    /// and a list of files captured.
    Show {
        /// Snapshot/stash ID.
        id: String,
    },

    /// Undo last operation
    Undo,
}

#[derive(Debug, Subcommand)]
enum TagCommands {
    /// Create a new tag (or auto-bump the next release tag with --release)
    Create {
        /// Tag name (omit when using --release)
        name: Option<String>,

        /// Tag message (creates annotated tag)
        #[arg(short, long)]
        message: Option<String>,

        /// Auto-bump the next version from conventional commits since last tag
        #[arg(long)]
        release: bool,

        /// Force a specific bump (used with --release): major, minor, patch
        #[arg(long, requires = "release")]
        bump: Option<String>,

        /// Preview the next version without creating the tag (used with --release)
        #[arg(long, requires = "release")]
        dry_run: bool,
    },

    /// List all tags
    List,

    /// Delete a tag
    Delete {
        /// Tag name to delete
        name: String,
    },

    /// Push tags to remote
    Push {
        /// Specific tag to push (all if not specified)
        name: Option<String>,

        /// Force-push tags even when the remote ref already exists at a
        /// different commit (rewrites remote tag history).
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// Show tag details
    Show {
        /// Tag name
        name: String,
    },
}

#[derive(Subcommand)]
enum MirrorCommands {
    /// Add a mirror (replica by default; use --primary for the source of truth)
    Add {
        /// Platform (github, gitlab, bitbucket, codeberg)
        platform: String,

        /// Account type (user or org)
        account_type: String,

        /// Account name (username or organization)
        account: String,

        /// Repository name
        repo: String,

        /// Mark this mirror as the primary (source of truth). Default: replica.
        #[arg(long)]
        primary: bool,

        /// Protocol (ssh or https, defaults to ssh)
        #[arg(short, long)]
        protocol: Option<String>,
    },

    /// List all mirrors
    List,

    /// Sync to all replica mirrors
    Sync {
        /// Force sync
        #[arg(short, long)]
        force: bool,
    },

    /// Promote a mirror to primary (source of truth)
    Promote {
        /// Platform
        platform: String,

        /// Account name
        account: String,
    },

    /// Remove a mirror
    Remove {
        /// Platform
        platform: String,

        /// Account name
        account: String,
    },

    /// Configure autofetch (automatic fetch from mirrors)
    Autofetch {
        /// Enable autofetch
        #[arg(long)]
        enable: bool,

        /// Disable autofetch
        #[arg(long, conflicts_with = "enable")]
        disable: bool,

        /// Fetch interval (e.g., 10m, 30s, 2h, 1d)
        #[arg(long)]
        interval: Option<String>,

        /// Show current autofetch status
        #[arg(long, conflicts_with_all = ["enable", "disable", "interval"])]
        status: bool,
    },
}

impl Cli {
    pub fn execute(&self) -> Result<()> {
        match &self.command {
            Commands::Init { path } => {
                let repo_path = path.as_deref().unwrap_or(".");
                GitRepo::init(repo_path)?;

                // Create .toriignore with sensible defaults
                let toriignore_path = std::path::Path::new(repo_path).join(".toriignore");
                if !toriignore_path.exists() {
                    std::fs::write(&toriignore_path, crate::toriignore::ToriIgnore::default_content())
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
            }

            Commands::Save { message, all, files, amend, revert, reset, reset_mode, unstage, skip_hooks, sign, no_sign } => {
                let repo = GitRepo::open(".")?;

                // 0.7.35 — translate `-S` / `--no-sign` into the
                // env-var that `commit_inner_split` reads. The guard
                // restores the previous value on drop so we don't leak
                // the override into any subprocess invoked later in
                // the same process.
                let _sign_guard = SignOverrideGuard::new(if *sign { Some(true) } else if *no_sign { Some(false) } else { None });

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

                    let msg = message.as_deref().ok_or_else(|| anyhow::anyhow!(
                        "--message/-m is required for commit/amend"
                    ))?;
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
            }

            Commands::Sync { branch, pull, push, force, fetch, all, merge, rebase, preview, verify, skip_hooks } => {
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
            }

            Commands::Status { tracked, null } => {
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
                    repo.status()?;
                }
            }

            Commands::Log { count, oneline, graph, author, since, until, grep, stat, reflog, signatures } => {
                let repo = GitRepo::open(".")?;
                if *reflog {
                    repo.show_reflog(count.unwrap_or(20))?;
                } else {
                    repo.log(*count, *oneline, *graph, author.as_deref(), since.as_deref(), until.as_deref(), grep.as_deref(), *stat, *signatures)?;
                }
            }

            Commands::Diff { staged, last } => {
                let repo = GitRepo::open(".")?;
                repo.diff(*staged, *last)?;
            }

            Commands::Blame { file, lines } => {
                eprintln!(
                    "⚠  'torii blame' is deprecated and will be removed in 0.8.\n   \
                     Use 'torii show {} --blame' instead.",
                    file
                );
                let repo = GitRepo::open(".")?;
                repo.blame(file, lines.as_deref())?;
            }

            Commands::Scan { history, commits, policy_file, limit } => {
                if *commits {
                    run_commit_scan(policy_file.as_deref(), *limit)?;
                } else {
                    run_scan(*history)?;
                }
            }

            Commands::CherryPick { commit, r#continue, abort } => {
                let repo = GitRepo::open(".")?;
                if *r#continue {
                    repo.cherry_pick_continue()?;
                } else if *abort {
                    repo.cherry_pick_abort()?;
                } else {
                    let hash = commit.as_deref().ok_or_else(|| anyhow::anyhow!("Commit hash required: torii cherry-pick <hash>"))?;
                    repo.cherry_pick(hash)?;
                }
            }

            Commands::Branch { name, create, orphan, delete, force, delete_remote, list, rename, all } => {
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
                            Ok(o) => errors.push(format!("{}: {}", remote_name, String::from_utf8_lossy(&o.stderr).trim().to_string())),
                            Err(e) => errors.push(format!("{}: {}", remote_name, e)),
                        }
                    }
                    if !deleted.is_empty() {
                        println!("✅ Deleted '{}' on: {}", branch_name, deleted.join(", "));
                    }
                    if !errors.is_empty() {
                        for e in &errors { eprintln!("⚠️  {}", e); }
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
                        println!("✅ Created orphan branch: {} (no parents — first commit will be a new root)", branch_name);
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
            }

            Commands::Clone { source, args, directory, protocol } => {
                // Match git clone's positional shape:
                //   torii clone <platform> <user/repo> [<path>]
                //   torii clone <url> [<path>]
                // The trailing path arg silently used to be ignored, surprising
                // users coming from `git clone <url> <path>`.
                //
                // Disambiguation: if `source` already looks like a URL/path
                // (http(s)://, git://, ssh://, file://, /abs, ./rel,
                // user@host:path), treat the first positional `args[0]` as
                // the destination — NOT as user/repo. Without this guard,
                // `torii clone file:///tmp/foo dest` errored with
                // "Unknown platform 'file:///tmp/foo'".
                let source_is_url = looks_like_clone_url(source);

                let url = if !args.is_empty() && !source_is_url {
                    // Shorthand: torii clone <platform> <user/repo>
                    let platform = source;
                    let user_repo = &args[0];

                    // Protocol priority: --protocol flag > config > auto-detect
                    let use_ssh = match protocol.as_deref() {
                        Some("https") | Some("http") => false,
                        Some("ssh") => true,
                        _ => {
                            let cfg = ToriiConfig::load_global().unwrap_or_default();
                            if cfg.mirror.default_protocol == "https" {
                                false
                            } else {
                                SshHelper::has_ssh_keys()
                            }
                        }
                    };

                    let (ssh_host, https_host) = match platform.as_str() {
                        "github"    => ("github.com", "github.com"),
                        "gitlab"    => ("gitlab.com", "gitlab.com"),
                        "codeberg"  => ("codeberg.org", "codeberg.org"),
                        "bitbucket" => ("bitbucket.org", "bitbucket.org"),
                        "gitea"     => ("gitea.com", "gitea.com"),
                        "forgejo"   => ("codeberg.org", "codeberg.org"),
                        _ => anyhow::bail!(
                            "Unknown platform '{}'. Supported: github, gitlab, codeberg, bitbucket, gitea, forgejo",
                            platform
                        ),
                    };

                    if use_ssh {
                        format!("git@{}:{}.git", ssh_host, user_repo)
                    } else {
                        format!("https://{}/{}.git", https_host, user_repo)
                    }
                } else if looks_like_clone_url(source) {
                    source.clone()
                } else {
                    anyhow::bail!(
                        "Usage:\n  torii clone <platform> <user/repo>        e.g. torii clone github user/repo\n  torii clone <platform> <user/repo> --protocol https\n  torii clone <url>                          e.g. torii clone https://github.com/user/repo.git\n  torii clone <local-path-or-file:///url>    e.g. torii clone /tmp/source.git"
                    )
                };

                // Resolve destination. Precedence:
                //   1. -d / --directory flag
                //   2. trailing positional arg (git-style):
                //        torii clone <plat> <user/repo> <path>   → args[1]
                //        torii clone <url> <path>                → args[0]
                //   3. derive from URL (default)
                let positional_dest: Option<&str> = if source_is_url {
                    // URL form: first positional after the URL is the dest.
                    args.first().map(|s| s.as_str())
                } else if !args.is_empty() {
                    // Shorthand: args[0] is user/repo, args[1] is dest.
                    args.get(1).map(|s| s.as_str())
                } else {
                    None
                };
                let target_dir = directory.as_deref().or(positional_dest);
                GitRepo::clone_repo(&url, target_dir)?;

                let dir_name = target_dir.unwrap_or_else(|| {
                    url.split('/').last().unwrap_or("repo").trim_end_matches(".git")
                });
                println!("✅ Cloned repository to: {}", dir_name);
            }

            Commands::Tag { action } => {
                let repo = GitRepo::open(".")?;
                match action {
                    TagCommands::Create { name, message, release, bump, dry_run } => {
                        if *release {
                            let tagger = AutoTagger::new(repo);
                            let current = tagger.get_latest_version()?;

                            let next = if let Some(bump_str) = bump {
                                use crate::versioning::semver::VersionBump;
                                let b = match bump_str.as_str() {
                                    "major" => VersionBump::Major,
                                    "minor" => VersionBump::Minor,
                                    "patch" => VersionBump::Patch,
                                    _ => anyhow::bail!("Invalid bump: use major, minor or patch"),
                                };
                                let base = current.clone().unwrap_or_else(crate::versioning::semver::Version::initial);
                                base.bump(b)
                            } else {
                                tagger.calculate_next_version_from_log()?
                                    .ok_or_else(|| anyhow::anyhow!("No releasable commits found since last tag (need feat: or fix:)"))?
                            };

                            println!("📦 Current version: {}", current.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()));
                            println!("🚀 Next version:    v{}", next);

                            if *dry_run {
                                println!("   (dry run — no tag created)");
                            } else {
                                tagger.create_tag(&next, &format!("Release v{}", next))?;
                                println!("💡 Push with: torii sync --push");
                            }
                        } else {
                            let tag_name = name.as_deref().ok_or_else(|| anyhow::anyhow!(
                                "Tag name required (or use --release to auto-bump)"
                            ))?;
                            repo.create_tag(tag_name, message.as_deref())?;
                            println!("✅ Tag created: {}", tag_name);
                        }
                    }
                    TagCommands::List => {
                        repo.list_tags()?;
                    }
                    TagCommands::Delete { name } => {
                        repo.delete_tag(name)?;
                        println!("✅ Tag deleted: {}", name);
                    }
                    TagCommands::Push { name, force } => {
                        repo.push_tags(name.as_deref(), *force)?;
                        let force_note = if *force { " (force)" } else { "" };
                        if let Some(tag) = name {
                            println!("✅ Pushed tag: {}{}", tag, force_note);
                        } else {
                            println!("✅ Pushed all tags{}", force_note);
                        }
                    }
                    TagCommands::Show { name } => {
                        repo.show_tag(name)?;
                    }
                }
            }

            Commands::Snapshot { action } => {
                let snapshot_mgr = SnapshotManager::new(".")?;
                match action {
                    SnapshotCommands::Create { name } => {
                        let snapshot_id = snapshot_mgr.create_snapshot(name.as_deref())?;
                        println!("✅ Snapshot created: {}", snapshot_id);
                    }
                    SnapshotCommands::List => {
                        snapshot_mgr.list_snapshots()?;
                    }
                    SnapshotCommands::Restore { id } => {
                        snapshot_mgr.restore_snapshot(id)?;
                        println!("✅ Restored snapshot: {}", id);
                    }
                    SnapshotCommands::Delete { id } => {
                        snapshot_mgr.delete_snapshot(id)?;
                        println!("✅ Deleted snapshot: {}", id);
                    }
                    SnapshotCommands::Config { enable, interval } => {
                        let interval_minutes = interval.as_ref().and_then(|s| s.parse::<u32>().ok());
                        snapshot_mgr.configure_auto_snapshot(*enable, interval_minutes)?;
                        println!("✅ Auto-snapshot configuration updated");
                    }
                    SnapshotCommands::Stash { name, include_untracked } => {
                        snapshot_mgr.stash(name.as_deref(), *include_untracked)?;
                    }
                    SnapshotCommands::Unstash { id, keep } => {
                        snapshot_mgr.unstash(id.as_deref(), *keep)?;
                    }
                    SnapshotCommands::Apply { id } => {
                        snapshot_mgr.unstash(id.as_deref(), true)?;
                    }
                    SnapshotCommands::Pop { id } => {
                        snapshot_mgr.unstash(id.as_deref(), false)?;
                    }
                    SnapshotCommands::Drop { id } => {
                        snapshot_mgr.delete_snapshot(id)?;
                        println!("✅ Dropped snapshot: {}", id);
                    }
                    SnapshotCommands::Clear { yes } => {
                        if !*yes {
                            use std::io::{self, BufRead, IsTerminal, Write};
                            if !io::stdin().is_terminal() {
                                anyhow::bail!("Refusing to clear without --yes when there's no tty to prompt.");
                            }
                            print!("⚠  Delete ALL snapshots in this repo? [y/N] ");
                            io::stdout().flush().ok();
                            let mut line = String::new();
                            io::stdin().lock().read_line(&mut line)?;
                            if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                                println!("Aborted.");
                                return Ok(());
                            }
                        }
                        let count = snapshot_mgr.clear_all()?;
                        println!("🧹 Cleared {count} snapshot(s).");
                    }
                    SnapshotCommands::Show { id } => {
                        snapshot_mgr.show(id)?;
                    }
                    SnapshotCommands::Undo => {
                        snapshot_mgr.undo()?;
                    }
                }
            }

            Commands::Mirror { action } => {
                let mirror_mgr = MirrorManager::new(".")?;
                match action {
                    MirrorCommands::Add { platform, account_type, account, repo, primary, protocol } => {
                        let acc_type = parse_account_type(account_type)?;
                        let proto = parse_protocol(protocol.as_ref());
                        mirror_mgr.add_mirror(platform, acc_type, account, repo, proto, *primary)?;
                        let kind = if *primary { "Primary" } else { "Replica" };
                        println!("✅ {} mirror added: {}/{} on {}", kind, account, repo, platform);
                    }
                    MirrorCommands::List => {
                        mirror_mgr.list_mirrors()?;
                    }
                    MirrorCommands::Sync { force } => {
                        mirror_mgr.sync_all(*force)?;
                    }
                    MirrorCommands::Promote { platform, account } => {
                        mirror_mgr.set_primary(platform, account)?;
                        println!("✅ Promoted to primary: {}/{}", platform, account);
                    }
                    MirrorCommands::Remove { platform, account } => {
                        mirror_mgr.remove_mirror_by_account(platform, account)?;
                        println!("✅ Mirror removed: {}/{}", platform, account);
                    }
                    MirrorCommands::Autofetch { enable, disable, interval, status } => {
                        if *status {
                            mirror_mgr.show_autofetch_status()?;
                        } else if *enable {
                            let interval_minutes = if let Some(interval_str) = interval {
                                Some(parse_duration(interval_str)?)
                            } else {
                                None
                            };
                            mirror_mgr.configure_autofetch(true, interval_minutes)?;
                        } else if *disable {
                            mirror_mgr.configure_autofetch(false, None)?;
                        } else {
                            mirror_mgr.show_autofetch_status()?;
                        }
                    }
                }
            }

            Commands::Auth { action } => {
                run_auth(action)?;
            }

            Commands::Publish { dry_run, no_verify, token, allow_dirty } => {
                let resolved = match token {
                    Some(t) => t.clone(),
                    None => crate::auth::resolve_token("cargo", ".")
                        .value
                        .ok_or_else(|| anyhow::anyhow!(
                            "No cargo token configured. Set one with:\n  torii auth set cargo <token>\n\
                             …or pass --token <value> just for this invocation."
                        ))?,
                };
                let mut cmd = std::process::Command::new("cargo");
                cmd.arg("publish");
                if *dry_run { cmd.arg("--dry-run"); }
                if *no_verify { cmd.arg("--no-verify"); }
                if *allow_dirty { cmd.arg("--allow-dirty"); }
                // Pass --locked by default — same convention torii uses
                // elsewhere so the verify-build matches the committed lock.
                cmd.arg("--locked");
                cmd.env("CARGO_REGISTRY_TOKEN", &resolved);
                let mode = if *dry_run { "dry-run" } else { "publishing" };
                println!("📦 cargo {} (token injected from torii auth)…", mode);
                let status = cmd.status()?;
                if !status.success() {
                    anyhow::bail!("cargo publish exited with {}", status);
                }
                if !*dry_run {
                    // Read [package].name from Cargo.toml so the URL is
                    // accurate even if the binary name differs from the
                    // crate name (gitorii binary is `torii`).
                    let crate_name = std::fs::read_to_string("Cargo.toml")
                        .ok()
                        .and_then(|s| {
                            s.lines()
                                .find(|l| l.trim_start().starts_with("name "))
                                .and_then(|l| l.split('=').nth(1))
                                .map(|v| v.trim().trim_matches('"').to_string())
                        })
                        .unwrap_or_else(|| "<crate>".to_string());
                    println!(
                        "\n✅ Published. View at https://crates.io/crates/{}",
                        crate_name
                    );
                }
            }

            Commands::Config { action } => {
                match action {
                    ConfigCommands::Set { key, value, local } => {
                        // Auth tokens migrated to `torii auth` in 0.7.1.
                        // Redirect transparently so old scripts keep
                        // working but the user is steered to the new home.
                        if let Some(provider_token) = key.strip_prefix("auth.") {
                            if let Some(provider) = provider_token.strip_suffix("_token") {
                                let repo: Option<&std::path::Path> = if *local {
                                    Some(std::path::Path::new("."))
                                } else {
                                    None
                                };
                                crate::auth::set_token(provider, value, repo)?;
                                eprintln!(
                                    "⚠  `torii config set auth.{p}_token` is deprecated and will be removed in 0.8.\n   \
                                     Saved via the new path: `torii auth set {p} …` (which is what you want next time).",
                                    p = provider
                                );
                                let scope = if *local { "local" } else { "global" };
                                println!("✅ {} token saved ({} store).", provider, scope);
                                return Ok(());
                            }
                        }

                        if *local {
                            let mut config = ToriiConfig::load_local(".")?;
                            config.set(key, value)?;
                            config.save_local(".")?;
                            println!("✅ Local config updated: {} = {}", key, value);
                        } else {
                            let mut config = ToriiConfig::load_global()?;
                            config.set(key, value)?;
                            config.save_global()?;
                            println!("✅ Global config updated: {} = {}", key, value);
                        }
                    }
                    ConfigCommands::Get { key, local } => {
                        let config = if *local {
                            ToriiConfig::load_local(".")?
                        } else {
                            ToriiConfig::load_global()?
                        };
                        
                        if let Some(value) = config.get(key) {
                            println!("{}", value);
                        } else {
                            println!("❌ Config key not found: {}", key);
                        }
                    }
                    ConfigCommands::List { local } => {
                        let config = if *local {
                            ToriiConfig::load_local(".")?
                        } else {
                            ToriiConfig::load_global()?
                        };
                        
                        let scope = if *local { "Local" } else { "Global" };
                        println!("⚙️  {} Configuration:\n", scope);
                        
                        for (key, value) in config.list() {
                            println!("  {} = {}", key, value);
                        }
                    }
                    ConfigCommands::Edit { local } => {
                        let config_path = if *local {
                            std::path::PathBuf::from(".").join(".torii").join("config.toml")
                        } else {
                            dirs::config_dir()
                                .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
                                .join("torii")
                                .join("config.toml")
                        };
                        
                        // Ensure config exists
                        if *local {
                            let config = ToriiConfig::load_local(".")?;
                            config.save_local(".")?;
                        } else {
                            let config = ToriiConfig::load_global()?;
                            config.save_global()?;
                        }
                        
                        // Get editor
                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                        
                        // Open editor
                        let status = std::process::Command::new(&editor)
                            .arg(&config_path)
                            .status()?;
                        
                        if status.success() {
                            println!("✅ Configuration edited");
                        } else {
                            println!("❌ Editor exited with error");
                        }
                    }
                    ConfigCommands::Reset { local } => {
                        let config = ToriiConfig::default();

                        if *local {
                            config.save_local(".")?;
                            println!("✅ Local configuration reset to defaults");
                        } else {
                            config.save_global()?;
                            println!("✅ Global configuration reset to defaults");
                        }
                    }
                    ConfigCommands::CheckSsh => {
                        run_ssh_check();
                    }
                }
            }

            Commands::Remote { action } => {
                match action {
                    RemoteCommands::Create { platforms, name, description, public, private: _, push, namespace } => {
                        let platforms: Vec<String> = platforms.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                        if platforms.is_empty() {
                            anyhow::bail!("At least one platform is required");
                        }
                        let visibility = if *public { Visibility::Public } else { Visibility::Private };
                        let multi = platforms.len() > 1;

                        // Resolve namespace + repo name. Precedence:
                        //   --namespace <owner> wins (NAME stays bare).
                        //   else, last `/` in NAME splits owner/repo (GitLab
                        //   subgroups stay in the owner segment, e.g.
                        //   `engineering/web/api` → owner=`engineering/web`,
                        //   repo=`api`).
                        let (resolved_ns, resolved_name): (Option<String>, String) = match namespace {
                            Some(ns) => (Some(ns.clone()), name.clone()),
                            None => match name.rsplit_once('/') {
                                Some((owner, repo)) => (Some(owner.to_string()), repo.to_string()),
                                None => (None, name.clone()),
                            },
                        };

                        let mut created: Vec<(String, crate::remote::RemoteRepo)> = Vec::new();
                        for platform in &platforms {
                            print!("🚀 {} - ", platform);
                            match get_platform_client(platform) {
                                Ok(client) => match client.create_repo(&resolved_name, description.as_deref(), visibility.clone(), resolved_ns.as_deref()) {
                                    Ok(repo) => {
                                        println!("✅ Created");
                                        println!("   URL: {}", repo.url);
                                        println!("   SSH: {}", repo.ssh_url);
                                        created.push((platform.clone(), repo));
                                    }
                                    Err(e) => println!("❌ Failed: {}", e),
                                },
                                Err(e) => println!("❌ Platform error: {}", e),
                            }
                        }

                        if multi {
                            println!("\n📊 Created on {}/{} platforms", created.len(), platforms.len());
                        }

                        if *push && !created.is_empty() {
                            println!("\n📤 Linking remotes and pushing...");
                            let git_repo = GitRepo::open(".")?;
                            for (idx, (platform, repo)) in created.iter().enumerate() {
                                let remote_name = if !multi || idx == 0 { "origin".to_string() } else { platform.clone() };
                                let _ = git_repo.repository().remote(&remote_name, &repo.ssh_url);
                            }
                            git_repo.push(false)?;
                            println!("✅ Pushed");
                        }
                    }
                    RemoteCommands::Delete { platforms, owner, repo, yes } => {
                        let platforms: Vec<String> = platforms.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                        if platforms.is_empty() {
                            anyhow::bail!("At least one platform is required");
                        }
                        if !yes {
                            println!("⚠️  Are you sure you want to delete {}/{} on {} platform(s)? This cannot be undone!", owner, repo, platforms.len());
                            println!("   Run with --yes to confirm");
                            return Ok(());
                        }

                        for platform in &platforms {
                            print!("🗑️  {} - ", platform);
                            match get_platform_client(platform) {
                                Ok(client) => match client.delete_repo(owner, repo) {
                                    Ok(_) => println!("✅ Deleted"),
                                    Err(e) => println!("❌ Failed: {}", e),
                                },
                                Err(e) => println!("❌ Platform error: {}", e),
                            }
                        }
                        return Ok(());
                    }
                    RemoteCommands::Visibility { platform, owner, repo, public, private } => {
                        let client = get_platform_client(platform)?;
                        
                        let visibility = if *public {
                            Visibility::Public
                        } else if *private {
                            Visibility::Private
                        } else {
                            println!("❌ Specify --public or --private");
                            return Ok(());
                        };
                        
                        println!("🔒 Changing visibility of {}/{} to {:?}...", owner, repo, visibility);
                        client.set_visibility(owner, repo, visibility)?;
                        println!("✅ Visibility updated");
                    }
                    RemoteCommands::Configure { 
                        platform, owner, repo, description, homepage, default_branch,
                        enable_issues, disable_issues, enable_wiki, disable_wiki,
                        enable_projects, disable_projects 
                    } => {
                        let client = get_platform_client(platform)?;
                        
                        // Build settings
                        let mut settings = RepoSettings::default();
                        settings.description = description.clone();
                        settings.homepage = homepage.clone();
                        settings.default_branch = default_branch.clone();
                        
                        // Build features
                        let mut features = RepoFeatures::default();
                        if *enable_issues { features.issues = Some(true); }
                        if *disable_issues { features.issues = Some(false); }
                        if *enable_wiki { features.wiki = Some(true); }
                        if *disable_wiki { features.wiki = Some(false); }
                        if *enable_projects { features.projects = Some(true); }
                        if *disable_projects { features.projects = Some(false); }
                        
                        println!("⚙️  Configuring repository {}/{}...", owner, repo);
                        
                        // Update settings if any
                        if settings.description.is_some() || settings.homepage.is_some() || settings.default_branch.is_some() {
                            client.update_repo(owner, repo, settings)?;
                        }
                        
                        // Update features if any
                        if features.issues.is_some() || features.wiki.is_some() || features.projects.is_some() {
                            client.configure_features(owner, repo, features)?;
                        }
                        
                        println!("✅ Repository configured");
                    }
                    RemoteCommands::Info { platform, owner, repo } => {
                        let client = get_platform_client(platform)?;
                        println!("📊 Fetching repository information...");
                        let repo_info = client.get_repo(owner, repo)?;
                        
                        println!("\n📦 Repository: {}", repo_info.name);
                        if let Some(desc) = &repo_info.description {
                            println!("   Description: {}", desc);
                        }
                        println!("   Visibility: {:?}", repo_info.visibility);
                        println!("   Default Branch: {}", repo_info.default_branch);
                        println!("   URL: {}", repo_info.url);
                        println!("   SSH: {}", repo_info.ssh_url);
                    }
                    RemoteCommands::Local => {
                        let repo = GitRepo::open(".")?;
                        let git_repo = repo.repository();
                        let remotes = git_repo.remotes()?;
                        if remotes.is_empty() {
                            println!("No remotes configured");
                        } else {
                            for name in remotes.iter().flatten() {
                                if let Ok(remote) = git_repo.find_remote(name) {
                                    let url = remote.url().unwrap_or("(no url)");
                                    println!("  {}  {}", name, url);
                                }
                            }
                        }
                    }
                    RemoteCommands::Link { name, platform, repo, https, url, force } => {
                        let resolved_url = if let Some(u) = url {
                            u.clone()
                        } else {
                            let plat = platform.as_deref().ok_or_else(|| anyhow::anyhow!(
                                "Provide --url <URL> or <platform> <owner>/<repo>"
                            ))?;
                            let owner_repo = repo.as_deref().ok_or_else(|| anyhow::anyhow!(
                                "Missing <owner>/<repo>"
                            ))?;
                            let (ssh_host, https_host) = match plat {
                                "github"    => ("github.com", "github.com"),
                                "gitlab"    => ("gitlab.com", "gitlab.com"),
                                "codeberg"  => ("codeberg.org", "codeberg.org"),
                                "bitbucket" => ("bitbucket.org", "bitbucket.org"),
                                "gitea"     => ("gitea.com", "gitea.com"),
                                "forgejo"   => ("codeberg.org", "codeberg.org"),
                                "sourcehut" => ("git.sr.ht", "git.sr.ht"),
                                _ => anyhow::bail!(
                                    "Unknown platform '{}'. Supported: github, gitlab, codeberg, bitbucket, gitea, forgejo, sourcehut",
                                    plat
                                ),
                            };
                            let use_ssh = if *https { false } else { SshHelper::has_ssh_keys() };
                            if use_ssh {
                                format!("git@{}:{}.git", ssh_host, owner_repo)
                            } else {
                                format!("https://{}/{}.git", https_host, owner_repo)
                            }
                        };

                        let git_repo = GitRepo::open(".")?;
                        let inner = git_repo.repository();
                        let exists = inner.find_remote(name).is_ok();
                        if exists {
                            if !*force {
                                anyhow::bail!(
                                    "Remote '{}' already exists. Use --force to overwrite, or 'torii remote local' to inspect.",
                                    name
                                );
                            }
                            inner.remote_set_url(name, &resolved_url)?;
                            println!("🔗 Updated remote '{}' → {}", name, resolved_url);
                        } else {
                            inner.remote(name, &resolved_url)?;
                            println!("🔗 Linked remote '{}' → {}", name, resolved_url);
                        }
                    }
                    RemoteCommands::Unlink { name, yes } => {
                        let git_repo = GitRepo::open(".")?;
                        let inner = git_repo.repository();
                        let remote = inner.find_remote(name).map_err(|_| anyhow::anyhow!(
                            "No local remote named '{}'. Run `torii remote local` to list.",
                            name
                        ))?;
                        let url = remote.url().unwrap_or("(no url)").to_string();
                        drop(remote);

                        if !*yes {
                            use std::io::{BufRead, Write};
                            println!("⚠️  Drop local alias '{}' → {}?", name, url);
                            println!("   (Does NOT touch the remote on the platform.)");
                            print!("   Confirm [y/N]: ");
                            std::io::stdout().flush().ok();
                            let mut line = String::new();
                            std::io::stdin().lock().read_line(&mut line)?;
                            let ans = line.trim().to_ascii_lowercase();
                            if !matches!(ans.as_str(), "y" | "yes") {
                                println!("Aborted.");
                                return Ok(());
                            }
                        }

                        inner.remote_delete(name)
                            .map_err(|e| anyhow::anyhow!("delete remote '{}': {}", name, e))?;
                        println!("🔗 Unlinked local remote '{}' (platform untouched)", name);
                    }
                    RemoteCommands::List { platform } => {
                        let client = get_platform_client(platform)?;
                        println!("📋 Fetching repositories from {}...", platform);
                        let repos = client.list_repos()?;

                        if repos.is_empty() {
                            println!("No repositories found");
                        } else {
                            println!("\n📦 Repositories ({}):\n", repos.len());
                            for repo in repos {
                                println!("  • {} - {:?}", repo.name, repo.visibility);
                                if let Some(desc) = &repo.description {
                                    println!("    {}", desc);
                                }
                            }
                        }
                    }
                    RemoteCommands::Refs { target, heads, tags } => {
                        // Resolve target — local remote alias or URL.
                        let repo = git2::Repository::open(".")?;
                        let mut remote = match repo.find_remote(target) {
                            Ok(r) => r,
                            Err(_) => repo.remote_anonymous(target)?,
                        };
                        // Connect read-only with default auth callbacks.
                        let mut callbacks = git2::RemoteCallbacks::new();
                        callbacks.credentials(|url, user, allowed| {
                            // SSH agent first (most common); fall back to
                            // userpass-plaintext nothing — let libgit2 fail
                            // cleanly so the user knows to set up auth.
                            if allowed.contains(git2::CredentialType::SSH_KEY) {
                                git2::Cred::ssh_key_from_agent(user.unwrap_or("git"))
                            } else {
                                Err(git2::Error::from_str(&format!(
                                    "no credentials available for {url}"
                                )))
                            }
                        });
                        remote.connect_auth(git2::Direction::Fetch, Some(callbacks), None)?;
                        let list = remote.list()?;
                        for head in list {
                            let name = head.name();
                            let keep = match (*heads, *tags) {
                                (true, false) => name.starts_with("refs/heads/"),
                                (false, true) => name.starts_with("refs/tags/"),
                                _ => true,
                            };
                            if keep {
                                println!("{}\t{}", head.oid(), name);
                            }
                        }
                    }
                }
            }


            Commands::Show { object, blame, lines, signature } => {
                let repo = GitRepo::open(".")?;
                if *signature {
                    run_show_signature(&repo, object.as_deref())?;
                } else if *blame {
                    let file = object.as_deref().ok_or_else(|| anyhow::anyhow!("File path required for --blame"))?;
                    repo.blame(file, lines.as_deref())?;
                } else {
                    repo.show(object.as_deref())?;
                }
            }

            Commands::Sign { target, print_only, yes } => {
                run_sign(target.as_deref(), *print_only, *yes)?;
            }

            Commands::History { action } => {
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
                    HistoryCommands::Rebase { target, interactive, todo_file, root, r#continue, abort, skip } => {
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
                        let stats = history_reauthor::reauthor(
                            std::path::Path::new("."),
                            old_m,
                            new_id,
                            &opts,
                        )?;
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
                            let mailmap_path = file
                                .clone()
                                .unwrap_or_else(|| PathBuf::from(".mailmap"));
                            if !mailmap_path.exists() {
                                anyhow::bail!(
                                    "mailmap file not found: {}",
                                    mailmap_path.display()
                                );
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
            }

            Commands::Workspace { action } => {
                use crate::workspace::workspace::WorkspaceManager;
                match action {
                    WorkspaceCommands::Add { workspace, path } => {
                        WorkspaceManager::add(workspace, path)?;
                    }
                    WorkspaceCommands::Remove { workspace, path } => {
                        WorkspaceManager::remove(workspace, path)?;
                    }
                    WorkspaceCommands::Delete { workspace } => {
                        WorkspaceManager::delete(workspace)?;
                    }
                    WorkspaceCommands::List => {
                        WorkspaceManager::list()?;
                    }
                    WorkspaceCommands::Status { workspace } => {
                        WorkspaceManager::status(workspace)?;
                    }
                    WorkspaceCommands::Save { workspace, message, all } => {
                        WorkspaceManager::save(workspace, message, *all)?;
                    }
                    WorkspaceCommands::Sync { workspace, force } => {
                        WorkspaceManager::sync(workspace, *force)?;
                    }
                }
            }

            Commands::Pr { action } => {
                use crate::pr::{get_pr_client, detect_platform_from_remote, CreatePrOptions, MergeMethod};
                let repo_path = std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .to_string_lossy().to_string();
                let (platform, owner, repo_name) = detect_platform_from_remote(&repo_path)
                    .ok_or_else(|| crate::error::ToriiError::InvalidConfig(
                        "Could not detect platform from remote. Is 'origin' set to a GitHub/GitLab URL?".to_string()
                    ))?;
                let client = get_pr_client(&platform)?;
                match action {
                    PrCommands::List { state } => {
                        let prs = client.list(&owner, &repo_name, state)?;
                        if prs.is_empty() {
                            println!("No {} pull requests.", state);
                        } else {
                            for pr in &prs {
                                let draft = if pr.draft { " [draft]" } else { "" };
                                let merge = match pr.mergeable {
                                    Some(true)  => " ✓",
                                    Some(false) => " ✗",
                                    None        => "",
                                };
                                println!("#{:<5} {}{}{}", pr.number, pr.title, draft, merge);
                                println!("       {} → {}  by {}  {}", pr.head, pr.base, pr.author, pr.created_at);
                                println!("       {}", pr.url);
                                println!();
                            }
                        }
                    }
                    PrCommands::Create { title, base, head, description, draft } => {
                        let head_branch = if let Some(h) = head {
                            h.clone()
                        } else {
                            let repo = git2::Repository::discover(&repo_path)
                                .map_err(crate::error::ToriiError::Git)?;
                            repo.head().ok()
                                .and_then(|h| h.shorthand().map(|s| s.to_string()))
                                .unwrap_or_else(|| "HEAD".to_string())
                        };
                        let opts = CreatePrOptions {
                            title: title.clone(),
                            body: description.clone(),
                            head: head_branch,
                            base: base.clone(),
                            draft: *draft,
                        };
                        let pr = client.create(&owner, &repo_name, opts)?;
                        println!("Created PR #{}: {}", pr.number, pr.title);
                        println!("{}", pr.url);
                    }
                    PrCommands::Merge { number, method } => {
                        let merge_method = match method.as_str() {
                            "squash" => MergeMethod::Squash,
                            "rebase" => MergeMethod::Rebase,
                            _        => MergeMethod::Merge,
                        };
                        client.merge(&owner, &repo_name, *number, merge_method)?;
                        println!("Merged PR #{}", number);
                    }
                    PrCommands::Close { number } => {
                        client.close(&owner, &repo_name, *number)?;
                        println!("Closed PR #{}", number);
                    }
                    PrCommands::Checkout { number } => {
                        let pr = client.get(&owner, &repo_name, *number)?;
                        let branch = client.checkout_branch(&pr);
                        let status = std::process::Command::new("torii")
                            .args(["branch", &branch])
                            .status();
                        match status {
                            Ok(s) if s.success() => println!("Checked out branch: {}", branch),
                            _ => eprintln!("Failed to checkout branch: {}", branch),
                        }
                    }
                    PrCommands::Open { number } => {
                        let pr = client.get(&owner, &repo_name, *number)?;
                        let _ = std::process::Command::new("xdg-open")
                            .arg(&pr.url)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                        println!("Opening: {}", pr.url);
                    }
                }
            }

            Commands::Issue { action } => {
                let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
                let (platform, owner, repo_name) = detect_platform_from_remote(&repo_path)
                    .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote origin"))?;
                let client = get_issue_client(&platform)?;
                match action {
                    IssueCommands::List { state } => {
                        let issues = client.list(&owner, &repo_name, &state)?;
                        if issues.is_empty() {
                            println!("No {} issues.", state);
                        } else {
                            for i in &issues {
                                let labels = if i.labels.is_empty() {
                                    String::new()
                                } else {
                                    format!(" [{}]", i.labels.join(", "))
                                };
                                let comments = if i.comments > 0 { format!(" 💬{}", i.comments) } else { String::new() };
                                println!("#{:<6} {}{}{}", i.number, i.title, labels, comments);
                                println!("       {} → {}  by {}  {}", i.state, i.url, i.author, &i.created_at[..10]);
                            }
                        }
                    }
                    IssueCommands::Create { title, description } => {
                        let opts = CreateIssueOptions { title: title.clone(), body: description.clone() };
                        let issue = client.create(&owner, &repo_name, opts)?;
                        println!("Created issue #{}: {}", issue.number, issue.title);
                        println!("{}", issue.url);
                    }
                    IssueCommands::Close { number } => {
                        client.close(&owner, &repo_name, *number)?;
                        println!("✅ Closed issue #{}", number);
                    }
                    IssueCommands::Comment { number, message } => {
                        client.comment(&owner, &repo_name, *number, message)?;
                        println!("✅ Comment added to issue #{}", number);
                    }
                }
            }

            Commands::Pipeline { action, remote } => {
                let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
                let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
                    .ok_or_else(|| anyhow::anyhow!(
                        "Could not detect platform from remote `{}`. \
                         Check the remote exists (`torii remote local`) and points to github / gitlab / codeberg.",
                        remote
                    ))?;
                let client = get_pipeline_client(&platform)?;
                match action {
                    PipelineCommands::List { status, limit } => {
                        let filters = ListFilters { status: status.clone(), per_page: *limit };
                        let pipelines = client.list(&owner, &repo_name, &filters)?;
                        if pipelines.is_empty() {
                            println!("No pipelines found.");
                        } else {
                            println!("{:<12} {:<12} {:<24} {:<10} {}", "ID", "STATUS", "BRANCH", "SHA", "CREATED");
                            for p in &pipelines {
                                let icon = match p.status.as_str() {
                                    "success"  => "✅",
                                    "failed"   => "❌",
                                    "running"  => "🟡",
                                    "canceled" => "⚪",
                                    "pending"  => "⏳",
                                    _          => "·",
                                };
                                let sha_short = p.sha.chars().take(8).collect::<String>();
                                let created = p.created_at.get(..10).unwrap_or(&p.created_at);
                                println!("{:<12} {} {:<10} {:<24} {:<10} {}",
                                         p.id, icon, p.raw_status, p.branch, sha_short, created);
                            }
                        }
                    }
                    PipelineCommands::Cancel { id } => {
                        client.cancel(&owner, &repo_name, id)?;
                        println!("✅ Cancelled pipeline {}", id);
                    }
                    PipelineCommands::Retry { id } => {
                        client.retry(&owner, &repo_name, id)?;
                        println!("✅ Retried pipeline {}", id);
                    }
                    PipelineCommands::Delete { id, status, older_than, yes } => {
                        // Two modes:
                        //   1. Explicit id → delete that one.
                        //   2. Filter-driven → list everything matching
                        //      --status, narrow further by --older-than,
                        //      confirm count, delete each. Reports per-id
                        //      success/failure so a single 403 doesn't abort
                        //      the rest.
                        if let Some(pid) = id {
                            if !*yes {
                                print!("Delete pipeline {}? [y/N] ", pid);
                                use std::io::Write;
                                std::io::stdout().flush()?;
                                let mut input = String::new();
                                std::io::stdin().read_line(&mut input)?;
                                if !input.trim().eq_ignore_ascii_case("y") {
                                    println!("❌ Cancelled.");
                                    return Ok(());
                                }
                            }
                            client.delete(&owner, &repo_name, pid)?;
                            println!("✅ Deleted pipeline {}", pid);
                            return Ok(());
                        }
                        if status.is_none() && older_than.is_none() {
                            anyhow::bail!(
                                "`pipeline delete` needs either an explicit id or \
                                 at least one of --status / --older-than"
                            );
                        }
                        // List up to 100 matching, then narrow by date.
                        let filters = ListFilters { status: status.clone(), per_page: 100 };
                        let mut targets = client.list(&owner, &repo_name, &filters)?;
                        if let Some(d) = older_than.as_deref() {
                            let mins = crate::duration::parse_duration(d)? as i64;
                            let days = std::cmp::max(1, mins / (60 * 24));
                            targets = filter_older_than(targets, days);
                        }
                        if targets.is_empty() {
                            println!("No pipelines matched the filter.");
                            return Ok(());
                        }
                        if !*yes {
                            println!("Will delete {} pipeline(s):", targets.len());
                            for p in targets.iter().take(10) {
                                println!("  {} [{}] {} {}",
                                         p.id, p.raw_status, p.branch, &p.created_at[..p.created_at.len().min(10)]);
                            }
                            if targets.len() > 10 {
                                println!("  ... and {} more", targets.len() - 10);
                            }
                            print!("Proceed? [y/N] ");
                            use std::io::Write;
                            std::io::stdout().flush()?;
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                println!("❌ Cancelled.");
                                return Ok(());
                            }
                        }
                        let mut ok = 0usize;
                        let mut failed: Vec<(String, String)> = Vec::new();
                        for p in &targets {
                            match client.delete(&owner, &repo_name, &p.id) {
                                Ok(_) => { ok += 1; println!("  ✅ {}", p.id); }
                                Err(e) => { failed.push((p.id.clone(), e.to_string())); println!("  ❌ {}: {}", p.id, e); }
                            }
                        }
                        println!("Done: {} deleted, {} failed.", ok, failed.len());
                        if !failed.is_empty() {
                            anyhow::bail!("{} pipeline(s) could not be deleted", failed.len());
                        }
                    }
                }
            }

            Commands::Job { action, remote } => {
                let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
                let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
                    .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote `{}`.", remote))?;
                let client = get_pipeline_client(&platform)?;
                match action {
                    JobCommands::List { pipeline, status } => {
                        let jobs = client.list_jobs(&owner, &repo_name, pipeline, status.as_deref())?;
                        if jobs.is_empty() {
                            println!("No jobs found for pipeline {}.", pipeline);
                        } else {
                            println!("{:<14} {:<10} {:<24} {:<12} {}", "ID", "STATUS", "NAME", "STAGE", "DURATION");
                            for j in &jobs {
                                let icon = match j.status.as_str() {
                                    "success"  => "✅",
                                    "failed"   => "❌",
                                    "running"  => "🟡",
                                    "canceled" => "⚪",
                                    "pending"  => "⏳",
                                    _          => "·",
                                };
                                let dur = j.duration_seconds.map(|d| format!("{}s", d as i64)).unwrap_or_else(|| "—".into());
                                println!("{:<14} {} {:<8} {:<24} {:<12} {}",
                                         j.id, icon, j.raw_status, j.name, j.stage, dur);
                            }
                        }
                    }
                    JobCommands::Log { id, tail } => {
                        let log = client.job_log(&owner, &repo_name, id)?;
                        if let Some(n) = tail {
                            let lines: Vec<&str> = log.lines().collect();
                            let start = lines.len().saturating_sub(*n);
                            for line in &lines[start..] {
                                println!("{}", line);
                            }
                        } else {
                            println!("{}", log);
                        }
                    }
                    JobCommands::Retry { id } => {
                        client.job_retry(&owner, &repo_name, id)?;
                        println!("✅ Retried job {}", id);
                    }
                    JobCommands::Cancel { id } => {
                        client.job_cancel(&owner, &repo_name, id)?;
                        println!("✅ Cancelled job {}", id);
                    }
                    JobCommands::Artifacts { id, output } => {
                        let default_path = format!("./{}-artifacts.zip", id);
                        let out = output.clone().unwrap_or(default_path);
                        let path = std::path::Path::new(&out);
                        client.job_artifacts_download(&owner, &repo_name, id, path)?;
                        println!("✅ Downloaded job {} artifacts → {}", id, out);
                    }
                    JobCommands::Erase { id, yes } => {
                        if !*yes {
                            print!("Erase log + artifacts of job {}? (job metadata kept) [y/N] ", id);
                            use std::io::Write;
                            std::io::stdout().flush()?;
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                println!("❌ Cancelled.");
                                return Ok(());
                            }
                        }
                        client.job_erase(&owner, &repo_name, id)?;
                        println!("✅ Erased job {} log + artifacts", id);
                    }
                }
            }

            Commands::Runner { action, remote } => {
                let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
                let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
                    .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote `{}`.", remote))?;
                let client = crate::runner::get_runner_client(&platform)?;
                match action {
                    RunnerCommands::List => {
                        let runners = client.list(&owner, &repo_name)?;
                        if runners.is_empty() {
                            println!("No runners found on {} for {}/{}.", platform, owner, repo_name);
                        } else {
                            println!("{:<8} {:<10} {:<28} {:<8} {:<12} TAGS",
                                     "ID", "STATUS", "DESCRIPTION", "OS", "TYPE");
                            for r in &runners {
                                let icon = match r.status.as_str() {
                                    "online" | "active" => "🟢",
                                    "offline" | "stale" => "⚪",
                                    "paused"            => "⏸",
                                    _                   => "·",
                                };
                                let tags = if r.tags.is_empty() { "—".into() }
                                           else { r.tags.join(",") };
                                let trim = |s: &str, n: usize| -> String {
                                    if s.chars().count() <= n { s.to_string() }
                                    else { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) }
                                };
                                println!("{:<8} {} {:<8} {:<28} {:<8} {:<12} {}",
                                         r.id, icon, r.status,
                                         trim(&r.description, 27),
                                         trim(&r.os, 7),
                                         trim(&r.runner_type, 11),
                                         tags);
                            }
                        }
                    }
                    RunnerCommands::Show { id } => {
                        let r = client.show(&owner, &repo_name, id)?;
                        println!("Runner #{}", r.id);
                        println!("  description: {}", r.description);
                        println!("  status:      {}{}", r.status,
                                 if r.paused { " (paused)" } else { "" });
                        println!("  type:        {}", r.runner_type);
                        println!("  os:          {}", r.os);
                        if !r.ip_address.is_empty() {
                            println!("  ip:          {}", r.ip_address);
                        }
                        if !r.version.is_empty() {
                            println!("  version:     {}", r.version);
                        }
                        println!("  tags:        {}",
                                 if r.tags.is_empty() { "—".to_string() } else { r.tags.join(", ") });
                        if !r.web_url.is_empty() {
                            println!("  url:         {}", r.web_url);
                        }
                    }
                    RunnerCommands::Remove { id, yes } => {
                        if !*yes {
                            print!("Delete runner {} on {}? The agent on the host still needs uninstalling separately. [y/N] ", id, platform);
                            use std::io::Write;
                            std::io::stdout().flush()?;
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                println!("❌ Cancelled.");
                                return Ok(());
                            }
                        }
                        client.remove(&owner, &repo_name, id)?;
                        println!("✅ Removed runner {}", id);
                    }
                    RunnerCommands::ResetToken { id } => {
                        let new_token = client.reset_token(&owner, &repo_name, id)?;
                        println!("✅ New auth token for runner {}:\n", id);
                        println!("    {}\n", new_token);
                        println!("Paste it into the runner's config.toml under `token = \"…\"` and restart the agent.");
                    }
                    RunnerCommands::Pause { id } => {
                        client.pause(&owner, &repo_name, id)?;
                        println!("⏸  Paused runner {}", id);
                    }
                    RunnerCommands::Resume { id } => {
                        client.resume(&owner, &repo_name, id)?;
                        println!("▶  Resumed runner {}", id);
                    }
                    RunnerCommands::Register {
                        description, tags, executor, docker_image, runner_dir, yes,
                    } => {
                        let reg = client.registration_token(&owner, &repo_name)?;
                        run_runner_register(
                            &platform, &reg,
                            description.as_deref(),
                            tags.as_deref(),
                            executor,
                            docker_image,
                            runner_dir.as_deref(),
                            *yes,
                        )?;
                    }
                    RunnerCommands::Init => {
                        run_runner_init(&platform)?;
                    }
                    RunnerCommands::Spawn {
                        name, runner_image, executor, image, tags, description, yes,
                    } => {
                        let reg = client.registration_token(&owner, &repo_name)?;
                        run_runner_spawn(
                            &platform, &owner, &repo_name, &reg,
                            name.as_deref(),
                            runner_image,
                            executor,
                            image,
                            tags.as_deref(),
                            description.as_deref(),
                            *yes,
                        )?;
                    }
                    RunnerCommands::Status => {
                        run_runner_status()?;
                    }
                    RunnerCommands::Start { name } => {
                        run_runner_docker(&["start", &container_name(name)], "start")?;
                    }
                    RunnerCommands::Stop { name } => {
                        run_runner_docker(&["stop", &container_name(name)], "stop")?;
                    }
                    RunnerCommands::Logs { name, follow, tail } => {
                        let mut args: Vec<String> = vec!["logs".to_string()];
                        if *follow { args.push("-f".to_string()); }
                        if let Some(n) = tail { args.push(format!("--tail={}", n)); }
                        args.push(container_name(name));
                        let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                        run_runner_docker_inherit(&argv)?;
                    }
                    RunnerCommands::Destroy { name, yes } => {
                        let full = container_name(name);
                        if !*yes {
                            print!("Destroy container `{}` (still leaves the runner registered on the platform)? [y/N] ", full);
                            use std::io::Write;
                            std::io::stdout().flush()?;
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                println!("❌ Cancelled.");
                                return Ok(());
                            }
                        }
                        // `rm -f` stops + removes in one go.
                        run_runner_docker(&["rm", "-f", &full], "rm")?;
                    }
                }
            }

            Commands::Package { action, remote } => {
                let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
                let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
                    .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote `{}`.", remote))?;
                let client = get_package_client(&platform)?;
                match action {
                    PackageCommands::List { package_type, name, limit } => {
                        let filters = PackageListFilters {
                            package_type: package_type.clone(),
                            name_search: name.clone(),
                            per_page: *limit,
                        };
                        let packages = client.list(&owner, &repo_name, &filters)?;
                        if packages.is_empty() {
                            println!("No packages found.");
                        } else {
                            println!("{:<8} {:<20} {:<16} {:<10} {}", "ID", "NAME", "VERSION", "TYPE", "CREATED");
                            for p in &packages {
                                let created = p.created_at.get(..10).unwrap_or(&p.created_at);
                                println!("{:<8} {:<20} {:<16} {:<10} {}",
                                         p.id, p.name, p.version, p.package_type, created);
                            }
                        }
                    }
                    PackageCommands::Files { id } => {
                        let files = client.list_files(&owner, &repo_name, id)?;
                        if files.is_empty() {
                            println!("No files in package {}.", id);
                        } else {
                            println!("{:<10} {:<40} {}", "FILE-ID", "NAME", "SIZE");
                            for f in &files {
                                let size_mb = f.size_bytes as f64 / 1_048_576.0;
                                println!("{:<10} {:<40} {:.2} MB",
                                         f.id, f.file_name, size_mb);
                            }
                        }
                    }
                    PackageCommands::Delete { id, version, older_than, yes } => {
                        if let Some(pid) = id {
                            if !*yes {
                                print!("Delete package {}? [y/N] ", pid);
                                use std::io::Write;
                                std::io::stdout().flush()?;
                                let mut input = String::new();
                                std::io::stdin().read_line(&mut input)?;
                                if !input.trim().eq_ignore_ascii_case("y") {
                                    println!("❌ Cancelled.");
                                    return Ok(());
                                }
                            }
                            client.delete(&owner, &repo_name, pid)?;
                            println!("✅ Deleted package {}", pid);
                            return Ok(());
                        }
                        if version.is_none() && older_than.is_none() {
                            anyhow::bail!(
                                "`package delete` needs either an explicit id or at least one of --version / --older-than"
                            );
                        }
                        // List ALL packages (no API-side version filter needed,
                        // we narrow client-side for predictable semantics).
                        let filters = PackageListFilters { per_page: 100, ..PackageListFilters::default() };
                        let mut targets = client.list(&owner, &repo_name, &filters)?;
                        if let Some(v) = version {
                            targets = pkg_filter_by_version(targets, v);
                        }
                        if let Some(d) = older_than.as_deref() {
                            let mins = crate::duration::parse_duration(d)? as i64;
                            let days = std::cmp::max(1, mins / (60 * 24));
                            targets = pkg_filter_older_than(targets, days);
                        }
                        if targets.is_empty() {
                            println!("No packages matched the filter.");
                            return Ok(());
                        }
                        if !*yes {
                            println!("Will delete {} package(s):", targets.len());
                            for p in targets.iter().take(10) {
                                println!("  {} {} {} {} {}",
                                         p.id, p.name, p.version, p.package_type,
                                         &p.created_at[..p.created_at.len().min(10)]);
                            }
                            if targets.len() > 10 {
                                println!("  ... and {} more", targets.len() - 10);
                            }
                            print!("Proceed? [y/N] ");
                            use std::io::Write;
                            std::io::stdout().flush()?;
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                println!("❌ Cancelled.");
                                return Ok(());
                            }
                        }
                        let mut ok = 0usize;
                        let mut failed: Vec<(String, String)> = Vec::new();
                        for p in &targets {
                            match client.delete(&owner, &repo_name, &p.id) {
                                Ok(_) => { ok += 1; println!("  ✅ {} {} {}", p.id, p.name, p.version); }
                                Err(e) => { failed.push((p.id.clone(), e.to_string())); println!("  ❌ {}: {}", p.id, e); }
                            }
                        }
                        println!("Done: {} deleted, {} failed.", ok, failed.len());
                        if !failed.is_empty() {
                            anyhow::bail!("{} package(s) could not be deleted", failed.len());
                        }
                    }
                }
            }

            Commands::Release { action, remote } => {
                let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
                let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
                    .ok_or_else(|| anyhow::anyhow!("Could not detect platform from remote `{}`.", remote))?;
                let client = get_release_client(&platform)?;
                match action {
                    ReleaseCommands::List { limit } => {
                        let rels = client.list(&owner, &repo_name, *limit)?;
                        if rels.is_empty() {
                            println!("No releases found.");
                        } else {
                            println!("{:<14} {:<32} {}", "TAG", "NAME", "CREATED");
                            for r in &rels {
                                let created = r.created_at.get(..10).unwrap_or(&r.created_at);
                                println!("{:<14} {:<32} {}", r.tag, r.name, created);
                            }
                        }
                    }
                    ReleaseCommands::Show { tag } => {
                        let r = client.get(&owner, &repo_name, tag)?;
                        println!("Tag:         {}", r.tag);
                        println!("Name:        {}", r.name);
                        println!("Created:     {}", r.created_at);
                        if !r.web_url.is_empty() {
                            println!("URL:         {}", r.web_url);
                        }
                        if let Some(id) = &r.id {
                            println!("ID:          {}", id);
                        }
                        println!("\n--- Description ---\n{}", r.description);
                    }
                    ReleaseCommands::Edit { tag, name, notes } => {
                        // Resolve `--notes` source: file path, `-` for stdin, or absent.
                        let body = match notes.as_deref() {
                            Some("-") => {
                                use std::io::Read;
                                let mut buf = String::new();
                                std::io::stdin().read_to_string(&mut buf)?;
                                Some(buf)
                            }
                            Some(path) => {
                                Some(std::fs::read_to_string(path)
                                    .map_err(|e| anyhow::anyhow!("Failed to read notes file {}: {}", path, e))?)
                            }
                            None => None,
                        };
                        client.edit(&owner, &repo_name, tag, name.as_deref(), body.as_deref())?;
                        println!("✅ Edited release {}", tag);
                    }
                    ReleaseCommands::Delete { tag, yes } => {
                        if !*yes {
                            print!("Delete release {} (tag stays, only the release entity is removed)? [y/N] ", tag);
                            use std::io::Write;
                            std::io::stdout().flush()?;
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            if !input.trim().eq_ignore_ascii_case("y") {
                                println!("❌ Cancelled.");
                                return Ok(());
                            }
                        }
                        client.delete(&owner, &repo_name, tag)?;
                        println!("✅ Deleted release {}", tag);
                    }
                }
            }

            Commands::Ignore { action } => {
                handle_ignore(action)?;
            }

            Commands::Tui => {
                let current = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                if git2::Repository::discover(&current).is_ok() {
                    // Estamos dentro de un repo — abre directamente
                    crate::tui::run()?;
                } else {
                    // No hay repo — lanza el picker
                    use crate::tui::picker::{run_picker, save_workspace, PickerResult};
                    match run_picker(&current)? {
                        PickerResult::Cancelled => {}
                        PickerResult::SingleRepo(path) => {
                            std::env::set_current_dir(&path)?;
                            crate::tui::run()?;
                        }
                        PickerResult::Workspace { name, repos } => {
                            save_workspace(&name, &repos)?;
                            if let Some(first) = repos.first() {
                                std::env::set_current_dir(first)?;
                            }
                            crate::tui::run_with_workspace(name)?;
                        }
                        PickerResult::OpenWorkspace(name) => {
                            let ws_path = dirs::home_dir()
                                .map(|h| h.join(".torii/workspaces.toml"))
                                .unwrap_or_default();
                            if let Ok(content) = std::fs::read_to_string(&ws_path) {
                                let mut in_ws = false;
                                let mut first_path: Option<std::path::PathBuf> = None;
                                for line in content.lines() {
                                    let line = line.trim();
                                    if line == format!("[{}]", name) { in_ws = true; continue; }
                                    if line.starts_with('[') { in_ws = false; }
                                    if in_ws && line.starts_with("path") {
                                        let p = line.split('=').nth(1).unwrap_or("").trim().trim_matches('"');
                                        first_path = Some(std::path::PathBuf::from(p));
                                        break;
                                    }
                                }
                                if let Some(p) = first_path {
                                    std::env::set_current_dir(&p)?;
                                }
                            }
                            crate::tui::run_with_workspace(name)?;
                        }
                    }
                }
            }

            Commands::Worktree { action } => {
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
            }

            Commands::Submodule { action } => {
                use crate::submodule;
                let repo_path = std::path::Path::new(".");
                match action.as_ref() {
                    None | Some(SubmoduleCommands::Status) => {
                        submodule::status(repo_path)?;
                    }
                    Some(SubmoduleCommands::Add { url, path, branch, name, recursive }) => {
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
                            anyhow::bail!("foreach needs a command, e.g. torii submodule foreach 'cargo build'");
                        }
                        let joined = cmd.join(" ");
                        submodule::foreach(repo_path, &joined)?;
                    }
                    Some(SubmoduleCommands::Remove { path }) => {
                        submodule::remove(repo_path, path)?;
                    }
                }
            }

            Commands::Subtree { action } => {
                use crate::subtree;
                let repo_path = std::path::Path::new(".");
                match action {
                    SubtreeCommands::Add { prefix, url, refname, squash } => {
                        subtree::add(repo_path, prefix, url, refname, &subtree::CommonOpts { squash: *squash })?;
                    }
                    SubtreeCommands::Pull { prefix, url, refname, squash } => {
                        subtree::pull(repo_path, prefix, url, refname, &subtree::CommonOpts { squash: *squash })?;
                    }
                    SubtreeCommands::Push { prefix, url, refname } => {
                        subtree::push(repo_path, prefix, url, refname)?;
                    }
                    SubtreeCommands::Split { prefix, branch, annotate } => {
                        subtree::split(repo_path, prefix, branch.as_deref(), annotate.as_deref())?;
                    }
                    SubtreeCommands::Merge { prefix, refname, squash } => {
                        subtree::merge(repo_path, prefix, refname, &subtree::CommonOpts { squash: *squash })?;
                    }
                }
            }

            Commands::Bisect { action } => {
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
            }

            Commands::Describe { tags, long, dirty, candidates } => {
                let opts = crate::describe::Opts {
                    tags: *tags,
                    long: *long,
                    dirty: *dirty,
                    candidates: *candidates,
                };
                crate::describe::describe(std::path::Path::new("."), &opts)?;
            }

            Commands::Archive { revision, output, format, prefix } => {
                let opts = crate::archive::Opts {
                    output: output.clone(),
                    format: format.clone(),
                    prefix: prefix.clone(),
                };
                crate::archive::archive(std::path::Path::new("."), revision, &opts)?;
            }

            Commands::Remove { paths, cached, recursive, force } => {
                let opts = crate::fileops::RmOpts {
                    cached: *cached,
                    recursive: *recursive,
                    force: *force,
                };
                crate::fileops::rm(std::path::Path::new("."), paths, &opts)?;
            }

            Commands::Rename { from, to, force } => {
                let opts = crate::fileops::MvOpts { force: *force };
                crate::fileops::mv(std::path::Path::new("."), from, to, &opts)?;
            }

            Commands::Grep { pattern, paths, ignore_case, word_regexp, files_with_matches, no_line_number } => {
                let opts = crate::grep::Opts {
                    ignore_case: *ignore_case,
                    word_regexp: *word_regexp,
                    files_with_matches: *files_with_matches,
                    no_line_number: *no_line_number,
                    extra: Vec::new(),
                };
                crate::grep::grep(std::path::Path::new("."), pattern, paths, &opts)?;
            }

            Commands::Notes { action } => {
                let p = std::path::Path::new(".");
                match action.as_ref() {
                    None | Some(NotesCommands::List) => crate::notes::list(p)?,
                    Some(NotesCommands::Add { commit, message, force }) => {
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
            }

            Commands::Patch { action } => {
                let p = std::path::Path::new(".");
                match action {
                    PatchCommands::Export { range, output_dir, stdout, cover_letter } => {
                        let opts = crate::patch::ExportOpts {
                            output_dir: output_dir.clone(),
                            stdout: *stdout,
                            cover_letter: *cover_letter,
                        };
                        crate::patch::export(p, range, &opts)?;
                    }
                    PatchCommands::Apply { files, three_way, continue_, skip, abort } => {
                        let opts = crate::patch::ApplyOpts {
                            three_way: *three_way,
                            continue_: *continue_,
                            skip: *skip,
                            abort: *abort,
                        };
                        crate::patch::apply(p, files, &opts)?;
                    }
                }
            }

            Commands::Clean { force, dirs, include_ignored, only_ignored } => {
                let opts = crate::clean::Opts {
                    force: *force,
                    dirs: *dirs,
                    include_ignored: *include_ignored,
                    only_ignored: *only_ignored,
                };
                crate::clean::clean(std::path::Path::new("."), &opts)?;
            }
        }

        Ok(())
    }
}

fn run_ssh_check() {
    println!("🔐 SSH Configuration Check\n");

    if SshHelper::has_ssh_keys() {
        println!("✅ SSH keys found!\n");

        let keys = SshHelper::list_keys();
        if !keys.is_empty() {
            println!("Available keys:");
            for key in &keys {
                println!("  • {}", key);
            }
        }

        println!("\n💡 Recommendation: Use SSH protocol (default)");
    } else {
        println!("❌ No SSH keys found");
        println!("\n💡 To set up SSH keys:");
        println!("   1. Generate a new key:");
        println!("      ssh-keygen -t ed25519 -C \"your_email@example.com\"");
        println!("   2. Start the SSH agent:");
        println!("      eval \"$(ssh-agent -s)\"");
        println!("   3. Add your key:");
        println!("      ssh-add ~/.ssh/id_ed25519");
        println!("   4. Copy your public key:");
        println!("      cat ~/.ssh/id_ed25519.pub");
        println!("   5. Add it to your Git hosting service");
    }
}

fn run_auth(action: &AuthCommands) -> Result<()> {
    use crate::auth;
    use crate::cloud::{whoami::whoami, CloudClient};

    match action {
        AuthCommands::Login { key, endpoint } => {
            let key_value = match key {
                Some(k) => k.clone(),
                None => {
                    use std::io::{BufRead, Write};
                    print!("API key (gitorii_sk_…): ");
                    std::io::stdout().flush().ok();
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line)?;
                    line.trim().to_string()
                }
            };
            if !key_value.starts_with("gitorii_sk_") {
                anyhow::bail!("API key must start with `gitorii_sk_`");
            }
            let endpoint = endpoint
                .clone()
                .unwrap_or_else(auth::default_endpoint);
            // Validate before saving so we don't store a bogus key.
            let client = CloudClient::new(auth::ApiKey {
                key: key_value.clone(),
                endpoint: endpoint.clone(),
            });
            let me = whoami(&client)?;
            auth::save_cloud(&key_value, &endpoint)?;
            println!("✅ Logged in to {}", endpoint);
            println!("   org:  {} ({})", me.org_name, me.org_slug);
            println!("   plan: {}", me.plan);
        }
        AuthCommands::Status | AuthCommands::Whoami => {
            let key = auth::load().ok_or_else(|| anyhow::anyhow!(
                "no API key configured. Run `torii auth login` or set TORII_API_KEY."
            ))?;
            let client = CloudClient::new(key);
            let me = whoami(&client)?;
            println!("endpoint: {}", client.endpoint());
            println!("org:      {} ({}) [{}]", me.org_name, me.org_slug, me.org_id);
            println!("plan:     {}", me.plan);
            println!("seats:    {}", me.seats);
            if me.suspended {
                println!("status:   ⚠️  suspended");
            }
        }
        AuthCommands::Logout => {
            auth::delete()?;
            println!("✅ Local API key deleted");
            if std::env::var("TORII_API_KEY").is_ok() {
                println!("⚠️  TORII_API_KEY env var still set — unset it to fully log out.");
            }
        }
        AuthCommands::Set { provider, token, local, ttl } => {
            let resolved_token = if token == "-" {
                use std::io::{BufRead, Write};
                eprint!("Paste token (input hidden — Ctrl-D to end): ");
                std::io::stderr().flush().ok();
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line)?;
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    anyhow::bail!("Empty token from stdin.");
                }
                trimmed
            } else {
                token.clone()
            };
            let repo: Option<&std::path::Path> = if *local {
                Some(std::path::Path::new("."))
            } else {
                None
            };
            let expires_at = ttl_to_iso(ttl.as_deref())?;
            auth::set_token_with_expiry(provider, &resolved_token, expires_at.as_deref(), repo)?;
            let scope = if *local { "local" } else { "global" };
            println!("✅ {} token saved ({} store).", provider, scope);
            if let Some(iso) = &expires_at {
                println!("   expires: {}", iso);
            }
        }
        AuthCommands::Get { provider, unsafe_show } => {
            let resolved = auth::resolve_token(provider, ".");
            match (&resolved.value, unsafe_show) {
                (Some(v), true) => println!("{}", v),
                (Some(v), false) => println!("{}", mask_token(v)),
                (None, _) => {
                    println!("(not set for '{}')", provider);
                }
            }
        }
        AuthCommands::List => {
            println!("🔑 Stored tokens:\n");
            for p in auth::PROVIDERS {
                let r = auth::resolve_token(p, ".");
                match r.value.as_deref() {
                    Some(v) => println!(
                        "  {:<10} {:<24} {}",
                        p,
                        mask_token(v),
                        describe_source(&r.source)
                    ),
                    None => println!("  {:<10} —", p),
                }
            }
        }
        AuthCommands::Remove { provider, local } => {
            let repo: Option<&std::path::Path> = if *local {
                Some(std::path::Path::new("."))
            } else {
                None
            };
            let removed = auth::remove_token(provider, repo)?;
            if removed {
                println!("✅ Removed {} token.", provider);
            } else {
                println!("(no {} token was set; nothing to remove)", provider);
            }
        }
        AuthCommands::Oauth { provider, local, ttl } => {
            // Pick the right OAuth flow for the provider. Most platforms
            // support RFC 8628 Device Flow; a few (e.g. Bitbucket) only
            // support Authorization Code Grant and need a localhost
            // loopback server. Both end at the same point: an access
            // token saved under the same provider key.
            let token = if crate::oauth::device_flow_supported(provider) {
                crate::oauth::run_device_flow(provider)?
            } else if crate::oauth::auth_code_flow_supported(provider) {
                crate::oauth::run_auth_code_flow(provider)?
            } else {
                anyhow::bail!(
                    "OAuth flow not configured for `{}`. \
                     Device flow supports: github, gitlab, codeberg. \
                     Auth-code flow supports: bitbucket. \
                     For other providers, create a PAT manually and run `torii auth set {} ...`.",
                    provider, provider
                );
            };
            let repo: Option<&std::path::Path> = if *local {
                Some(std::path::Path::new("."))
            } else {
                None
            };
            let expires_at = ttl_to_iso(ttl.as_deref())?;
            auth::set_token_with_expiry(provider, &token, expires_at.as_deref(), repo)?;
            let scope = if *local { "local" } else { "global" };
            println!("✅ {} token saved ({} store).", provider, scope);
            if let Some(iso) = &expires_at {
                println!("   expires: {}", iso);
            }
        }
        AuthCommands::Rotate { provider, pat, local, ttl } => {
            // Snapshot the current token first — once we set the new
            // value the cached one in `auth::resolve_token` becomes
            // the new one, and there's no way to get the old back.
            let old = auth::resolve_token(provider, ".").value
                .ok_or_else(|| anyhow::anyhow!(
                    "No `{}` token is currently stored. Use `torii auth oauth {}` \
                     or `torii auth set {} <token>` to set one first.",
                    provider, provider, provider
                ))?;

            let new_token = if *pat {
                // PAT-native rotate path. GitLab is the only platform
                // that exposes this today; for others, suggest the
                // OAuth path explicitly.
                if provider != "gitlab" {
                    anyhow::bail!(
                        "`--pat` rotate is only implemented for GitLab. \
                         For `{}`, drop `--pat` to use the OAuth flow.",
                        provider
                    );
                }
                println!("🔁 Rotating GitLab PAT via /personal_access_tokens/self/rotate…");
                crate::oauth::rotate_gitlab_pat(&old)?
            } else {
                // OAuth path: run the flow, then revoke the old token.
                println!("🔁 Rotating {} token via OAuth — re-authorise in your browser.\n", provider);
                let t = if crate::oauth::device_flow_supported(provider) {
                    crate::oauth::run_device_flow(provider)?
                } else if crate::oauth::auth_code_flow_supported(provider) {
                    crate::oauth::run_auth_code_flow(provider)?
                } else {
                    anyhow::bail!(
                        "No OAuth flow wired for `{}`. Supported: github, gitlab, codeberg \
                         (device flow), bitbucket (auth-code+PKCE). For `{}` you'd need to \
                         rotate manually in the platform's web UI.",
                        provider, provider
                    );
                };
                t
            };

            let repo: Option<&std::path::Path> = if *local {
                Some(std::path::Path::new("."))
            } else {
                None
            };
            let expires_at = ttl_to_iso(ttl.as_deref())?;
            auth::set_token_with_expiry(provider, &new_token, expires_at.as_deref(), repo)?;
            let scope = if *local { "local" } else { "global" };
            println!("✅ New {} token saved ({} store).", provider, scope);
            if let Some(iso) = &expires_at {
                println!("   expires: {}", iso);
            }

            // Best-effort revoke. PAT rotate already invalidated the
            // old one server-side, so we skip the revoke call there.
            if !*pat {
                match crate::oauth::revoke_token(provider, &old) {
                    Ok(true)  => println!("✅ Old {} token revoked.", provider),
                    Ok(false) => {
                        let hint = crate::oauth::revoke_hint_url(provider)
                            .unwrap_or("the platform's settings page");
                        println!("⚠  No programmatic revoke for `{}` (or no client secret \
                                  available). Revoke the old token manually at:\n     {}",
                                  provider, hint);
                    }
                    Err(e) => {
                        let hint = crate::oauth::revoke_hint_url(provider)
                            .unwrap_or("the platform's settings page");
                        println!("⚠  Revoke failed: {}. Revoke manually at:\n     {}",
                                  e, hint);
                    }
                }
            }
        }
        AuthCommands::Doctor => {
            println!("🔍 Token resolution (env > local > global):\n");
            for p in auth::PROVIDERS {
                let r = auth::resolve_token(p, ".");
                let state = match &r.value {
                    Some(_) => "✓ resolved",
                    None => "— missing",
                };
                let source = describe_source(&r.source);
                let expiry = auth::token_expires_at(p)
                    .as_deref()
                    .and_then(describe_expiry)
                    .map(|s| format!("   {}", s))
                    .unwrap_or_default();
                println!("  {:<10} {:<14} {}{}", p, state, source, expiry);
            }
            // Also surface the legacy config.toml [auth] if it lingers.
            if let Some(cd) = dirs::config_dir() {
                let cfg = cd.join("torii").join("config.toml");
                if let Ok(t) = std::fs::read_to_string(&cfg) {
                    let has_legacy = t.lines().any(|l| {
                        let l = l.trim();
                        l == "[auth]" || l.starts_with("[auth]")
                    });
                    if has_legacy {
                        println!(
                            "\n⚠  Legacy [auth] block still present in {}.\n   \
                             Tokens have been migrated to auth.toml — you can delete that section now.",
                            cfg.display()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Render a token as `prefix6_xxxx****suffix4` so screenshots / logs
/// don't leak the secret. Tokens shorter than 12 chars are fully masked.
fn mask_token(t: &str) -> String {
    let chars: Vec<char> = t.chars().collect();
    if chars.len() < 12 {
        return "****".to_string();
    }
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{}…{}", head, tail)
}

fn describe_source(s: &crate::auth::TokenSource) -> String {
    match s {
        crate::auth::TokenSource::EnvVar(name) => format!("from env: ${}", name),
        crate::auth::TokenSource::EnvGeneric => "from env: $TORII_HTTPS_TOKEN".to_string(),
        crate::auth::TokenSource::Local => "local .torii/auth.toml".to_string(),
        crate::auth::TokenSource::Global => "global ~/.config/torii/auth.toml".to_string(),
        crate::auth::TokenSource::Missing => "(not set)".to_string(),
    }
}

/// Resolve a `--ttl <duration>` flag into an ISO-8601 timestamp at which
/// the token is considered expired. Returns `Ok(None)` when no TTL was
/// passed; the caller stores `None` as "no expiration tracked".
fn ttl_to_iso(ttl: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(s) = ttl else { return Ok(None) };
    let mins = crate::duration::parse_duration(s)
        .map_err(|e| anyhow::anyhow!("invalid --ttl `{}`: {}", s, e))?;
    let when = chrono::Utc::now() + chrono::Duration::minutes(mins as i64);
    Ok(Some(when.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)))
}

/// One-line "expires in X" / "expired Y ago" string for the doctor and
/// list output. Returns None when the timestamp is missing or unparsable
/// — callers can decide whether to print nothing or a placeholder.
fn describe_expiry(iso: &str) -> Option<String> {
    let when = chrono::DateTime::parse_from_rfc3339(iso).ok()?
        .with_timezone(&chrono::Utc);
    let now = chrono::Utc::now();
    let delta = when.signed_duration_since(now);
    let days = delta.num_days();
    let hours = delta.num_hours();
    if delta.num_seconds() < 0 {
        let past = -delta;
        if past.num_days() > 0 {
            Some(format!("⛔ expired {}d ago ({})", past.num_days(), iso))
        } else if past.num_hours() > 0 {
            Some(format!("⛔ expired {}h ago ({})", past.num_hours(), iso))
        } else {
            Some(format!("⛔ expired just now ({})", iso))
        }
    } else if days < 7 {
        // Warn band: less than a week. Rotate soon.
        if days >= 1 {
            Some(format!("⚠ expires in {}d ({})", days, iso))
        } else {
            Some(format!("⚠ expires in {}h ({})", hours.max(1), iso))
        }
    } else {
        Some(format!("⏳ expires in {}d ({})", days, iso))
    }
}

/// `torii runner register` — wrap the platform's native register CLI.
/// We never copy the platform's logic; we just hand it the token and
/// a tidy argv. If the binary is missing we tell the user where to
/// install it from, instead of trying to ship our own.
fn run_runner_register(
    platform: &str,
    reg: &crate::runner::RegistrationToken,
    description: Option<&str>,
    tags: Option<&str>,
    executor: &str,
    docker_image: &str,
    runner_dir: Option<&str>,
    yes: bool,
) -> Result<()> {
    use std::process::Command;

    let (bin, args, cwd) = match platform {
        "gitlab" => {
            let bin = which_binary("gitlab-runner").ok_or_else(|| anyhow::anyhow!(
                "`gitlab-runner` not found on PATH. Install it first: \
                 https://docs.gitlab.com/runner/install/. Then re-run \
                 `torii runner register`."
            ))?;
            let mut argv: Vec<String> = vec![
                "register".to_string(),
                "--non-interactive".to_string(),
                "--url".to_string(),       reg.register_url.clone(),
                "--registration-token".to_string(), reg.token.clone(),
                "--executor".to_string(),  executor.to_string(),
            ];
            if executor == "docker" {
                argv.push("--docker-image".to_string());
                argv.push(docker_image.to_string());
            }
            if let Some(d) = description {
                argv.push("--description".to_string());
                argv.push(d.to_string());
            }
            if let Some(t) = tags {
                argv.push("--tag-list".to_string());
                argv.push(t.to_string());
            }
            (bin, argv, None::<String>)
        }
        "github" => {
            let dir = runner_dir.map(String::from)
                .unwrap_or_else(|| "./actions-runner".to_string());
            let config_sh = std::path::Path::new(&dir).join("config.sh");
            if !config_sh.exists() {
                anyhow::bail!(
                    "GitHub Actions runner not found at `{}`. Download it from \
                     https://github.com/{}/{}/settings/actions/runners/new and \
                     unpack it there, then re-run with `--runner-dir <path>`.",
                    config_sh.display(),
                    // can't access owner/repo here; the registration_token
                    // URL already includes them.
                    "OWNER", "REPO"
                );
            }
            let mut argv: Vec<String> = vec![
                "--unattended".to_string(),
                "--url".to_string(),    reg.register_url.clone(),
                "--token".to_string(),  reg.token.clone(),
                "--replace".to_string(),
            ];
            if let Some(d) = description {
                argv.push("--name".to_string());
                argv.push(d.to_string());
            }
            if let Some(t) = tags {
                argv.push("--labels".to_string());
                argv.push(t.to_string());
            }
            (config_sh.display().to_string(), argv, Some(dir))
        }
        other => {
            anyhow::bail!(
                "`torii runner register` is GitHub + GitLab only. `{}` not implemented yet.",
                other
            );
        }
    };

    // Show the resolved command so the user can audit it before we
    // launch a subprocess that mutates the host's runner state.
    let pretty_args = args.iter().map(|a| {
        if a.contains(' ') { format!("\"{}\"", a) } else { a.clone() }
    }).collect::<Vec<_>>().join(" ");
    println!("🛠  Will run:");
    if let Some(d) = &cwd {
        println!("     (in {}) {} {}", d, bin, pretty_args);
    } else {
        println!("     {} {}", bin, pretty_args);
    }
    if let Some(exp) = reg.expires_in_seconds {
        println!("     (token expires in ~{}s)", exp);
    }

    if !yes {
        use std::io::Write;
        print!("Proceed? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Cancelled.");
            return Ok(());
        }
    }

    let mut cmd = Command::new(&bin);
    cmd.args(&args);
    if let Some(d) = &cwd { cmd.current_dir(d); }
    let status = cmd.status().map_err(|e| anyhow::anyhow!(
        "failed to exec `{}`: {}", bin, e
    ))?;
    if !status.success() {
        anyhow::bail!(
            "Runner register exited with {}. Token was already consumed; \
             generate a fresh one before retrying.",
            status
        );
    }
    println!("✅ Runner registered.");
    Ok(())
}

/// Final docker container name for a torii-managed runner. Single
/// source of truth so `spawn` and the other commands agree on the
/// name shape.
fn container_name(suffix: &str) -> String {
    let trimmed = suffix.trim();
    if trimmed.is_empty() {
        return "torii-runner".to_string();
    }
    if trimmed.starts_with("torii-runner-") || trimmed == "torii-runner" {
        return trimmed.to_string();
    }
    format!("torii-runner-{}", trimmed)
}

/// `torii runner spawn` — bring a Dockerized GitLab Runner up against
/// the current project. Two-phase: `docker run` to start the agent,
/// then `docker exec gitlab-runner register` inside to attach it.
/// Mirrors the manual flow the user has been doing by hand.
fn run_runner_spawn(
    platform: &str,
    owner: &str,
    repo: &str,
    reg: &crate::runner::RegistrationToken,
    name: Option<&str>,
    runner_image: &str,
    executor: &str,
    job_image: &str,
    tags: Option<&str>,
    description: Option<&str>,
    yes: bool,
) -> Result<()> {
    if platform != "gitlab" {
        anyhow::bail!(
            "`torii runner spawn` is GitLab-only for now. GitHub Actions self-hosted \
             runners use a different container shape (ephemeral tokens, JIT config) — \
             use `torii runner register --runner-dir <path>` against an unpacked \
             actions-runner tarball instead."
        );
    }

    which_binary("docker").ok_or_else(|| anyhow::anyhow!(
        "`docker` not found on PATH. Install Docker (or Podman with a `docker` shim) \
         and re-run."
    ))?;

    let slug = name
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", owner.replace('/', "-"), repo.replace('/', "-")));
    let cname = container_name(&slug);

    // Phase 1 — the docker run command. Mount the host socket so the
    // runner (executor=docker) can launch sibling job containers.
    // `--restart=unless-stopped` matches the convention from
    // gitlab-runner Docker docs.
    let run_args: Vec<String> = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),       cname.clone(),
        "--restart".to_string(),    "unless-stopped".to_string(),
        "-v".to_string(),           "/var/run/docker.sock:/var/run/docker.sock".to_string(),
        "-v".to_string(),           format!("{}-config:/etc/gitlab-runner", cname),
        runner_image.to_string(),
    ];

    // Phase 2 — register inside.
    let mut register_args: Vec<String> = vec![
        "exec".to_string(), cname.clone(),
        "gitlab-runner".to_string(), "register".to_string(),
        "--non-interactive".to_string(),
        "--url".to_string(),                reg.register_url.clone(),
        "--registration-token".to_string(), reg.token.clone(),
        "--executor".to_string(),           executor.to_string(),
    ];
    if executor == "docker" {
        register_args.push("--docker-image".to_string());
        register_args.push(job_image.to_string());
    }
    if let Some(d) = description {
        register_args.push("--description".to_string());
        register_args.push(d.to_string());
    } else {
        register_args.push("--description".to_string());
        register_args.push(format!("torii-spawned {}", cname));
    }
    if let Some(t) = tags {
        register_args.push("--tag-list".to_string());
        register_args.push(t.to_string());
    }

    let pretty_run = run_args.join(" ");
    let pretty_reg = register_args.iter()
        .map(|a| if a == &reg.token { "<TOKEN>".to_string() } else { a.clone() })
        .collect::<Vec<_>>()
        .join(" ");
    println!("🛠  Will run:");
    println!("     docker {}", pretty_run);
    println!("     docker {}", pretty_reg);

    if !yes {
        use std::io::Write;
        print!("Proceed? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Cancelled.");
            return Ok(());
        }
    }

    // Spawn phase. We don't pipe stdout because the user just wants
    // the container id back, same as bare `docker run -d` prints.
    let run_argv: Vec<&str> = run_args.iter().map(|s| s.as_str()).collect();
    run_runner_docker_inherit(&run_argv)?;

    // Brief pause: the freshly-started container may still be coming
    // up when we run exec; gitlab-runner takes a beat to settle. A
    // tight loop polling `docker exec` would be cleaner but a single
    // 1s sleep is enough for the common case and keeps the code
    // trivial.
    std::thread::sleep(std::time::Duration::from_secs(1));

    let reg_argv: Vec<&str> = register_args.iter().map(|s| s.as_str()).collect();
    run_runner_docker_inherit(&reg_argv)?;

    println!("✓ Runner container `{}` up and registered.", cname);
    println!("  · `torii runner status`           list torii-managed runners");
    println!("  · `torii runner logs {} -f`       follow output", cname);
    println!("  · `torii runner stop {}`          pause without unregistering", cname);
    println!("  · `torii runner destroy {}`       remove the container", cname);
    Ok(())
}

/// `torii runner status` — surface every container whose name starts
/// with `torii-runner-`, with its docker state column.
fn run_runner_status() -> Result<()> {
    which_binary("docker").ok_or_else(|| anyhow::anyhow!(
        "`docker` not found on PATH. (`runner status` only knows about Dockerized \
         runners spawned by `torii runner spawn`.)"
    ))?;

    let out = std::process::Command::new("docker")
        .args([
            "ps", "-a",
            "--filter", "name=torii-runner-",
            "--format", "{{.Names}}\t{{.State}}\t{{.Status}}\t{{.Image}}",
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("run docker ps: {}", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("docker ps failed: {}", err.trim());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        println!("No torii-managed runner containers found.");
        println!("Spawn one with: `torii runner spawn`");
        return Ok(());
    }

    println!("{:<32} {:<10} {:<24} IMAGE", "NAME", "STATE", "STATUS");
    for line in lines {
        let cols: Vec<&str> = line.split('\t').collect();
        let (name, state, status, image) = (
            cols.first().copied().unwrap_or(""),
            cols.get(1).copied().unwrap_or(""),
            cols.get(2).copied().unwrap_or(""),
            cols.get(3).copied().unwrap_or(""),
        );
        let icon = match state {
            "running"    => "🟢",
            "exited"     => "⚪",
            "paused"     => "⏸",
            "restarting" => "🔄",
            _            => "·",
        };
        println!("{:<32} {} {:<8} {:<24} {}", name, icon, state, status, image);
    }
    Ok(())
}

/// Run `docker <args>` capturing stdout/stderr, surface stderr on
/// failure with the original status code. Used for the short ops
/// (start/stop) where there's no useful output to stream.
fn run_runner_docker(args: &[&str], label: &str) -> Result<()> {
    which_binary("docker").ok_or_else(|| anyhow::anyhow!(
        "`docker` not found on PATH."
    ))?;
    let out = std::process::Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| anyhow::anyhow!("spawn docker {}: {}", label, e))?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout);
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            println!("{}", trimmed);
        }
        println!("✓ {} ok", label);
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("docker {} failed: {}", label, err.trim());
    }
}

/// Same as `run_runner_docker` but inherits stdio. Used for `spawn`
/// (the docker run prints the container id) and `logs` (where the
/// user wants the stream live).
fn run_runner_docker_inherit(args: &[&str]) -> Result<()> {
    which_binary("docker").ok_or_else(|| anyhow::anyhow!(
        "`docker` not found on PATH."
    ))?;
    let status = std::process::Command::new("docker")
        .args(args)
        .status()
        .map_err(|e| anyhow::anyhow!("spawn docker: {}", e))?;
    if !status.success() {
        anyhow::bail!("docker {} exited {}", args.first().copied().unwrap_or("?"), status);
    }
    Ok(())
}

/// `torii runner init` — scaffold a starter config so `register` has
/// somewhere to land its block. Today only GitLab benefits (the
/// `gitlab-runner` binary expects a config.toml on first register).
/// GitHub's `./config.sh` writes its own files in the runner dir.
fn run_runner_init(platform: &str) -> Result<()> {
    match platform {
        "gitlab" => {
            let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no HOME"))?;
            let dir = home.join(".gitlab-runner");
            let path = dir.join("config.toml");
            if path.exists() {
                println!("ℹ  {} already exists; nothing to do.", path.display());
                return Ok(());
            }
            std::fs::create_dir_all(&dir)
                .map_err(|e| anyhow::anyhow!("mkdir {}: {}", dir.display(), e))?;
            // Minimal config that `gitlab-runner register` will append
            // its `[[runners]]` block into. The `concurrent` value can
            // be tuned later via `gitlab-runner` itself.
            let body = "concurrent = 1\ncheck_interval = 0\n\n[session_server]\n  session_timeout = 1800\n";
            std::fs::write(&path, body)
                .map_err(|e| anyhow::anyhow!("write {}: {}", path.display(), e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            println!("✅ Wrote starter {}.", path.display());
            println!("   Next: `torii runner register` to attach this host to the project.");
            Ok(())
        }
        "github" => {
            println!("ℹ  GitHub Actions runners don't need a scaffold — the runner tarball");
            println!("   carries its own `./config.sh`. Download it from");
            println!("   https://github.com/<owner>/<repo>/settings/actions/runners/new ,");
            println!("   unpack it, and run `torii runner register --runner-dir <path>`.");
            Ok(())
        }
        other => {
            anyhow::bail!(
                "`torii runner init` is GitHub + GitLab only. `{}` not implemented yet.",
                other
            );
        }
    }
}

/// Look up an executable by name on PATH. Returns the full path so
/// `Command::new(<that>)` is unambiguous, or None when missing.
fn which_binary(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

/// 0.7.35 — scope guard for `TORII_SIGN_OVERRIDE`. Set on construction,
/// restored on drop. Lets the `Save` handler force-enable / disable
/// GPG signing for a single commit without leaving the env var dirty
/// for anything that runs after (subprocesses for hooks, the mirror
/// sync that follows a `save -am … && sync`, …).
struct SignOverrideGuard {
    prev: Option<String>,
    touched: bool,
}

impl SignOverrideGuard {
    fn new(value: Option<bool>) -> Self {
        let prev = std::env::var("TORII_SIGN_OVERRIDE").ok();
        match value {
            Some(true)  => std::env::set_var("TORII_SIGN_OVERRIDE", "true"),
            Some(false) => std::env::set_var("TORII_SIGN_OVERRIDE", "false"),
            None        => return SignOverrideGuard { prev, touched: false },
        }
        SignOverrideGuard { prev, touched: true }
    }
}

impl Drop for SignOverrideGuard {
    fn drop(&mut self) {
        if !self.touched { return; }
        match &self.prev {
            Some(v) => std::env::set_var("TORII_SIGN_OVERRIDE", v),
            None    => std::env::remove_var("TORII_SIGN_OVERRIDE"),
        }
    }
}

/// `torii show --signature` — extract the GPG armor from a commit
/// object and print it, followed by the local verification verdict.
fn run_show_signature(repo: &GitRepo, object: Option<&str>) -> Result<()> {
    let r = repo.repository();
    let target = object.unwrap_or("HEAD");
    let oid = r
        .revparse_single(target)
        .map_err(|e| anyhow::anyhow!("`{}`: {}", target, e))?
        .id();

    let (sig_buf, payload_buf) = r.extract_signature(&oid, None)
        .map_err(|_| anyhow::anyhow!(
            "commit {} has no GPG signature attached. Use `torii sign {}` to add one.",
            &oid.to_string()[..8], target
        ))?;
    let sig_bytes: &[u8] = &sig_buf;
    let armor = std::str::from_utf8(sig_bytes)
        .map_err(|e| anyhow::anyhow!("signature is not valid UTF-8: {}", e))?;
    let payload: Vec<u8> = (&*payload_buf).to_vec();

    println!("commit: {}", oid);
    println!();
    println!("{}", armor.trim_end());
    println!();

    let program = r.workdir()
        .and_then(|wd| crate::config::ToriiConfig::load_local(wd).ok())
        .and_then(|c| c.git.gpg_program);

    match crate::gpg::verify(armor, &payload, program.as_deref())? {
        crate::gpg::VerifyStatus::Good { signer } => {
            println!("✓ Good signature from {}", signer);
        }
        crate::gpg::VerifyStatus::UnknownKey { key_id } => {
            let k = key_id.as_deref().unwrap_or("?");
            println!("? Unknown signer key {} — import it to verify locally.", k);
        }
        crate::gpg::VerifyStatus::Bad => {
            println!("✗ BAD signature — payload does not match.");
        }
        crate::gpg::VerifyStatus::Other(msg) => {
            println!("? {}", msg);
        }
    }
    Ok(())
}

/// `torii sign <oid|range>` — rewrite the named commits to include a
/// fresh `gpgsig` header. Rejects unreachable targets and dirty work
/// trees because rewriting in those conditions is rarely what users
/// actually want (and it's the kind of mistake we'd rather guard
/// against than recover from).
fn run_sign(target: Option<&str>, print_only: bool, yes: bool) -> Result<()> {
    let target = target.unwrap_or("HEAD");
    let repo = GitRepo::open(".")?;
    let r = repo.repository();

    // Cheap dirty-tree check (only when we're going to mutate).
    if !print_only {
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(false);
        if !r.statuses(Some(&mut opts))?.is_empty() {
            anyhow::bail!(
                "working tree is dirty — commit or stash first. \
                 (`torii sign` rewrites history; running with uncommitted \
                 changes makes the resulting state hard to reason about.)"
            );
        }
    }

    let tc = r.workdir()
        .and_then(|wd| crate::config::ToriiConfig::load_local(wd).ok())
        .unwrap_or_else(|| crate::config::ToriiConfig::load_global().unwrap_or_default());
    let key = tc.git.gpg_key.as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!(
            "git.gpg_key is not set. Configure with `torii config set git.gpg_key <KEY-ID>`."
        ))?;

    // Resolve the target as either a single commit or `A..B` range.
    let oids: Vec<git2::Oid> = if let Some((from, to)) = target.split_once("..") {
        let from_oid = r.revparse_single(from)?.id();
        let to_oid   = r.revparse_single(to)?.id();
        let mut walk = r.revwalk()?;
        walk.push(to_oid)?;
        walk.hide(from_oid)?;
        walk.flatten().collect()
    } else {
        vec![r.revparse_single(target)?.id()]
    };

    if oids.is_empty() {
        println!("(no commits in range)");
        return Ok(());
    }

    if print_only {
        for oid in &oids {
            let commit = r.find_commit(*oid)?;
            let buffer = r.commit_create_buffer(
                &commit.author(), &commit.committer(),
                commit.message().unwrap_or(""),
                &commit.tree()?,
                &commit.parents().collect::<Vec<_>>().iter().collect::<Vec<_>>(),
            )?;
            let armor = crate::gpg::sign_blob(&buffer, key, tc.git.gpg_program.as_deref())?;
            println!("# {}", oid);
            println!("{}", armor.trim_end());
            println!();
        }
        return Ok(());
    }

    if !yes {
        println!("About to rewrite {} commit(s) with new GPG signatures.", oids.len());
        println!("All affected commits' OIDs will change. Branches pointing at them get rewritten.");
        print!("Proceed? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Cancelled.");
            return Ok(());
        }
    }

    // Walk the range oldest-first so each rewrite's child can reuse
    // the parent's new OID instead of the original.
    let mut ordered = oids.clone();
    ordered.reverse();
    let mut remap: std::collections::HashMap<git2::Oid, git2::Oid> =
        std::collections::HashMap::new();

    for oid in &ordered {
        let commit = r.find_commit(*oid)?;
        let parents: Vec<git2::Commit> = commit.parents()
            .map(|p| {
                let real = remap.get(&p.id()).copied().unwrap_or(p.id());
                r.find_commit(real).map_err(crate::error::ToriiError::Git)
            })
            .collect::<crate::error::Result<Vec<_>>>()?;
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        let tree = commit.tree()?;
        let buffer = r.commit_create_buffer(
            &commit.author(), &commit.committer(),
            commit.message().unwrap_or(""),
            &tree, &parent_refs,
        )?;
        let buffer_str = std::str::from_utf8(&buffer)?;
        let armor = crate::gpg::sign_blob(&buffer, key, tc.git.gpg_program.as_deref())?;
        let new_oid = r.commit_signed(buffer_str, &armor, Some("gpgsig"))?;
        remap.insert(*oid, new_oid);
        println!("  {} → {}", &oid.to_string()[..8], &new_oid.to_string()[..8]);
    }

    // Move every branch whose tip is one of the rewritten oids onto
    // the new oid.
    let mut moved = 0usize;
    for b in r.branches(Some(git2::BranchType::Local))?.flatten() {
        let (br, _) = b;
        if let Ok(Some(reference_name)) = br.get().resolve().map(|rr| rr.name().map(String::from)) {
            let _ = reference_name;
        }
        let tip = br.get().target();
        if let (Some(t), Some(name)) = (tip, br.name().ok().flatten()) {
            if let Some(new_oid) = remap.get(&t) {
                r.reference(&format!("refs/heads/{}", name), *new_oid, true,
                            "torii sign — re-sign history")?;
                moved += 1;
            }
        }
    }
    if moved > 0 {
        println!("Moved {} branch tip(s) to the new signed commit(s).", moved);
    }
    println!("✓ Signed {} commit(s).", oids.len());
    Ok(())
}

fn run_scan(history: bool) -> Result<()> {
    let repo_path = std::path::Path::new(".");
    if history {
        println!("🔍 Scanning full git history for sensitive data...\n");
        let results = scanner::scan_history(repo_path)?;
        if results.is_empty() {
            println!("✅ No sensitive data found in history.");
        } else {
            println!("⚠️  Found sensitive data in {} commit(s):\n", results.len());
            for (commit, findings) in &results {
                println!("  📌 {}", commit);
                for f in findings {
                    println!("     {}:{} — {}", f.file, f.line, f.pattern_name);
                    println!("     {}", f.preview);
                }
                println!();
            }
            println!("💡 To clean history: torii history rebase <base> --todo-file <plan>");
        }
    } else {
        println!("🔍 Scanning staged files for sensitive data...\n");
        let findings = scanner::scan_staged(repo_path)?;
        if findings.is_empty() {
            println!("✅ No sensitive data detected in staged files.");
        } else {
            println!("⚠️  Found {} issue(s):\n", findings.len());
            for f in &findings {
                println!("  {}:{} — {}", f.file, f.line, f.pattern_name);
                println!("  {}\n", f.preview);
            }
            println!("💡 Tip: use .env.example for placeholder values.");
        }
    }
    Ok(())
}

fn run_commit_scan(policy_path: Option<&std::path::Path>, limit: usize) -> Result<()> {
    use crate::commit_scan::{CompiledCommitPolicy, default_policy_path, scan_repo};
    let repo = git2::Repository::discover(".").map_err(|e| anyhow::anyhow!("not a repo: {}", e))?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("bare repos can't host policies/commits.toml"))?
        .to_path_buf();
    let path = match policy_path {
        Some(p) => p.to_path_buf(),
        None => default_policy_path(&workdir),
    };
    let policy = match CompiledCommitPolicy::load(&path)? {
        Some(p) => p,
        None => {
            println!("ℹ️  No commit policy found at {}.", path.display());
            println!("    Run `torii init` (or create the file manually) to add one.");
            return Ok(());
        }
    };
    let violations = scan_repo(&repo, &policy, limit)?;
    if violations.is_empty() {
        println!("✅ {} commits scanned, no policy violations.", limit);
        return Ok(());
    }
    println!("❌ {} violation(s) across the last {} commits:\n", violations.len(), limit);
    for v in &violations {
        println!("  {} \"{}\"", v.commit_short, v.subject);
        println!("      [{}] {}", v.rule, v.detail);
    }
    println!();
    std::process::exit(1);
}

/// Walk the object database, mark everything reachable from refs + reflogs +
/// the index + HEAD, then list / inspect / restore the leftover unreachable
/// objects. Recovery aid after destructive ops (reset --hard, force-push,
/// rebase that drops commits, etc.).
fn run_fsck(
    show: Option<&str>,
    restore: Option<&str>,
    to: Option<&std::path::Path>,
) -> Result<()> {
    use std::collections::HashSet;
    let repo = git2::Repository::discover(".")
        .map_err(|e| anyhow::anyhow!("not a repo: {}", e))?;

    // --- branch: --show <oid>
    if let Some(oid_str) = show {
        let oid = resolve_oid(&repo, oid_str)?;
        let odb = repo.odb().map_err(|e| anyhow::anyhow!("odb: {}", e))?;
        let obj = odb.read(oid).map_err(|e| anyhow::anyhow!("read {}: {}", oid, e))?;
        match obj.kind() {
            git2::ObjectType::Blob => {
                use std::io::Write;
                std::io::stdout().write_all(obj.data()).ok();
            }
            git2::ObjectType::Commit => {
                let commit = repo
                    .find_commit(oid)
                    .map_err(|e| anyhow::anyhow!("find commit {}: {}", oid, e))?;
                println!("commit {}", oid);
                if let Some(t) = commit.tree_id().to_string().get(..) {
                    println!("tree   {}", t);
                }
                for p in commit.parent_ids() {
                    println!("parent {}", p);
                }
                let a = commit.author();
                println!("author {} <{}>", a.name().unwrap_or(""), a.email().unwrap_or(""));
                println!();
                println!("{}", commit.message().unwrap_or(""));
            }
            git2::ObjectType::Tree => {
                let tree = repo
                    .find_tree(oid)
                    .map_err(|e| anyhow::anyhow!("find tree {}: {}", oid, e))?;
                println!("tree {} ({} entries)", oid, tree.len());
                for e in tree.iter() {
                    println!(
                        "  {:o} {} {}",
                        e.filemode(),
                        e.id(),
                        e.name().unwrap_or("?")
                    );
                }
            }
            other => println!("object {} kind={:?} size={}", oid, other, obj.len()),
        }
        return Ok(());
    }

    // --- branch: --restore <oid> --to <path>
    if let Some(oid_str) = restore {
        let dest = to.ok_or_else(|| anyhow::anyhow!("--restore requires --to <path>"))?;
        let oid = resolve_oid(&repo, oid_str)?;
        let blob = repo
            .find_blob(oid)
            .map_err(|e| anyhow::anyhow!("not a blob {}: {}", oid, e))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(dest, blob.content())
            .map_err(|e| anyhow::anyhow!("write {}: {}", dest.display(), e))?;
        println!(
            "✅ Restored {} bytes from {} → {}",
            blob.content().len(),
            oid,
            dest.display()
        );
        return Ok(());
    }

    // --- default: list unreachable
    let mut reachable: HashSet<git2::Oid> = HashSet::new();

    // Refs (branches, tags, remotes)
    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            if let Some(target) = r.target() {
                mark_commit_tree(&repo, target, &mut reachable);
            }
        }
    }
    // HEAD (covers detached HEAD case)
    if let Ok(head) = repo.head() {
        if let Some(target) = head.target() {
            mark_commit_tree(&repo, target, &mut reachable);
        }
    }
    // Reflog of HEAD + every branch — protects work that survived
    // ref deletion but still has a reflog entry.
    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            let Some(name) = r.name() else { continue };
            if let Ok(rl) = repo.reflog(name) {
                for entry in rl.iter() {
                    mark_commit_tree(&repo, entry.id_old(), &mut reachable);
                    mark_commit_tree(&repo, entry.id_new(), &mut reachable);
                }
            }
        }
    }
    if let Ok(rl) = repo.reflog("HEAD") {
        for entry in rl.iter() {
            mark_commit_tree(&repo, entry.id_old(), &mut reachable);
            mark_commit_tree(&repo, entry.id_new(), &mut reachable);
        }
    }
    // Index — protects staged blobs not yet committed
    if let Ok(index) = repo.index() {
        for e in index.iter() {
            reachable.insert(e.id);
        }
    }

    // Walk ODB, collect unreachable.
    let odb = repo.odb().map_err(|e| anyhow::anyhow!("odb: {}", e))?;
    let mut unreachable: Vec<(git2::Oid, git2::ObjectType, usize)> = Vec::new();
    odb.foreach(|oid| {
        if !reachable.contains(oid) {
            if let Ok(obj) = odb.read(*oid) {
                unreachable.push((*oid, obj.kind(), obj.len()));
            }
        }
        true
    })
    .map_err(|e| anyhow::anyhow!("odb walk: {}", e))?;

    if unreachable.is_empty() {
        println!("✅ No unreachable objects.");
        return Ok(());
    }

    // Sort: commits first, then trees, then blobs by size desc
    unreachable.sort_by(|a, b| {
        let ka = type_rank(a.1);
        let kb = type_rank(b.1);
        ka.cmp(&kb).then(b.2.cmp(&a.2))
    });

    let total: usize = unreachable.iter().map(|(_, _, s)| *s).sum();
    println!(
        "🔍 {} unreachable object(s), {} bytes total\n",
        unreachable.len(),
        total
    );
    println!("{:<8} {:7} {:>10}  preview", "type", "oid", "size");
    println!("{}", "─".repeat(60));

    for (oid, kind, size) in &unreachable {
        let short: String = oid.to_string().chars().take(7).collect();
        let kind_str = match kind {
            git2::ObjectType::Commit => "commit",
            git2::ObjectType::Tree => "tree",
            git2::ObjectType::Blob => "blob",
            git2::ObjectType::Tag => "tag",
            _ => "any",
        };
        let preview = preview_object(&repo, *oid, *kind);
        println!(
            "{:<8} {:7} {:>10}  {}",
            kind_str, short, size, preview
        );
    }
    println!();
    println!("Inspect: torii history fsck --show <oid>");
    println!("Restore: torii history fsck --restore <oid> --to <path>");
    Ok(())
}

/// Resolve a (possibly short) hex OID to a full Oid by walking the ODB.
/// Accepts 4..=40 hex chars, errors on ambiguous prefixes.
fn resolve_oid(repo: &git2::Repository, hex: &str) -> Result<git2::Oid> {
    if hex.len() == 40 {
        return git2::Oid::from_str(hex)
            .map_err(|e| anyhow::anyhow!("bad oid {}: {}", hex, e));
    }
    if hex.len() < 4 {
        anyhow::bail!("oid prefix too short (need ≥4 chars): {}", hex);
    }
    let odb = repo.odb().map_err(|e| anyhow::anyhow!("odb: {}", e))?;
    let mut matches: Vec<git2::Oid> = Vec::new();
    odb.foreach(|oid| {
        if oid.to_string().starts_with(hex) {
            matches.push(*oid);
        }
        true
    })
    .map_err(|e| anyhow::anyhow!("odb walk: {}", e))?;
    match matches.len() {
        0 => anyhow::bail!("no object matches prefix {}", hex),
        1 => Ok(matches[0]),
        n => anyhow::bail!("ambiguous prefix {} ({} matches)", hex, n),
    }
}

fn type_rank(t: git2::ObjectType) -> u8 {
    match t {
        git2::ObjectType::Commit => 0,
        git2::ObjectType::Tag => 1,
        git2::ObjectType::Tree => 2,
        git2::ObjectType::Blob => 3,
        _ => 4,
    }
}

fn mark_commit_tree(
    repo: &git2::Repository,
    oid: git2::Oid,
    set: &mut std::collections::HashSet<git2::Oid>,
) {
    if !set.insert(oid) {
        return;
    }
    let Ok(obj) = repo.find_object(oid, None) else { return };
    match obj.kind() {
        Some(git2::ObjectType::Commit) => {
            if let Ok(commit) = obj.peel_to_commit() {
                set.insert(commit.tree_id());
                if let Ok(tree) = commit.tree() {
                    mark_tree(repo, &tree, set);
                }
                for p in commit.parent_ids() {
                    mark_commit_tree(repo, p, set);
                }
            }
        }
        Some(git2::ObjectType::Tag) => {
            if let Ok(tag) = obj.peel_to_tag() {
                mark_commit_tree(repo, tag.target_id(), set);
            }
        }
        Some(git2::ObjectType::Tree) => {
            if let Ok(tree) = obj.peel_to_tree() {
                mark_tree(repo, &tree, set);
            }
        }
        _ => {}
    }
}

fn mark_tree(
    repo: &git2::Repository,
    tree: &git2::Tree,
    set: &mut std::collections::HashSet<git2::Oid>,
) {
    for entry in tree.iter() {
        let id = entry.id();
        if !set.insert(id) {
            continue;
        }
        if entry.kind() == Some(git2::ObjectType::Tree) {
            if let Ok(sub) = repo.find_tree(id) {
                mark_tree(repo, &sub, set);
            }
        }
    }
}

fn preview_object(repo: &git2::Repository, oid: git2::Oid, kind: git2::ObjectType) -> String {
    match kind {
        git2::ObjectType::Commit => repo
            .find_commit(oid)
            .ok()
            .and_then(|c| c.summary().map(|s| s.to_string()))
            .unwrap_or_default(),
        git2::ObjectType::Blob => repo
            .find_blob(oid)
            .ok()
            .and_then(|b| std::str::from_utf8(b.content()).ok().map(|s| s.to_string()))
            .map(|s| s.lines().next().unwrap_or("").chars().take(50).collect())
            .unwrap_or_else(|| "<binary>".to_string()),
        git2::ObjectType::Tree => repo
            .find_tree(oid)
            .ok()
            .map(|t| format!("({} entries)", t.len()))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn handle_ignore(action: &IgnoreCommands) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let repo_root = std::path::Path::new(".");
    let public = repo_root.join(".toriignore");
    let local = repo_root.join(".toriignore.local");

    fn append_section(path: &std::path::Path, section: &str, line: &str) -> Result<()> {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let header = format!("[{}]", section);
        // Active header = line equal to `[section]` after trimming, NOT commented.
        let has_active_header = existing.lines().any(|l| l.trim() == header);
        let mut out = OpenOptions::new().create(true).append(true).open(path)?;
        if !has_active_header {
            if !existing.is_empty() && !existing.ends_with('\n') {
                writeln!(out)?;
            }
            writeln!(out)?;
            writeln!(out, "{}", header)?;
        }
        writeln!(out, "{}", line)?;
        Ok(())
    }

    match action {
        IgnoreCommands::Add { pattern, local: use_local } => {
            let target = if *use_local { &local } else { &public };
            let existing = std::fs::read_to_string(target).unwrap_or_default();
            let mut f = OpenOptions::new().create(true).append(true).open(target)?;
            if !existing.is_empty() && !existing.ends_with('\n') {
                writeln!(f)?;
            }
            writeln!(f, "{}", pattern)?;
            let label = if *use_local { ".toriignore.local (private)" } else { ".toriignore" };
            println!("✅ Added `{}` to {}", pattern, label);
        }
        IgnoreCommands::Secret { pattern, name, public: use_public } => {
            // Validate regex before writing
            regex::Regex::new(pattern)
                .map_err(|e| anyhow::anyhow!("invalid regex: {}", e))?;
            let line = match name {
                Some(n) => format!("deny: {}  # {}", pattern, n),
                None => format!("deny: {}", pattern),
            };
            let target = if *use_public { &public } else { &local };
            append_section(target, "secrets", &line)?;
            let label = if *use_public {
                ".toriignore (public — visible in repo)"
            } else {
                ".toriignore.local (private — never committed)"
            };
            println!("✅ Added secret rule to {}", label);
            if *use_public {
                println!("⚠️  Consider --local instead: secret-pattern shape can aid recon if repo leaks");
            }
        }
        IgnoreCommands::List => {
            let ti = crate::toriignore::ToriIgnore::load(repo_root)?;
            println!("📋 Effective .toriignore rules (public + local merged)\n");
            println!("Paths ({}):", ti.patterns().len());
            for p in ti.patterns() { println!("  {}", p); }
            println!("\nSecrets ({}):", ti.secrets.len());
            for s in &ti.secrets { println!("  {} → {}", s.name, s.regex.as_str()); }
            if ti.size.max_bytes.is_some() || ti.size.warn_bytes.is_some() {
                println!("\nSize:");
                if let Some(m) = ti.size.max_bytes { println!("  max: {} bytes", m); }
                if let Some(w) = ti.size.warn_bytes { println!("  warn: {} bytes", w); }
            }
            if !ti.hooks.pre_save.is_empty() || !ti.hooks.pre_sync.is_empty() {
                println!("\nHooks:");
                for h in &ti.hooks.pre_save { println!("  pre-save: {}", h); }
                for h in &ti.hooks.pre_sync { println!("  pre-sync: {}", h); }
                for h in &ti.hooks.post_save { println!("  post-save: {}", h); }
                for h in &ti.hooks.post_sync { println!("  post-sync: {}", h); }
            }
            if local.exists() {
                println!("\n🔒 .toriignore.local present (private, gitignored)");
            }
        }
    }
    Ok(())
}
