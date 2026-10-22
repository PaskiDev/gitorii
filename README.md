# Gitorii ⛩️

[![Crates.io](https://img.shields.io/crates/v/gitorii.svg)](https://crates.io/crates/gitorii)
[![Downloads](https://img.shields.io/crates/d/gitorii.svg)](https://crates.io/crates/gitorii)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-blue.svg)](#license)
[![Rust 1.95+](https://img.shields.io/badge/rust-1.95+-orange.svg)](https://www.rust-lang.org)

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
  Codeberg / Forgejo, auth-code+PKCE for Bitbucket and Gitea.
  Self-hosted hosts take `FORGEJO_URL` / `GITEA_URL`. Automatic refresh
  — no more re-authing every two hours.
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
curl -L "https://gitlab.com/api/v4/projects/paskidev%2Fgitorii/packages/generic/gitorii/v0.17.1/torii-linux-x86_64" \
  -o ~/.local/bin/torii && chmod +x ~/.local/bin/torii
```

(replace `v0.17.1` with the latest tag from the
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
[**Troubleshooting**](#troubleshooting-rustc-sigsegv-during-install) below.


## Quick start

```bash
torii init                            # initialize repo
torii status                          # see what changed
torii save -am "feat: add user auth"  # stage all + commit
torii sync                            # pull + push
torii branch feature -c               # create and switch to a branch
torii sync main --merge               # merge main in; asks which side wins on conflicts
torii log --oneline --graph           # history
torii snapshot create -n "before-x"   # safety save you can restore
torii scan --history                  # hunt for leaked secrets
torii mirror sync                     # push to every mirror at once
torii                                 # open the TUI
```

Every command, flag by flag: [**command reference**](https://gitlab.com/paskidev/gitorii/-/blob/main/docs/commands.md) ·
[gitorii.com/docs](https://gitorii.com/docs).

## TUI

```bash
torii tui        # or just `torii`
```

Full-screen interface with sidebar navigation: files, save, sync, log,
branches, tags, snapshots, PRs/MRs, issues, CI pipelines, worktrees,
submodules, workspaces, auth and config — all from the keyboard.

| Key | Action |
|-----|--------|
| `↑↓` / `j k` | Navigate sidebar (previews view in real time) |
| `Tab` / `Enter` | Enter selected view |
| `Esc` | Return to sidebar |
| `q` / `Ctrl+C` | Quit |
| `e` | Toggle event log |
| `?` | Help |

Every view and its keys: [command reference → TUI](https://gitlab.com/paskidev/gitorii/-/blob/main/docs/commands.md#tui).

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
| `git blame src/main.rs` | `torii blame src/main.rs` |
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

## Troubleshooting: `rustc SIGSEGV` during install

`cargo install gitorii` can fail with:

```
error: rustc interrupted by SIGSEGV, printing backtrace
... LlvmCodegenBackend ... compile_codegen_unit ...
```

This happens when monomorphisation through the transitive crypto chain
that `russh` pulls in goes deep enough to overflow rustc's 8 MB default
thread stack, or when too many parallel rustcs blow up system RAM. Fix:
raise the stack, cap parallelism.

```bash
RUST_MIN_STACK=67108864 cargo install gitorii --locked -j 1
```

`-j 1` (single-thread) is the bulletproof setting. Slower but stable.

**Last resort: skip the compiler.** Grab the prebuilt binary from the
[GitLab Generic Package Registry](https://gitlab.com/paskidev/gitorii/-/releases) — they're built by CI on every tag and don't expire.

## Links

- [Website](https://gitorii.com)
- [Releases](https://gitlab.com/paskidev/gitorii/-/releases)
- [Docs](https://gitorii.com/docs)
- [Command reference](https://gitlab.com/paskidev/gitorii/-/blob/main/docs/commands.md)
- [Issues](https://gitlab.com/paskidev/gitorii/-/issues)
- [crates.io](https://crates.io/crates/gitorii)
- [Changelog](https://gitlab.com/paskidev/gitorii/-/blob/main/CHANGELOG.md)

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

Contributions are accepted under the Developer Certificate of Origin 1.1 — sign your commits with `git commit -s` (or `torii save -s`).

### Trademark

The names **Torii** and **Gitorii** and associated logos are trademarks of Pasqual Peñalver. The license above covers the source code only; forks and derivative works must not use these names or confusingly similar names in a way that suggests official endorsement.

## Author

Built by **Pasqual Peñalver Collado** ([PaskiDev](https://paski.dev)) — Lead Full Stack Developer in Barcelona. More projects and devlog at [paski.dev](https://paski.dev).
