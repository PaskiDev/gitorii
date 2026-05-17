# Gitorii (torii) — Command Reference

> Install: `cargo install gitorii`  
> Binary: `torii`  
> Version: 0.1.16

---

## Quick Start

```bash
torii init                        # New repo
torii save -am "feat: add login"  # Stage all + commit
torii sync                        # Pull + push
torii status                      # What changed
```

---

## `torii init`

Initialize a new git repository.

```bash
torii init                          # Current directory
torii init --path ~/projects/myrepo # Specific path
```

---

## `torii save`

Simplified commit. Replaces `git add` + `git commit`.

```bash
torii save -m "fix: null check"              # Commit staged changes
torii save -am "feat: add login"             # Stage all + commit
torii save src/auth.rs -m "fix: token"       # Stage specific file + commit
torii save --amend -m "fix: typo"            # Amend last commit
torii save --revert abc1234 -m "revert"      # Revert a commit
torii save --reset HEAD~1 --reset-mode soft  # Undo last commit, keep changes staged
torii save --reset HEAD~1 --reset-mode mixed # Undo last commit, unstage changes (default)
torii save --reset HEAD~1 --reset-mode hard  # Undo last commit, discard changes
```

| Flag | Description |
|------|-------------|
| `-m` | Commit message (required) |
| `-a` / `--all` | Stage all changes before committing |
| `--amend` | Amend previous commit |
| `--revert <hash>` | Revert a specific commit |
| `--reset <hash>` | Reset to a specific commit |
| `--reset-mode` | `soft` / `mixed` / `hard` (default: mixed) |

---

## `torii sync`

Pull + push in one command. Also integrates branches.

```bash
torii sync                   # Pull then push
torii sync --pull            # Pull only
torii sync --push            # Push only
torii sync --force           # Force push (rewrites remote history)
torii sync --fetch           # Fetch remote refs without merging

# Branch integration
torii sync main              # Integrate main into current branch (smart)
torii sync main --merge      # Force merge strategy
torii sync main --rebase     # Force rebase strategy
torii sync main --preview    # Preview without executing
```

---

## `torii status`

Show working tree status: staged, unstaged, and untracked files.

```bash
torii status
```

---

## `torii log`

Show commit history.

```bash
torii log                          # Last 10 commits
torii log -n 50                    # Last 50 commits
torii log --oneline                # One line per commit
torii log --oneline --graph        # Compact graph view
torii log --author "Alice"         # Filter by author
torii log --since 2024-01-01       # Commits after date
torii log --until 2024-12-31       # Commits before date
torii log --grep "feat"            # Filter by message pattern
torii log --stat                   # File change stats per commit
```

---

## `torii diff`

Show changes.

```bash
torii diff            # Unstaged changes
torii diff --staged   # Staged changes (ready to commit)
torii diff --last     # Changes in last commit
```

---

## `torii branch`

Manage branches.

```bash
torii branch                          # List local branches
torii branch --all                    # List local + remote branches
torii branch <name> -c                # Create and switch to branch
torii branch <name>                   # Switch to existing branch
torii branch -d <name>                # Delete branch
torii branch --rename <new-name>      # Rename current branch
```

**Examples:**
```bash
torii branch feature/login -c
torii branch fix/null-pointer -c
torii branch develop
```

---

## `torii clone`

Clone a repository. Supports platform shorthands and full URLs.

```bash
torii clone <platform> <user>/<repo>                      # Auto SSH/HTTPS
torii clone <platform> <user>/<repo> --protocol https     # Force HTTPS
torii clone <platform> <user>/<repo> -d <directory>       # Custom directory
torii clone https://github.com/<user>/<repo>.git          # Full URL
torii clone git@github.com:<user>/<repo>.git              # SSH URL
```

**Examples:**
```bash
torii clone github torvalds/linux
torii clone gitlab paskidev/gitorii-api --protocol https
torii clone github torvalds/linux -d my-linux
```

**Platforms:** `github`, `gitlab`, `codeberg`, `bitbucket`, `gitea`, `forgejo`

Protocol auto-detected: SSH if keys present, HTTPS otherwise.  
Override: `torii config set mirror.default_protocol https`

---

## `torii tag`

Manage tags and releases.

```bash
torii tag list                               # List all tags
torii tag create <version> -m "<message>"    # Create annotated tag
torii tag delete <version>                   # Delete tag
torii tag push <version>                     # Push specific tag
torii tag push                               # Push all tags
torii tag show <version>                     # Show tag details

# Auto-release from conventional commits
torii tag create --release                  # Auto-bump version
torii tag create --release --bump minor     # Force minor bump
torii tag create --release --dry-run        # Preview without creating
```

**Auto-bump rules (Conventional Commits):**

| Commit type | Version bump |
|-------------|-------------|
| `feat:` | minor (0.1.0 → 0.2.0) |
| `fix:` / `perf:` | patch (0.1.0 → 0.1.1) |
| `feat!:` | major (0.1.0 → 1.0.0) |

---

## `torii snapshot`

Save and restore work-in-progress states. Unlike git stash, snapshots are named, persistent, and don't affect your working tree until explicitly restored.

```bash
torii snapshot create -n "before-refactor"  # Named snapshot
torii snapshot list                          # List all snapshots
torii snapshot restore <id>                  # Restore a snapshot
torii snapshot delete <id>                   # Delete a snapshot

# Stash (quick save/restore)
torii snapshot stash                         # Stash current work
torii snapshot stash -u                      # Include untracked files
torii snapshot unstash                       # Restore latest stash
torii snapshot unstash <id> --keep           # Restore but keep stash

# Undo
torii snapshot undo                          # Undo last operation

# Auto-snapshot config
torii snapshot config                        # Show auto-snapshot settings
```

---

## `torii mirror`

Mirror your repo across multiple platforms simultaneously.

```bash
# Setup
torii mirror add <platform> user <username> <repo> --primary   # Set primary (source of truth)
torii mirror add <platform> user <username> <repo>             # Add replica mirror (default)

# Sync
torii mirror sync                   # Push to all replicas
torii mirror sync --force           # Force push to all replicas

# Manage
torii mirror list                   # List configured mirrors
torii mirror promote github user    # Promote a mirror to primary
torii mirror remove github user     # Remove a mirror

# Auto-fetch
torii mirror autofetch --enable --interval 30m   # Auto-fetch every 30 min
torii mirror autofetch --disable                  # Disable
torii mirror autofetch --status                   # Show status
```

**Platforms:** `github`, `gitlab`, `codeberg`, `bitbucket`, `gitea`, `forgejo`

---

## `torii show`

Show details of a commit, tag, or file.

```bash
torii show                          # HEAD commit with diff
torii show <hash>                   # Specific commit
torii show <tag>                    # Tag details
torii show <file> --blame           # Line-by-line change history
torii show <file> --blame -L 10,20  # Blame specific range
```

---

## `torii config check-ssh`

Verify SSH key configuration and print setup instructions if needed.

```bash
torii config check-ssh
```

---

## `torii blame`, `torii scan`, `torii cherry-pick`

Common file inspection and commit operations are available at the top level.

```bash
torii blame <file>                  # Line-by-line change history
torii blame <file> -L 10,20         # Specific line range

torii scan                          # Scan staged files for secrets
torii scan --history                # Scan entire git history

torii cherry-pick <hash>            # Apply commit to current branch
torii cherry-pick --continue        # Resume after resolving conflicts
torii cherry-pick --abort           # Abort an in-progress cherry-pick
```

---

## `torii history`

Maintenance operations on existing history.

```bash
# Rebase
torii history rebase main              # Rebase onto main
torii history rebase -i HEAD~5         # Interactive rebase last 5 commits
torii history rebase --root            # Rebase from the root commit (squash initial)
torii history rebase --continue        # Continue after resolving conflicts
torii history rebase --abort           # Abort rebase
torii history rebase --skip            # Skip current patch

# Rewrite / cleanup
torii history rewrite "<start-date>" "<end-date>"  # Rewrite commit dates
torii history remove-file <file>                   # Purge file from all commits
torii history clean                                # GC + expire reflog

# Identity rewrite (reauthor / mailmap)
torii history reauthor --old "Old <a@x>" --new "New <b@y>"
torii history reauthor --old oldname    --new "New <b@y>"   # match by name only
torii history reauthor --old a@x        --new "New <b@y>"   # match by email only
torii history reauthor ... --committer                       # also rewrite committer
torii history reauthor ... --since v0.6.0                    # limit to a range
torii history reauthor ... --dry-run                         # preview, no changes
torii history reauthor ... --no-snapshot                     # skip safety snapshot
torii history reauthor ... --allow-dirty                     # allow uncommitted changes

torii history mailmap apply                                  # apply .mailmap at repo root
torii history mailmap apply --file other.mailmap             # alternative path
torii history mailmap apply --since v0.6.0 --dry-run         # preview a range

# Inspection (also exposed as flags)
torii log --reflog                     # HEAD movement history
torii sync --verify                    # Compare local vs remote HEAD
```

### Identity rewrite details

`reauthor` and `mailmap apply` share the same engine:

- A **safety snapshot** is taken before rewriting (unless `--no-snapshot`).
  Revert with `torii snapshot restore <id>`.
- **Annotated tags** get a new tagger that matches the rewrite (always — the
  point of reauthor is identity reconciliation, leaving a stale tagger would
  contradict that intent).
- **Commit/author timestamps are preserved.** Only *who* changes, never
  *when*. Use `torii history rewrite` to change dates.
- **GPG signatures invalidate** after rewrite — they're computed over the
  old author. Re-sign manually after the rewrite if your repo enforces
  signed commits (`git.sign_commits = true`).
- **Aborts on pending operations** (merge/rebase/cherry-pick in flight) and
  on a dirty working tree (override with `--allow-dirty`).
- **`--since <rev>`** limits the walk: only commits reachable from HEAD that
  are *not* reachable from `<rev>` are touched.

Mailmap format follows [git's standard](https://git-scm.com/docs/gitmailmap):

```text
Proper Name <commit@email.xx>
<proper@email.xx> <commit@email.xx>
Proper Name <proper@email.xx> <commit@email.xx>
Proper Name <proper@email.xx> Commit Name <commit@email.xx>
```

After rewriting, history is diverged from the remote — push with
`torii sync --push --force` (and coordinate with collaborators).

### Secret scanner patterns

Detects automatically:
- Private keys (PEM)
- JWT tokens
- AWS access/secret keys
- GitHub / GitLab tokens (`ghp_`, `glpat-`, etc.)
- Generic API keys / passwords
- Database connection strings with credentials
- Stripe keys (`sk_live_`, `pk_live_`)
- Twilio / SendGrid / Brevo keys

Skips: example files (`.env.example`), i18n files, binary files, lock files.

---

## `torii config`

Manage global and local configuration.

```bash
torii config list                               # All config values
torii config list --local                       # Local repo config
torii config get user.name                      # Get a value
torii config set user.name "Alice"              # Set global value
torii config set user.email "a@b.com" --local  # Set local value
torii config edit                               # Open in editor
torii config reset                              # Reset to defaults
```

**Available keys:**

| Key | Description |
|-----|-------------|
| `user.name` | Git author name |
| `user.email` | Git author email |
| `user.editor` | Preferred editor |
| `auth.github_token` | GitHub personal access token |
| `auth.gitlab_token` | GitLab personal access token |
| `auth.gitea_token` | Gitea token |
| `auth.forgejo_token` | Forgejo token |
| `auth.codeberg_token` | Codeberg token |
| `git.default_branch` | Default branch name |
| `git.sign_commits` | GPG sign commits |
| `git.pull_rebase` | Rebase on pull |
| `mirror.default_protocol` | `ssh` or `https` |
| `mirror.autofetch_enabled` | Auto-fetch from mirrors |
| `snapshot.auto_enabled` | Auto-snapshots |
| `snapshot.auto_interval_minutes` | Auto-snapshot interval |
| `ui.colors` | Colored output |
| `ui.emoji` | Emoji in output |
| `ui.verbose` | Verbose mode |
| `ui.date_format` | Date format string |

---

## `torii auth`

Manage the gitorii.com API key used by cloud features (CI transpile, etc.). The key is stored locally at `~/.config/torii/auth.toml` (chmod 600). `TORII_API_KEY` env var overrides the stored key for the current process.

```bash
torii auth login                       # prompt for an API key and save it
torii auth login --key gitorii_sk_…    # save a key non-interactively
torii auth status                      # show org / plan / seats tied to the key
torii auth whoami                      # alias of status
torii auth logout                      # forget the local key
torii auth login --endpoint http://localhost:8080   # self-hosted / dev backend
```

Generate a key in the dashboard: <https://gitorii.com/dashboard/api-keys>.

---

## `torii remote`

Create and manage remote repositories via platform APIs (requires auth token configured).

```bash
torii remote create <platform> <repo> --public            # Create public repo
torii remote create <platform> <repo> --private           # Create private repo
torii remote create <platform> <repo> --private --push    # Create + push current branch
torii remote delete <platform> <owner> <repo> --yes       # Delete repo
torii remote visibility <platform> <owner> <repo> --public
torii remote visibility <platform> <owner> <repo> --private
torii remote configure <platform> <owner> <repo> --default-branch main
torii remote info <platform> <owner> <repo>               # Show repo details
torii remote list <platform>                               # List your repos
```

---

## Multi-platform repo creation

`torii remote create` accepts a comma-separated list of platforms.

```bash
torii remote create github,gitlab,codeberg <name> --private
torii remote create github,gitlab <name> --public --push
torii remote delete github,gitlab <owner> <name> --yes
```

---

## `torii workspace`

Group several repositories under a logical "workspace" and run common operations across all of them at once. Workspace definitions are stored in `~/.config/torii/workspaces.toml`.

```bash
torii workspace add work ~/repos/api          # add a repo to a workspace
torii workspace add work ~/repos/frontend     # add another repo to the same workspace
torii workspace list                          # list all workspaces and their repos
torii workspace status work                   # status across every repo in the workspace
torii workspace save work -m "wip"            # commit (staged) across all repos
torii workspace save work -am "wip"           # stage all + commit across all repos
torii workspace sync work                     # pull + push every repo
torii workspace sync work --force             # force-push every repo
torii workspace remove work ~/repos/api       # remove a single repo from the workspace
torii workspace delete work                   # delete the workspace entirely (repos stay on disk)
```

---

## `torii pr`

Manage pull requests / merge requests against the platform configured for the current repository (GitHub, GitLab, Codeberg, etc.). Requires the corresponding `auth.<platform>_token` to be set via `torii config set`.

```bash
torii pr list                                  # list open PRs
torii pr list --state closed                   # list closed PRs
torii pr list --state merged                   # list merged PRs
torii pr list --state all                      # list every PR
torii pr create -t "feat: login" -b main       # create a PR (head = current branch)
torii pr create -t "feat: login" --head feat/x # explicit head branch
torii pr create -t "wip" --draft               # create as draft
torii pr create -t "fix" -d "long description" # with description body
torii pr merge 42                              # merge PR #42 (merge commit)
torii pr merge 42 --method squash              # squash merge
torii pr merge 42 --method rebase              # rebase merge
torii pr close 42                              # close without merging
torii pr checkout 42                           # checkout the branch of PR #42
torii pr open 42                               # open PR #42 in the browser
```

---

## `torii issue`

Manage issues on the platform configured for the current repository. Same auth requirements as `torii pr`.

```bash
torii issue list                               # list open issues
torii issue list --state closed                # list closed issues
torii issue list --state all                   # list every issue
torii issue create -t "bug: crash on save"     # create a minimal issue
torii issue create -t "title" -d "description" # with description body
torii issue close 42                           # close issue #42
torii issue comment 42 -m "Fixed in v0.6.6"   # add a comment
```

---

## `torii ignore`

Manage `.toriignore` rules without hand-editing the file. Paths default to the public `.toriignore` (committed). Secret regex rules default to `.toriignore.local` (machine-private — never committed) because the mere existence of a rule for, say, an internal token format can aid recon if the public repo leaks.

```bash
torii ignore add 'build/'                          # add path to public .toriignore
torii ignore add --local 'internal/billing/'       # add path to .toriignore.local
torii ignore secret 'AKIA[0-9A-Z]{16}' --name AWS  # add secret regex (defaults to .local)
torii ignore secret '<pattern>' --public           # put regex in the committed .toriignore
torii ignore list                                  # show effective rules (public + local merged)
```

---

## `torii tui`

Launch the interactive terminal UI. Shows repository status, log, branch list, file navigation, snapshots, mirrors, PRs, issues and more — all keyboard-driven. Useful for browsing without remembering subcommands.

```bash
torii tui
```

---

## `torii worktree`

Multiple working copies of the same repository, each on its own branch, sharing the underlying object database. Useful for hot-fixes ("don't disturb my in-progress feature branch") or reviewing a PR without stashing.

```bash
torii worktree add -b feature/auth                 # new branch + worktree at ../<repo>-feature-auth/
torii worktree add ../hotfix -b release/0.7        # new branch at explicit path
torii worktree add ../hotfix release/0.7           # check out existing branch in a worktree
torii worktree list                                # every worktree with branch + clean/dirty
torii worktree remove ../hotfix                    # delete worktree (snapshot taken first)
torii worktree remove ../hotfix --force            # remove even if dirty
torii worktree remove ../hotfix --no-snapshot      # skip the safety snapshot
torii worktree prune                               # clean up metadata of deleted worktrees
torii worktree open ../hotfix                      # launch $SHELL inside the worktree
```

### Default path resolution

If you omit `<path>`, the directory is derived from `worktree.base_dir` (default `..`) + `<repo>-<branch-sanitized>/`. Examples:

```bash
torii config set worktree.base_dir ..              # default: sibling of the main repo
torii config set worktree.base_dir ~/worktrees     # central directory
torii config set worktree.base_dir /tmp/wt          # somewhere disposable
```

Branch slashes are replaced with `-` in the directory name (`feature/auth` → `feature-auth`).

### Safety behaviour

- **Snapshot before remove**: `remove` always takes a snapshot of the worktree before deletion so you can restore it via `torii snapshot restore <id>`. Skip with `--no-snapshot`. The snapshot may fail silently if the worktree's `.git` is a link file (current snapshot module limitation); the remove proceeds either way and a warning is printed.
- **Dirty refusal**: `remove` refuses if the worktree has uncommitted changes. Pass `--force` to drop them. Combine with `--no-snapshot` to also skip the safety net.
- **Pending operations**: `add` and `remove` will surface libgit2 errors if the target repo state is mid-rebase/merge/etc — no special handling, the underlying message is shown.
- **Unique paths**: `add` aborts if `<path>` already exists. Pick a different one or remove it first.

### Comparison vs `git worktree`

| Concept | git | torii |
|---------|-----|-------|
| Default path | always required | derived from `worktree.base_dir` + branch name |
| List | per-worktree text, no status | one-line per worktree with branch + clean/dirty |
| Remove safety | `--force` required if dirty | same + automatic snapshot |
| `open` | (not present) | launches `$SHELL` in the worktree directory |
| Lock / move / repair | yes | not yet in 0.6.8 — coming later |

---

## `.toriignore`

Extends `.gitignore` with optional `[sections]` for secrets, size limits and hooks. Path patterns sync to `.git/info/exclude` automatically so they are respected by every git operation without leaking ignore rules into the repo.

```ini
# path patterns (default section, .gitignore syntax)
build/
*.log

[secrets]
# regex rules used by `torii scan` / pre-save checks
name = "AWS access key" ; pattern = "AKIA[0-9A-Z]{16}"

[size]
# hard / soft limits applied on save
max  = "10MB"   ; warn = "1MB"
exclude = ["fixtures/*.bin"]

[hooks]
pre-save  = ["cargo fmt --check"]
pre-sync  = ["cargo test --quiet"]
post-save = ["echo done"]
```

A companion `.toriignore.local` is auto-excluded from git and machine-private. Use it for rules whose mere existence would aid recon if the public repo leaks (proprietary secret formats, internal paths, etc). When both files exist, public + local are merged at load time (local wins on size limits).

Hooks are gated by a one-time trust prompt; see [SECURITY.md](SECURITY.md) for the threat model. Use `torii ignore` to edit rules without touching the files by hand.

---

## System dependencies

> **Important:** Gitorii requires these system libraries to build from source:

| Platform | Command |
|----------|---------|
| Ubuntu/Debian | `apt install perl libssl-dev pkg-config` |
| Fedora/RHEL | `dnf install perl openssl-devel pkgconfig` |
| macOS | `brew install openssl pkg-config` |
| Arch | `pacman -S perl openssl pkgconf` |

---

## License

TSAL-1.0 — Free for personal and non-production use. Commercial use requires a license.  
See [LICENSE](LICENSE) for details. Converts to Apache 2.0 after 10 years.
