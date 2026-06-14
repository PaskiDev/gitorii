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
| `--amend` | Amend previous commit (preserves the original author date) |
| `--revert <hash>` | Revert a specific commit |
| `--reset <hash>` | Reset to a specific commit |
| `--reset-mode` | `soft` / `mixed` / `hard` (default: mixed) |
| `--date <when>` | Author date for the commit (any git date format) |
| `--keep-date` | With `--amend`: pin the committer date to the author date |
| `--reset-author` | With `--amend`: reset author to your identity + committer date |

`FILES` accepts directories (`torii save src/`) as well as individual files.
`--amend` preserves the original author date by default; `GIT_AUTHOR_DATE` and
`GIT_COMMITTER_DATE` are honoured for both new commits and amends.

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

## `torii show --blame`, `torii scan`, `torii cherry-pick`

Common file inspection and commit operations.

```bash
torii show <file> --blame           # Line-by-line change history (was: torii blame)
torii show <file> --blame -L 10,20  # Specific line range

torii scan                          # Scan staged files for secrets
torii scan --history                # Scan entire git history

torii cherry-pick <hash>            # Apply commit to current branch
torii cherry-pick --continue        # Resume after resolving conflicts
torii cherry-pick --abort           # Abort an in-progress cherry-pick
```

> `torii blame <file>` still works as a deprecated alias and prints a warning. Will be removed in 0.8.

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
torii history compact                              # Pack objects + expire reflog (alias: gc; was: history clean)
torii history orphans                              # Find unreachable objects (alias: fsck)

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

# Message rewrite (reword) — preserves author, committer, dates and content
torii history reword <hash> -m "feat: clearer subject"      # reword one commit
torii history reword <hash> -F message.txt                  # read message from a file
torii history reword --map rewords.txt                      # batch: '<hash> <message>' per line
torii history reword <hash> -m "..." --dry-run              # preview, no changes
torii history reword <hash> -m "..." -S                     # GPG-sign the rewritten commit

# Content rewrite (filter-branch / filter-repo equivalents) — preserves identity + dates
torii history replace-text --literal "SECRET==>***REMOVED***"   # replace text in all history
torii history replace-text --regex "AKIA[0-9A-Z]{16}==>***"     # regex replace
torii history replace-text --rules secrets.txt                  # rules file (filter-repo format)
torii history replace-text --redact-secrets                     # redact lines the scanner flags
torii history filter-path --remove secrets/                     # drop a path from all history
torii history filter-path --subdirectory crates/lib            # extract a subdir as new root
torii history filter-path --rename old/:new/                    # rename a path prefix
torii history redate <hash> --date "2026-06-01 10:00:00 +0000"  # set a commit's date
torii history exec-filter 'rm -f secret.txt'                    # arbitrary --tree-filter (slow)
torii history filter-path --remove x --prune-empty             # drop commits that become empty

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

One home for **every credential torii uses** — the gitorii.com cloud API key *and* the per-platform tokens (github, gitlab, gitea, forgejo, codeberg, bitbucket, sourcehut, cargo). Stored at `~/.config/torii/auth.toml` (chmod 600) for global, `<repo>/.torii/auth.toml` for per-repo.

### Cloud (gitorii.com)

```bash
torii auth login                       # prompt for the gitorii.com API key
torii auth login --key gitorii_sk_…    # save non-interactively
torii auth status                      # org / plan / seats tied to the key
torii auth whoami                      # alias of status
torii auth logout                      # forget the local key
torii auth login --endpoint http://localhost:8080   # self-hosted / dev backend
```

Generate a key at <https://gitorii.com/dashboard/api-keys>. Env var `TORII_API_KEY` overrides per-process.

### Platform tokens

```bash
torii auth set github ghp_xxx              # save a GitHub token globally
torii auth set cargo cio_xxx               # crates.io token (used by torii publish)
torii auth set gitlab glpat-xxx --local    # per-repo override (.torii/auth.toml)
torii auth set <provider> -                # read from stdin (CI-safe, no shell history)
torii auth list                            # every provider's state, masked
torii auth get github                      # one provider, masked
torii auth get github --unsafe-show        # raw value (rarely what you want)
torii auth remove gitea                    # drop a stored token
torii auth doctor                          # diagnose where each token comes from
```

Supported providers: `github`, `gitlab`, `gitea`, `forgejo`, `codeberg`, `bitbucket`, `sourcehut`, `cargo`.

### Resolution order

When torii needs a token (push, PR/issue API, `torii publish`, etc.), it checks **in this order** and stops at the first hit:

1. **Provider-specific env var**: `GITHUB_TOKEN` / `GH_TOKEN`, `GITLAB_TOKEN`, `GITEA_TOKEN`, `FORGEJO_TOKEN`, `CODEBERG_TOKEN`, `BITBUCKET_TOKEN`, `SOURCEHUT_TOKEN`, `CARGO_REGISTRY_TOKEN`.
2. **Generic env var**: `TORII_HTTPS_TOKEN` (any provider).
3. **Local repo store**: `<repo>/.torii/auth.toml`.
4. **Global store**: `~/.config/torii/auth.toml`.

Run `torii auth doctor` to see exactly which one is in effect for each provider.

### Migration from 0.7.0 and earlier

Pre-0.7.1, platform tokens lived in `~/.config/torii/config.toml [auth]`. Torii auto-migrates them to `auth.toml` on the next mutating call; `torii auth doctor` will tell you if a stale `[auth]` block remains in `config.toml` so you can clean it. `torii config set auth.<provider>_token …` keeps working as a deprecated alias that redirects to `torii auth set …` — will be removed in 0.8.

---

## `torii publish`

Publish the current crate to crates.io. Thin wrapper over `cargo publish` that **injects `auth.cargo`** automatically, so you don't need `CARGO_REGISTRY_TOKEN` in `.env` or your shell.

```bash
torii auth set cargo cio_xxx               # once
torii publish                              # validate + upload
torii publish --dry-run                    # validate only
torii publish --no-verify                  # skip verify-build (crates.io rebuilds anyway)
torii publish --allow-dirty                # ignore uncommitted changes
torii publish --token cio_yyy              # one-shot override
```

Passes `--locked` by default (verifies against the committed `Cargo.lock`). Useful in CI: set `CARGO_REGISTRY_TOKEN` env var and torii picks it up just like `cargo publish` does.

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
torii worktree                                     # default: list (same as 'torii worktree list')
torii worktree add -b feature/auth                 # new branch + worktree at ../<repo>-feature-auth/
torii worktree add ../hotfix -b release/0.7        # new branch at explicit path
torii worktree add ../hotfix release/0.7           # check out existing branch in a worktree
torii worktree list                                # every worktree with branch + clean/dirty + ahead/behind
torii worktree remove ../hotfix                    # delete worktree (snapshot taken first)
torii worktree remove ../hotfix --force            # remove even if dirty
torii worktree remove ../hotfix --no-snapshot      # skip the safety snapshot
torii worktree prune                               # clean up metadata of deleted worktrees
torii worktree open ../hotfix                      # launch $SHELL inside the worktree
```

### Inherit paths to avoid rebuilds

The biggest pain of worktrees in practice is that each one starts with an empty `target/`, `node_modules/`, `.venv/` etc., forcing a full rebuild. `worktree.inherit_paths` solves it:

```bash
torii config set worktree.inherit_paths ".env,target,node_modules"
torii worktree add -b feat/login           # .env copied, target/ + node_modules/ symlinked
```

Files are copied (need a real writable copy); directories are symlinked (share the cache between worktrees). Missing entries are silently skipped.

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
| List | per-worktree text, no status | one-line per worktree with branch + clean/dirty + ahead/behind |
| Remove safety | `--force` required if dirty | same + automatic snapshot |
| `open` | (not present) | launches `$SHELL` in the worktree directory |
| Inherit paths from main (`.env`, `target/`) | (not present) | `worktree.inherit_paths` config copies/symlinks them automatically |
| Lock / move / repair | yes | not yet — coming later |

---

## `torii bisect`

Binary-search for the commit that introduced a regression. State-machine wrapper over `git bisect`.

```bash
torii bisect start                  # enter bisect mode
torii bisect bad                    # current HEAD is bad
torii bisect good v0.6.0            # v0.6.0 was good
torii bisect skip                   # current commit unbuildable, skip
torii bisect run cargo test         # automate: exit 0 = good, !=0 = bad, 125 = skip
torii bisect log                    # show the search log so far
torii bisect reset                  # exit bisect mode, restore HEAD
```

---

## `torii describe`

Pretty name for HEAD based on the nearest tag (≡ `git describe`).

```bash
torii describe              # v0.6.9 or v0.6.9-3-gabc1234
torii describe --long       # always use the long form
torii describe --dirty      # append -dirty if working tree has changes
torii describe --tags       # include lightweight tags too
```

---

## `torii archive`

Export a tree or commit as tarball/zip. Wrapper over `git archive`.

```bash
torii archive HEAD -o release.tar.gz
torii archive v0.6.9 --prefix=gitorii-0.6.9/ -o gitorii-0.6.9.tar.gz
torii archive HEAD --format=zip -o release.zip
```

---

## `torii remove` / `torii rename`

Tracked-file operations that touch both the index and the working tree. `rm` and `mv` are kept as aliases for users coming from git.

```bash
torii remove src/old.rs                # remove + untrack (alias: rm)
torii remove src/old.rs --cached       # untrack only (keep on disk)
torii remove -r vendor/legacy/         # recursive
torii remove --force src/dirty.rs      # drop local changes too
torii rename old.rs new.rs             # stage a rename (alias: mv)
torii rename old.rs new.rs --force     # overwrite if target exists
```

---

## `torii grep`

Search tracked content. Wrapper over `git grep` (faster than ripgrep on tracked-only content). Different concern from `torii scan` (secrets).

```bash
torii grep TODO                        # search for TODO in tracked files
torii grep -i "fix me"                 # case-insensitive
torii grep -l unsafe                   # only file names that match
torii grep -w main src/                # word-boundary match, in src/ only
```

---

## `torii notes`

Annotations attached to commits, stored in `refs/notes/commits` so commit OIDs stay stable. Wrapper over `git notes`.

```bash
torii notes                             # list commits with notes
torii notes add HEAD -m "reviewed"      # add a note to HEAD
torii notes append HEAD -m "and also Y" # append to existing note
torii notes show HEAD                   # show the note attached to HEAD
torii notes edit HEAD                   # open $EDITOR on it
torii notes copy v0.6.8 v0.6.9          # copy notes between commits
torii notes remove HEAD                 # drop the note
```

---

## `torii patch`

Export commits as patch files / apply patches as new commits. Wrappers over `git format-patch` + `git am`.

```bash
torii patch export HEAD~3..HEAD                  # export last 3 commits
torii patch export v0.6.8..HEAD -o /tmp/p/       # into a directory
torii patch export HEAD~1..HEAD --stdout         # to stdout
torii patch apply 0001-fix.patch                  # apply a single patch
torii patch apply *.patch                          # apply a series
torii patch apply --continue                      # after resolving conflicts
torii patch apply --abort                         # bail out of an in-progress am
torii patch apply --3way                          # 3-way fallback
```

---

## `torii clean`

Remove untracked files (≡ `git clean`). Defaults to a **dry-run** for safety — pass `-f` to actually delete.

```bash
torii clean                # dry-run, list what would go
torii clean -f             # actually delete
torii clean -f -d          # include untracked directories
torii clean -f -x          # also remove .gitignore-matched files
torii clean -f -X          # ONLY remove .gitignore-matched files
```

> **Heads up:** in 0.7.0 the previous `torii history clean` was renamed to `torii history compact` (alias `gc`) to free up the word `clean` for this command (which matches `git clean` semantics). The old `torii history clean` still works as a deprecated alias and prints a warning.

---

## `torii submodule`

Embed another git repo at a path inside this one and pin it at a specific commit. The embedded repo's history stays separate.

```bash
torii submodule                                       # default: status
torii submodule add git@github.com:owner/lib.git vendor/lib       # register + clone + stage
torii submodule add git@... vendor/lib --branch main              # pin a tracked branch
torii submodule status                                # list with HEAD / working / state
torii submodule init                                  # copy .gitmodules URLs to .git/config
torii submodule init --force                          # overwrite existing entries
torii submodule update                                # fetch + checkout pinned commit
torii submodule update --init                         # init missing first, then update
torii submodule sync                                  # re-copy URLs (after upstream URL change)
torii submodule foreach 'cargo build'                 # run command in each submodule's wd
torii submodule foreach 'echo $TORII_SUBMODULE_PATH'  # env: TORII_SUBMODULE_NAME, _PATH
torii submodule remove vendor/lib                     # deregister + scrub all four state locations
```

### What `remove` actually does

Submodule state lives in four places. `remove` scrubs each of them:

1. `.gitmodules` — strip the `[submodule "<name>"]` section.
2. `.git/config` — same, but in local config.
3. `.git/modules/<name>/` — wipe the cached gitdir (so a future re-add starts clean).
4. **Super-repo index** — remove the gitlink entry via libgit2 directly (not `git rm`, which refuses if `.gitmodules` already has uncommitted edits).

After running, the user still needs `torii save -am "remove submodule X"` to commit the cleanup.

### Limitations

- **No `--recursive`** flag yet — nested submodules need a manual loop. Tracked for a later release.
- **`foreach` stops at first non-zero exit** — no `--continue-on-error`. Matches `git submodule foreach` default.

---

## `torii subtree`

Merge another project's history into a subdirectory of this repo, keeping commits but flattening it into our tree. **Thin wrapper over `git subtree`** (which must be installed — comes with git on most distros; on Debian/Ubuntu it's a separate `git-subtree` package).

```bash
torii subtree add   --prefix=vendor/lib git@... main --squash     # initial import
torii subtree pull  --prefix=vendor/lib git@... main --squash     # fetch upstream changes
torii subtree push  --prefix=vendor/lib git@... main              # push subtree back
torii subtree split --prefix=vendor/lib -b lib-split              # extract history to a new branch
torii subtree merge --prefix=vendor/lib some-ref                  # finish a manual conflict resolution
```

### Why wrapper, not from-scratch

`git subtree` is an official git contrib script refined since 2009 (~800 lines, lots of edge cases around parent detection, orphan commits, --squash semantics, history rewriting through merge bases). Reimplementing those correctly in Rust on top of libgit2 (which has no subtree primitives) would be 1k+ LOC of risk for behaviour that's already correct upstream. Torii adds the consistent UX layer, defers the motor.

### Submodule vs subtree at a glance

| Concept | Submodule | Subtree |
|---------|-----------|---------|
| Embedded history | separate repo, pinned commit | flattened into our commits |
| Extra files | `.gitmodules` | none |
| Cloners need extra steps | yes (`init` + `update`) | no |
| Pushing back upstream | clone-then-push the submodule | `torii subtree push` |
| Best for | shared deps you treat as black boxes | vendored deps you patch locally |

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
