# Gitorii ⛩️

[![Crates.io](https://img.shields.io/crates/v/gitorii.svg)](https://crates.io/crates/gitorii)
[![Downloads](https://img.shields.io/crates/d/gitorii.svg)](https://crates.io/crates/gitorii)
[![License](https://img.shields.io/badge/license-custom-blue.svg)](LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)

**A human-first Git client.** Simpler commands, a built-in TUI, snapshots,
multi-platform mirrors, secret scanning, OAuth, GPG signing, CI runners,
and a self-hosted platform registry — all from one binary called `torii`.

> Git was designed for Linus, by Linus. Gitorii is designed for everyone — including AI.

## What you get

- **Simpler verbs**: `torii save`, `torii sync`, `torii snapshot` — same
  semantics as git, fewer subcommands to remember.
- **A full TUI**: `torii tui` (or just `torii` with no args) — interactive
  views for commits, log, branches, tags, PRs, issues, CI pipelines,
  worktrees, submodules, runners, auth, config.
- **Multi-platform native**: GitHub, GitLab, Codeberg / Gitea / Forgejo,
  Bitbucket, sourcehut, Radicle, Azure DevOps. Mirror a repo across
  several at once.
- **Self-hosted first-class** (0.8+): `~/.config/torii/platforms.toml`
  declares your self-hosted GitLab / Gitea / Forgejo / GitHub Enterprise /
  Bitbucket Data Center. Detection routes the right client transparently.
- **OAuth + refresh tokens** (0.7.30+): device flow for GitHub / GitLab /
  Codeberg, auth-code+PKCE for Bitbucket. Automatic refresh — no more
  re-authing every two hours.
- **GPG signing done right** (0.7.35+): `commit.gpgsign` honoured at
  commit time, `torii sign` to re-sign existing history, `torii log
  --signatures` for a verdict column, armor viewer in the TUI Log view.
- **CI runners** (0.7.29+): `torii runner register` against the
  platform, `torii runner spawn --docker` to bring up a self-hosted
  GitLab Runner container, `torii runner exec <job>` to run a job
  locally without push.
- **Snapshots & safety nets**: `torii snapshot stash` / `torii snapshot
  create -n "wip"` — never lose work to a force-push or hard reset
  again. `torii history fsck` walks unreachable objects so you can
  recover what the reflog already expired.
- **Built-in secret scanner**: matches AWS keys, GitHub PATs, GitLab
  PATs, private SSH/PEM blocks, generic `password=` patterns. Runs
  pre-commit by default. Custom regex via `.toriignore [secrets]`.
- **Pure-Rust transports**: `rustls` HTTPS, `russh` SSH. No
  `libcurl`/`libssh2`/`openssl`/`perl` to fight on build day.

## Install

**Prebuilt binary** (Linux / macOS — recommended, no compiler needed):

```bash
curl -L "https://gitlab.com/api/v4/projects/paskidev%2Fgitorii/packages/generic/gitorii/v0.8.1/torii-linux-x86_64" \
  -o ~/.local/bin/torii && chmod +x ~/.local/bin/torii
```

(replace `v0.8.1` with the latest tag from the
[releases page](https://gitlab.com/paskidev/gitorii/-/releases); aliases
exist for `torii-linux-aarch64` and `torii-windows-x86_64.exe`).

**Arch Linux (AUR)**:

```bash
yay -S gitorii          # or `paru -S gitorii`
```

**From crates.io** (compiles locally):

```bash
cargo install gitorii --locked
```

Building from source needs only a C compiler (`gcc` / `clang`) — no
`perl`, no `openssl-dev`, no `libssh2-dev`, no `pkg-config`. If you
hit a `rustc` ICE or `SIGSEGV` mid-compile, see
[**Troubleshooting**](#troubleshooting-rustc-ice--sigsegv) below.

## Quick start

```bash
torii init                            # initialize repo
torii status                          # see what changed
torii save -am "feat: add user auth"  # stage all + commit
torii sync                            # pull + push
```

## Command reference

### Core

| Command | Description |
|---------|-------------|
| `torii init` | Initialize a repository |
| `torii save -m "msg"` | Commit staged changes |
| `torii save -am "msg"` | Stage all and commit |
| `torii save <files> -m "msg"` | Stage specific files and commit |
| `torii save --amend -m "msg"` | Amend last commit |
| `torii save --revert <hash> -m "msg"` | Revert a commit |
| `torii save --reset HEAD~1 --reset-mode soft` | Undo last commit, keep changes |
| `torii save --reset HEAD~1 --reset-mode hard` | Undo last commit, discard changes |
| `torii sync` | Pull and push |
| `torii sync --push` | Push only |
| `torii sync --pull` | Pull only |
| `torii sync --force` | Force push |
| `torii sync --fetch` | Fetch without merging |
| `torii sync <branch>` | Integrate branch (smart merge/rebase) |
| `torii sync <branch> --merge` | Force merge strategy |
| `torii sync <branch> --rebase` | Force rebase strategy |
| `torii sync <branch> --preview` | Preview without executing |
| `torii status` | Repository status |
| `torii diff` | Show unstaged changes |
| `torii diff --staged` | Show staged changes |
| `torii diff --last` | Show last commit diff |

### Workspaces

Run commands across multiple repos at once.

```bash
torii workspace add <name> ~/repos/api      # add repo to workspace
torii workspace add <name> ~/repos/frontend
torii workspace list                        # list all workspaces
torii workspace status <name>              # git status across all repos
torii workspace save <name> -m "wip" --all # commit all repos with changes
torii workspace sync <name>                # pull + push all repos
torii workspace remove <name> ~/repos/api  # remove a repo
torii workspace delete <name>              # delete workspace
```

### Branches

```bash
torii branch                  # list local branches
torii branch --all            # list local and remote branches
torii branch <name> -c        # create and switch
torii branch <name>           # switch to branch
torii branch -d <name>        # delete branch
torii branch --rename <name>  # rename current branch
```

### Worktrees

Multiple checkouts of the same repo, each on its own branch, sharing objects. Great for hot-fixes without disturbing in-progress work.

```bash
torii worktree                             # default: list
torii worktree add -b feature/auth         # new branch + worktree at ../<repo>-feature-auth/
torii worktree add ../hotfix release/0.7   # check out existing branch in a worktree
torii worktree list                        # all worktrees with branch + clean/dirty + ahead/behind
torii worktree remove ../hotfix            # delete worktree (snapshot taken automatically)
torii worktree remove ../hotfix --force    # ...even if dirty
torii worktree prune                       # clean up metadata of deleted worktrees
torii worktree open ../hotfix              # launch $SHELL inside the worktree
```

Default path comes from `worktree.base_dir` config (default `..`). `worktree.inherit_paths` automatically copies/symlinks `.env`, `target/`, `node_modules/` etc. into new worktrees so you don't rebuild from scratch:

```bash
torii config set worktree.inherit_paths ".env,target,node_modules"
```

### Submodules

Embed another git repo at a path and commit pinned at a specific commit.

```bash
torii submodule                              # default: status
torii submodule add git@github.com:owner/lib.git vendor/lib --branch main
torii submodule status                       # list with HEAD / working / state
torii submodule init                         # copy .gitmodules URLs to .git/config
torii submodule update --init                # init missing + checkout pinned commit
torii submodule sync                         # re-copy URLs (after upstream URL change)
torii submodule foreach 'cargo build'        # run command in each submodule
torii submodule remove vendor/lib            # deregister + scrub all four state locations
```

### Subtrees

Merge another project's history into a subdirectory of this repo, flattening it into your tree. Thin wrapper over `git subtree` (must be installed).

```bash
torii subtree add  --prefix=vendor/lib git@... main --squash    # initial import
torii subtree pull --prefix=vendor/lib git@... main --squash    # fetch upstream changes
torii subtree push --prefix=vendor/lib git@... main             # push subtree back upstream
torii subtree split --prefix=vendor/lib -b lib-split            # extract history to a new branch
```

Submodule vs subtree quick choice: submodule when the dep is a black box you bump occasionally; subtree when you patch it locally and want one cohesive history.

### Inspect

```bash
torii show                         # show HEAD commit with diff
torii show <hash>                  # show specific commit
torii show <tag>                   # show tag details
torii show <file> --blame          # line-by-line change history
torii show <file> --blame -L 10,20 # specific line range
```

### History

```bash
torii log                           # last 10 commits
torii log -n 50                     # last 50 commits
torii log --oneline                 # compact view
torii log --graph                   # branch graph
torii log --author "Alice"          # filter by author
torii log --since 2026-01-01        # filter by date
torii log --grep "feat"             # filter by message
torii log --stat                    # show file change stats
torii log --reflog                  # HEAD movement history

torii sync --verify                 # verify local vs remote HEAD

torii show <file> --blame           # line-by-line change history (was: torii blame, deprecated)
torii show <file> --blame -L 10,20

torii scan                          # scan staged files for secrets
torii scan --history                # scan entire git history

torii cherry-pick <hash>            # apply commit to current branch
torii cherry-pick --continue
torii cherry-pick --abort

torii history rewrite "2026-01-01" "2026-03-01"  # rewrite commit dates
torii history compact               # pack objects + expire reflog (alias: gc)
torii history orphans               # find unreachable objects (alias: fsck)
torii history remove-file <path>    # purge file from entire history

torii history rebase main           # rebase onto branch
torii history rebase HEAD~5 -i      # interactive rebase (opens editor)
torii history rebase --root         # rebase from root commit (squash initial)
torii history rebase HEAD~5 --todo-file plan.txt
torii history rebase --continue
torii history rebase --abort
torii history rebase --skip

torii history reauthor --old "Old <a@x>" --new "New <b@y>"   # rename author in history
torii history reauthor --old oldname --new "New <b@y>"        # match by name only
torii history reauthor --old a@x --new "New <b@y>" --committer  # also committer
torii history reauthor ... --since v0.6.0 --dry-run           # preview a range
torii history mailmap apply                                    # batch via .mailmap
torii history mailmap apply --file other.mailmap --dry-run
```

`reauthor` and `mailmap apply` take a safety snapshot before rewriting
(revert with `torii snapshot restore <id>`), preserve timestamps,
rewrite annotated-tag taggers to match, and abort on pending operations
or dirty working trees. GPG signatures invalidate after rewrite —
re-sign manually if needed.

### Security scanner

```bash
torii history scan            # scan staged files for secrets
torii history scan --history  # scan entire git history
```

Runs automatically before every `torii save`. Detects:
- JWT tokens, AWS keys (AKIA/ASIA), GitHub/GitLab tokens
- Stripe live keys, Twilio/SendGrid/Brevo keys
- PEM private keys, database connection strings with credentials
- Generic API keys and passwords

Files named `*.example`, `*.sample`, or `*.template` are always skipped.

### Ignore rules (`.toriignore` + `.toriignore.local`)

`.toriignore` extends `.gitignore` syntax with optional sections for custom secret patterns, file size limits, and pre/post hooks. It is auto-synced into `.git/info/exclude` so `git` itself respects the rules.

```bash
torii ignore add 'build/'                          # add path to public .toriignore
torii ignore add --local '/internal/billing/'      # add path to .toriignore.local
torii ignore secret 'AKIA[0-9A-Z]{16}' --name AWS  # add secret regex (defaults to .local)
torii ignore secret 'ghp_[A-Za-z0-9]{36}' --public # add to public .toriignore (warns)
torii ignore list                                  # show effective rules (merged)
```

**`.toriignore.local`** is machine-private — gitignored automatically and never committed. Use it for rules whose existence would aid recon if the public repo leaked: proprietary secret formats, internal paths, custom audit regex. Local rules merge on top of public ones; tighter local size limits override public ones.

```
# .toriignore                # .toriignore.local (private)
[secrets]                    [secrets]
deny: AKIA[0-9A-Z]{16}       deny: PROP_[a-z]{20}  # internal
[size]                       [size]
max: 10MB                    max: 5MB              # tighter wins
```

### Snapshots

Snapshots are local saves — not commits. Use them before risky operations.

```bash
torii snapshot create -n "before-refactor"
torii snapshot list
torii snapshot restore <id>
torii snapshot delete <id>
torii snapshot stash              # quick stash
torii snapshot stash -u           # include untracked files
torii snapshot unstash
torii snapshot unstash <id> --keep
torii snapshot undo               # undo last operation
```

### Tags

```bash
torii tag create v1.0.0 -m "Release"
torii tag list
torii tag delete v1.0.0
torii tag push v1.0.0
torii tag push                    # push all tags
torii tag show v1.0.0
torii tag create --release                  # auto-bump from conventional commits
torii tag create --release --bump minor     # force bump type
torii tag create --release --dry-run        # preview without creating
```

`torii tag create --release` reads commits since the last tag and bumps following [Conventional Commits](https://www.conventionalcommits.org/):
- `feat:` → minor bump
- `fix:` / `perf:` → patch bump
- `feat!:` / breaking → major bump

### Mirrors

Mirror your repository across multiple platforms simultaneously.

```bash
torii mirror add gitlab user <username> <repo> --primary
torii mirror add github user <username> <repo>
torii mirror add codeberg user <username> <repo>
torii mirror sync
torii mirror sync --force
torii mirror list
torii mirror promote gitlab user
torii mirror remove github user
torii mirror autofetch --enable --interval 30m
torii mirror autofetch --disable
torii mirror autofetch --status
```

Supported platforms: GitHub, GitLab, Codeberg, Bitbucket, Gitea, Forgejo.

### Remote repository management

Create and manage repositories directly from the CLI (requires auth token in config):

```bash
torii remote create github <repo> --public
torii remote create github <repo> --private --description "My repo"
torii remote delete github <owner> <repo> --yes
torii remote visibility github <owner> <repo> --public
torii remote configure github <owner> <repo> --default-branch main
torii remote info github <owner> <repo>
torii remote list github

# Multiple platforms at once (comma-separated)
torii remote create github,gitlab,codeberg <name> --public --push
torii remote delete github,gitlab <owner> <name> --yes
```

### Config

```bash
torii config set user.name "Alice"
torii config set user.name "Alice" --local
torii config get user.name
torii config list
torii config list --local
torii config edit
torii config reset
```

Available keys: `user.name`, `user.email`, `user.editor`, `auth.github_token`, `auth.gitlab_token`, `auth.gitea_token`, `auth.forgejo_token`, `auth.codeberg_token`, `git.default_branch`, `git.sign_commits`, `git.pull_rebase`, `mirror.default_protocol`, `mirror.autofetch_enabled`, `snapshot.auto_enabled`, `snapshot.auto_interval_minutes`, `ui.colors`, `ui.emoji`, `ui.verbose`, `ui.date_format`, `worktree.base_dir`, `worktree.inherit_paths` (comma-separated).

### Auth (gitorii.com cloud)

Separate from the per-platform `auth.<platform>_token` keys above. `torii auth` manages the API key for gitorii.com cloud features (CI transpile, etc.), stored at `~/.config/torii/auth.toml` (chmod 600).

```bash
torii auth login                    # prompt for API key and save
torii auth login --key gitorii_sk_… # save non-interactively
torii auth status                   # show org / plan / seats
torii auth whoami                   # alias of status
torii auth logout                   # forget the local key
```

Override per-process with `TORII_API_KEY=gitorii_sk_…`. Generate keys at <https://gitorii.com/dashboard/api-keys>.

### Pull requests

Works against the platform of the current repo (GitHub, GitLab, Codeberg, etc.). Requires `auth.<platform>_token` to be set.

```bash
torii pr list                                  # list open PRs
torii pr list --state closed|merged|all
torii pr create -t "feat: login" -b main       # create PR (head = current branch)
torii pr create -t "wip" --draft               # create as draft
torii pr merge 42                              # merge with merge commit
torii pr merge 42 --method squash|rebase
torii pr close 42                              # close without merging
torii pr checkout 42                           # checkout PR branch locally
torii pr open 42                               # open in browser
```

### Issues

```bash
torii issue list                               # open issues
torii issue list --state closed|all
torii issue create -t "bug: crash"             # create issue
torii issue create -t "title" -d "description"
torii issue close 42
torii issue comment 42 -m "Fixed in v0.6.6"
```

### CI / platform management

Four top-level commands wrap the platform-side APIs of GitLab, GitHub,
and Codeberg (Gitea / Forgejo, since 0.7.13) for the work that lives
next to (but not inside) git history: CI pipelines, individual jobs,
binary registries, and release pages.

All four:

- Auto-detect the platform (github / gitlab) from the URL of the git
  remote they're targeting.
- Use the platform's token from `torii auth set <platform>` for
  authentication.
- Accept `--remote NAME` to target a specific git remote when the
  project is mirrored across multiple platforms — see
  **multi-platform** below.
- Are also available interactively from the **platform** view in
  `torii tui` (0.7.12+): four sub-tabs with drill-down from pipelines
  into their jobs and per-job logs.

**Pipelines** — whole CI runs (GitLab Pipelines / GitHub Actions workflow runs):

```bash
torii pipeline list                            # current branch's recent pipelines
torii pipeline list --status failed            # only failed
torii pipeline list --limit 50                 # up to 50 (clamped to 100)
torii pipeline cancel <id>                     # cancel a running pipeline
torii pipeline retry <id>                      # retry failed jobs in a pipeline
torii pipeline delete <id>                     # delete one
torii pipeline delete --status failed --yes    # batch: every failed
torii pipeline delete --status failed --older-than 7d --yes
```

`--status` accepts `success | failed | running | canceled | pending`.

**Jobs** — drill into individual CI jobs inside a pipeline:

```bash
torii job list --pipeline 1234                 # jobs in a pipeline
torii job list --pipeline 1234 --status failed
torii job log <id>                             # print job log
torii job log <id> --tail 50                   # last 50 lines (post-mortem mode)
torii job retry <id>                           # retry one job        (GitLab only)
torii job cancel <id>                          # cancel one job       (GitLab only)
torii job artifacts <id> -o artifacts.zip      # download artifacts   (GitLab only)
torii job erase <id>                           # clear log + artifacts (GitLab only)
```

GitHub Actions scopes retry / cancel / artifacts to the *workflow run*,
not the job — those subcommands return an error pointing at the
equivalent `torii pipeline` operation.

**Packages** — GitLab Package Registry (binary artifacts uploaded by CI):

```bash
torii package list                             # all packages
torii package list --type generic              # filter by type
torii package list --name gitorii              # substring match
torii package files <package-id>               # files inside a package
torii package delete <id>                       # delete one
torii package delete --version v0.7.0 --yes    # batch by version
torii package delete --older-than 90d --yes    # batch by age
```

GitLab-only. GitHub's binary distribution is via Release Assets,
covered by `torii release`.

**Releases** — release pages with notes + asset links:

```bash
torii release list                             # recent releases
torii release show v0.7.9                      # full details (description, URL)
torii release edit v0.7.9 --name "New title"   # rename
torii release edit v0.7.9 --notes notes.md     # replace description from file
torii release edit v0.7.9 --notes -            # replace from stdin
torii release delete v0.7.9 --yes              # delete release entity (tag stays)
```

Both GitLab and GitHub.

#### Multi-platform with `--remote NAME`

By default, the platform commands auto-detect from the URL of the
`origin` remote. For repos mirrored across multiple platforms — e.g.
`origin → gitlab.com` and `github-paskidev → github.com` — each
backend has its own pipelines, packages, and releases. Use
`--remote NAME` to target a specific one:

```bash
torii pipeline list --remote origin                  # same as default
torii pipeline list --remote github-paskidev         # GitHub side
torii release edit v0.7.9 --notes new.md --remote github-paskidev
torii package delete --version v0.7.0 --remote origin --yes
```

The `--remote` flag is *global within the command*, so these are
equivalent:

```bash
torii pipeline --remote NAME list
torii pipeline list --remote NAME
```

Each platform has its own auth token (`torii auth set github`,
`torii auth set gitlab`) — to operate on both, both tokens must be
configured.

**Today (0.7.11):** each invocation targets one remote at a time.
**Coming in 0.7.12:** `--remote all` to iterate every configured
remote, plus multi-occurrence (`--remote a --remote b`) for an
arbitrary subset. The TUI Platform view consolidates everything into
tabs per remote.

**Coming in 0.8.0+:** `~/.config/torii/platforms.toml` to elevate
platforms to first-class citizens — `--remote NAME` stays as the
per-invocation override.

### Other

```bash
torii clone github <user>/<repo>        # clone with platform shorthand
torii clone https://...                 # clone with full URL
torii clone github <user>/<repo> -d dir # clone into specific directory
torii config check-ssh                  # verify SSH key setup
```

## TUI

Launch the interactive terminal UI:

```bash
torii tui
```

Full-screen interface with sidebar navigation. All views accessible from keyboard.

| Key | Action |
|-----|--------|
| `↑↓` / `j k` | Navigate sidebar (previews view in real time) |
| `Tab` / `Enter` | Enter selected view |
| `Esc` | Return to sidebar |
| `q` / `Ctrl+C` | Quit |
| `e` | Toggle event log |
| `?` | Help |

**Views** (navigate with `↑/↓` in the sidebar; views load on selection):

| View | Description |
|------|-------------|
| files | Staged / unstaged / untracked files. `Space` to stage/unstage, `d` for diff |
| save | Commit staged files. Optional conventional commit type selector |
| sync | Pull, push, fetch, force-push. Animated progress, non-blocking |
| snapshot | Create, restore, delete snapshots. Auto-snapshot with configurable interval |
| log | Commit history. `Enter` diff, `r` reset soft, `b` new branch |
| branch | List branches, checkout with `Enter` |
| tags | List, push, delete tags |
| worktrees | Per-repo and global worktrees |
| submodules | Submodule management |
| remote | Remote info + mirror sync (mirror tab inside) |
| workspace | Multi-repo workspace management |
| pr/mr | Pull requests / merge requests across platforms |
| issues | Issues across platforms |
| bisect | `git bisect` state machine |
| auth | Cloud key + platform tokens |
| platform | **0.7.12** — unified CI/CD surface: pipelines, jobs, releases, packages |
| config | Edit repo/global config inline |

**Platform view (0.7.12)** — four sub-tabs (`1` pipelines, `2` jobs, `3` releases, `4` packages) over a single remote. Enter on a pipeline drills into its jobs; Enter on a job streams the log into a scrollable panel; `Esc` walks back. `r` opens a centred popup to switch the remote (auto-discovered from the repo). `Ctrl+R` reloads the active sub-tab.

**Diff view** — LCS-based inline char highlighting, paired +/- lines, hunk separators, line numbers.

**Snapshot auto-interval** — configurable per-repo in `.torii/auto-interval` (travels with the project).

**Settings** — customizable brand color, border style, keybinds. Saved in `~/.torii/tui-settings.toml`.

## Gitorii vs other Git clients

| Feature | Gitorii | Lazygit | GitUI | Tig | Magit | gh CLI |
|---------|:-------:|:-------:|:-----:|:---:|:-----:|:------:|
| Pure CLI (no TUI required) | ✓ | ✗ | ✗ | ✗ | ✗ | ✓ |
| Optional TUI with full feature parity | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ |
| Secret scanner (pre-commit) | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Scan full git history | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Snapshots (pre-op safety saves) | ✓ | ✗ | ✗ | ✗ | ~ | ✗ |
| Multi-remote mirrors | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Workspace (multi-repo commands) | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| PR / MR creation from CLI | ✓ | ~ | ✗ | ✗ | ~ | ✓ |
| GitHub + GitLab native support | ✓ | ✗ | ✗ | ✗ | ~ | ✗ |
| Conventional commits auto-tag | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Remove file from entire history | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Interactive rebase | ✓ | ✓ | ~ | ✗ | ✓ | ✗ |
| No runtime dependencies | ✓ | ✗ | ✓ | ✓ | ✗ | ✗ |

> ✓ supported · ~ partial · ✗ not supported  
> Full comparison at [gitorii.com/vs](https://gitorii.com/vs)

## Why Gitorii?

| Git | Gitorii |
|-----|---------|
| `git add . && git commit -m "msg"` | `torii save -am "msg"` |
| `git pull && git push` | `torii sync` |
| `git switch -c branch` | `torii branch <name> -c` |
| `git fetch` | `torii sync --fetch` |
| `git reset --soft HEAD~1` | `torii save --reset HEAD~1 --reset-mode soft` |
| `git rebase -i HEAD~3` | `torii history rebase HEAD~3 -i` |
| `git stash push -u` | `torii snapshot stash -u` |
| `git log --oneline --author X` | `torii log --oneline --author X` |
| `git show HEAD` | `torii show` |
| `git blame src/main.rs` | `torii show src/main.rs --blame` |
| Push to 3 platforms | `torii mirror sync` |
| Hunt for exposed secrets | `torii scan --history` |
| Run status across 5 repos | `torii workspace status <name>` |
| Commit all dirty repos at once | `torii workspace save <name> -am "wip"` |

## System dependencies

**None at runtime** for prebuilt binaries. **Only a C compiler** when building from source.

Since 0.6.0 gitorii ships its own pure-Rust HTTPS (`rustls`) and SSH (`russh`)
transports, so libgit2 is built without HTTPS/SSH support — no openssl-dev,
no libssh2-dev, no pkg-config, no perl.

| Platform | Build prerequisite |
|----------|--------------------|
| Ubuntu/Debian | `sudo apt install build-essential` |
| Fedora/RHEL | `sudo dnf install gcc make` |
| macOS | `xcode-select --install` |
| Arch | `sudo pacman -S base-devel` |
| Alpine | `apk add build-base` |

Want a fully static binary with zero runtime libs (runs on Alpine, scratch,
busybox)? Build with the `static` feature on the musl target:

```bash
cargo build --release --target x86_64-unknown-linux-musl --features static
```

## Troubleshooting: `rustc` ICE / SIGSEGV

`cargo install gitorii` can fail in two distinct ways depending on your
toolchain. Both are upstream bugs triggered by the transitive crypto
chain `russh` pulls in (`rsa 0.10-rc` → `crypto-bigint 0.7-rc` →
`elliptic-curve 0.14-rc`). **Neither is a gitorii bug.**

**1. `rustc 1.95.0` ICE in mono-item partitioning.**

```
thread 'rustc' panicked at compiler/rustc_span/src/symbol.rs:2760
called `Option::unwrap()` on a `None` value
```

The crate ships `rust-toolchain.toml` pinning the build to `1.94.0`,
which `rustup` honours automatically inside the unpacked crate. Usually
all you need is:

```bash
rustup install 1.94.0
cargo install gitorii --locked
```

If your shell or cargo config overrides the pin, force the toolchain
explicitly:

```bash
cargo +1.94.0 install gitorii --locked
```

**2. `SIGSEGV` in LLVM codegen / stack overflow.**

```
error: rustc interrupted by SIGSEGV, printing backtrace
... LlvmCodegenBackend ... compile_codegen_unit ...
```

Hits independent of rustc version when monomorphisation goes deep enough
to overflow rustc's 8 MB default thread stack, or when too many parallel
rustcs blow up system RAM. Fix: raise the stack, cap parallelism.

```bash
RUST_MIN_STACK=67108864 \
  cargo +1.94.0 install gitorii --locked -j 1
```

`-j 1` (single-thread) is the bulletproof setting. Slower but stable.

**3. Last resort: skip the compiler.** Grab the prebuilt binary from the
[GitLab Generic Package Registry](https://gitlab.com/paskidev/gitorii/-/releases) — they're built by CI on every tag and don't expire.

Upstream tracking: `rust-lang/rust` (compiler ICE), `warp-tech/russh`
(crypto RC defaults). The `rust-toolchain.toml` pin and this section
go away once a fixed stable rustc lands and is validated against the
dep tree.

## Links

- [Website](https://gitorii.com)
- [Releases](https://gitlab.com/paskidev/gitorii/-/releases)
- [Docs](https://gitorii.com/docs)
- [Issues](https://gitlab.com/paskidev/gitorii/-/issues)
- [crates.io](https://crates.io/crates/gitorii)
- [Changelog](https://gitlab.com/paskidev/gitorii/-/blob/main/CHANGELOG.md)

## License

TSAL-1.0 — Free for personal and non-production use. Commercial use requires a license. Converts to Apache 2.0 after 10 years. See [LICENSE](LICENSE) for details.

## Author

Built by **Pasqual Peñalver Collado** ([PaskiDev](https://paski.dev)) — Lead Full Stack Developer in Barcelona. More projects and devlog at [paski.dev](https://paski.dev).
