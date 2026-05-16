# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.6] - 2026-05-16

### Build / toolchain
- **Pin build toolchain to `rustc 1.94.0` via `rust-toolchain.toml`** to work around a `rustc 1.95.0` ICE in mono-item partitioning. The regression hits the transitive crypto chain (`russh` → `rsa 0.10-rc` → `crypto-bigint 0.7-rc` → `elliptic-curve 0.14-rc`) and surfaces as `Option::unwrap() on a None value` inside the compiler or as SIGSEGV mid-compile. Honoured automatically by `rustup` users — distro-shipped rustc see README "Known issue" for manual workarounds. To be removed when a fixed stable lands.
- **Declare MSRV `rust-version = "1.85"`** in `Cargo.toml`, matching `russh`'s declared minimum. Stops cargo from attempting older toolchains where the transitive deps don't build at all.

### Fixed
- **No more `choose HTTP client (ureq or reqwest)` panic on first command per cache window.** `update-informer 1.3.0` made the HTTP backend feature non-optional; the previous `features = ["crates"]` declaration was enough to compile but landed on a stub that panics at runtime. Now pulls `ureq` (rustls-backed, no extra system deps) alongside `crates`. Reproduced via `torii mirror add gitlab user paskidev gitorii --primary` on a fresh cache.

### Documentation
- **`torii --help` now groups examples by intent** instead of listing nine random one-liners. Five thematic blocks (daily flow / branch & history / repos & identity / release & collaboration / interactive UI) cover the top-level surface and surface previously-hidden commands like `torii status`, `torii diff --staged`, `torii config set`, `torii history rebase`, `torii history scan`, `torii auth login`, `torii tag create`, `torii pr create`, `torii workspace status`, `torii tui`.
- **`COMMANDS.md` gains six previously-undocumented sections**: `torii auth`, `torii workspace`, `torii pr`, `torii issue`, `torii ignore`, `torii tui`. Every example is sourced verbatim from the `after_help` of the matching subcommand so reference and CLI stay in lockstep.
- **`COMMANDS.md` `.toriignore` reference rewritten**: previous text only mentioned `.gitignore` sync and omitted the `[secrets]` / `[size]` / `[hooks]` sections and the machine-private `.toriignore.local` overlay. Now documents the full schema and links to `SECURITY.md` for the hook trust model.
- **`README.md` adds Auth (cloud) / Pull requests / Issues sections** and completes the `torii config` key list (`auth.gitea_token`, `auth.forgejo_token`, `auth.codeberg_token`, `mirror.autofetch_enabled`, `snapshot.auto_interval_minutes`, `ui.date_format` were missing).

## [0.6.3] - 2026-05-10

### Fixed
- **`torii sync --push` no longer reports false success** when libgit2 returns Ok with zero refs ever acknowledged by the server. Common with very large pushes over SSH (3GB+ to GitLab). Errors out with a clear diagnostic suggesting HTTPS+token instead. `sideband_progress` callback wired so `remote: …` server messages reach stderr verbatim.
- **`torii sync --verify` queries the live remote**, not the cached `refs/remotes/origin/*`. Previous behaviour reported "in sync" against an empty remote right after a silently-failed push. Now opens a real connection and lists the actual refs; surfaces "no such ref on remote" when the branch isn't there at all.
- **`torii clone <plat> <user/repo> <path>`** honours the trailing path arg (was silently ignored). Same for `torii clone <url> <path>`. Precedence: `--directory` > positional > derive-from-URL.
- **Empty-clone HEAD points at config'd default branch** (`git.default_branch`, default `main`) instead of libgit2's `master` fallback. Previously, cloning an empty repo whose remote default was `main` left `.git/HEAD` at `refs/heads/master`, breaking the next `torii sync --pull`.
- **`torii clone` accepts `file://`, `git://`, `ssh://`, local paths, Windows drives, and scp-form URLs** via a unified `looks_like_clone_url` parser. Previously `torii clone file:///tmp/src dest` errored "Unknown platform 'file:///tmp/src'".

## [0.6.2] - 2026-05-10

### Added
- **Live clone progress.** `torii clone` now redraws every ~100ms with `📥 N% recv/total objects · indexed · MB` and passes server `remote: …` messages through verbatim. Previously cloning a multi-GB repo (servo, chromium) looked frozen for minutes. Set `TORII_CLONE_DEPTH=N` for a shallow fetch.
- HTTPS transport gains `connect_timeout=10s` + request `timeout=300s` (override with `TORII_HTTP_TIMEOUT_SECS`). Hung servers no longer freeze torii indefinitely.

### Fixed
- **`torii sync` no longer aborts on a freshly created remote** with `corrupted loose reference file: FETCH_HEAD`. Empty / missing FETCH_HEAD now treated as "nothing to pull".
- **`torii snapshot stash` actually saves working-tree changes** now. Previous impl copied `.git/` and reset `--hard`, silently dropping uncommitted edits because they aren't in `.git/objects` yet. Replaced with libgit2's native `Stash::save / pop`. `unstash` works against `stash@{0}` (default) or any index.
- **`torii remote create gitlab <user>/<repo>`** falls back to GitLab's `/users?username=` lookup when `/groups/<user>` 404s, so personal-namespace creates work alongside group ones.
- **`torii remote delete github`** now uses the GitHub REST API directly instead of shelling out to `gh` (which most users don't have installed). Surfaces clean errors on missing `delete_repo` scope (403) or unknown repo (404).
- **HTTPS auth body trim** (`cloud::short_body`) sliced by bytes, panicking when a server error message contained multi-byte UTF-8 straddling byte 200. Now slices by chars.
- **`torii scan` / `scan --history`** caps blob size at 5MB (override via `TORII_SCAN_MAX_BYTES`) so large generated assets don't OOM the scanner across long histories.

### Security
- **Hooks (`.toriignore [hooks]`) now require explicit one-time trust before executing.** Cloning a hostile repo could otherwise run arbitrary `sh -c …` on the very first `torii save`. On first encounter (or after the command list changes) torii prompts y/N with the commands printed verbatim; trust is persisted to `~/.config/torii/hook-trust.toml` keyed by repo path + command-list hash. Bypass with `TORII_TRUST_HOOKS=1` (CI), `TORII_NO_HOOKS=1` (skip), or `--skip-hooks`. Non-tty + untrusted refuses rather than silently running.

## [0.6.1] - 2026-05-09

### Added
- `torii auth login / status / whoami / logout` — manage gitorii.com API key for cloud features. Stored at `~/.config/torii/auth.toml` (chmod 600). Env override: `TORII_API_KEY`.
- `torii scan --commits` — enforce commit policy from `policies/commits.toml` (forbid/require trailers, forbid subjects, author email regex, length limits, conventional commits). `torii init` scaffolds a default policy.
- `torii history fsck` — recovery aid listing unreachable commits/blobs/trees after a destructive operation. `--show <oid>` prints content; `--restore <oid> --to <path>` writes a blob to disk.
- `torii log --graph` + always-on graph in TUI Log view. Lane-based ASCII rendering with five styles (`ascii`, `curves`, `heavy`, `bubbles`, `bubbles-x`) selectable from Settings.
- `torii remote create` accepts `owner/repo` to target an organization (GitHub/Gitea/Forgejo/Codeberg) or GitLab group/subgroup. Bare names keep current personal-namespace behaviour. `--namespace <OWNER>` flag is the explicit form.

### Changed
- `torii init` now writes default branch as `main` (config-driven via `git.default_branch`, no longer libgit2 default `master`).
- `torii sync --push --force` surfaces server-side rejections (branch protection, pre-receive hook decline) instead of reporting silent success.
- TUI sidebar drops view-switcher hotkeys (`g`, `l`, `b`, etc.) — they conflicted with in-view keys like `g` (graph). Navigation goes through the sidebar tabs.
- Commit policy schema migrated from Gate DSL to plain TOML — drops `gate-lang` dependency, simpler syntax.

### Fixed
- `torii history rebase --todo-file` with `reword` now actually rewrites the message (was silently equivalent to `pick`).
- `torii history rebase --continue / --abort / --skip` after a CLI-initiated `git rebase -i ... edit` pause (libgit2 `open_rebase` doesn't see CLI rebases; we now detect and shell out).
- Selected commit glyph in TUI no longer shrinks under `Modifier::BOLD` (some fonts lack a Regular bold variant for `⦿` etc).
- All compiler warnings (6 → 0) silenced with explicit `#[allow(dead_code)]` + comments.

## [0.6.0] - 2026-05-02

### Added
- **Pure-Rust HTTPS+SSH transports.** libgit2's libcurl/libssh2 transports replaced by custom impls registered via `git2::transport::register`. HTTPS over `reqwest` + `rustls`, SSH over `russh` + `aws-lc-rs`. Result: **build needs only a C compiler** — no perl, no openssl-dev, no libssh2-dev, no pkg-config.
- HTTPS auth via env vars per host: `GITHUB_TOKEN`, `GITLAB_TOKEN`, `CODEBERG_TOKEN`, `BITBUCKET_TOKEN`, `GITEA_TOKEN`, `FORGEJO_TOKEN`, `SOURCEHUT_TOKEN`. Generic fallback `TORII_HTTPS_TOKEN`.
- SSH auth chain: ssh-agent (`SSH_AUTH_SOCK`) → `~/.ssh/id_ed25519` → `~/.ssh/id_rsa`. Failure message lists each method tried.
- SSH host verification via `~/.ssh/known_hosts` (handles hashed entries and `[host]:port`). TOFU prompt on first connection if tty; `TORII_SSH_STRICT=1` to disable TOFU.
- Actionable HTTPS error messages distinguishing 401 (no auth / bad creds), 403 (forbidden), 404 (not found / not visible).
- Internal `crate::url::encode` helper (no `urlencoding` dep).

### Fixed
- **Silent push rejections.** `torii sync --push` previously printed `✅ Pushed to remote` even when the server rejected the update (branch protection, non-fast-forward without `--force`, pre-receive hook decline, missing permissions). libgit2's `remote.push()` returns Ok in those cases; rejections only surface via the `push_update_reference` callback. Now collected and reported as `push rejected by remote: <ref> → <reason>`. Bug pre-existed the transport rewrite and affected 0.5.0 too.

### Changed
- **Build deps reduced to just a C compiler.** No `perl`, `openssl-dev`, `libssh2-dev`, `make`, `cmake`. `pkg-config` optional.
- **Runtime deps:** `libz` (zlib) + libc only. No openssl, libssh2, libcurl.
- `git2` builds with `default-features = false` — libgit2 vendored without HTTPS/SSH (`GIT_HTTPS=0 GIT_SSH=0`).
- Bumped `reqwest` 0.11 → 0.12 with `rustls-tls`.
- `clap` pinned to `=4.5` to dodge a 4.6 crash in `Subcommand::augment_subcommands`.
- Direct deps trimmed: 18 → 14 (dropped `tokio` direct, `is-terminal`, `serde_yaml`, `urlencoding`).

### Notes
Validated end-to-end against **GitHub, GitLab, and Codeberg** (HTTPS + SSH, clone/fetch/push). Other forges (Bitbucket, Gitea, Forgejo, Sourcehut, SourceForge) speak the same Smart HTTP / SSH protocol so they should work, but have not been individually verified at push level. Please report issues at https://github.com/paskidev/gitorii/issues.

## [0.6.0-rc.2] - 2026-05-02 (yanked)

### Fixed
- README install/system-dependency sections still listed `perl`, `openssl-dev`, `libssh2-dev` and `pkg-config` from the pre-0.6 era. Updated to reflect that only a C compiler is required from source. Added a section for the `static` feature + musl target that produces a zero-runtime-deps binary. 0.6.0-rc.1 yanked because the README on crates.io misled testers into installing dependencies they no longer need.

## [0.6.0-rc.1] - 2026-05-02 (yanked)

### Added
- **Pure-Rust HTTPS+SSH transports** — libgit2's libcurl/libssh2 transports replaced by custom impls registered via `git2::transport::register`. HTTPS over `reqwest` + `rustls`, SSH over `russh` + `aws-lc-rs`.
- HTTPS auth via env vars per host: `GITHUB_TOKEN`, `GITLAB_TOKEN`, `CODEBERG_TOKEN`, `BITBUCKET_TOKEN`, `GITEA_TOKEN`, `FORGEJO_TOKEN`, `SOURCEHUT_TOKEN`. Generic fallback `TORII_HTTPS_TOKEN`.
- SSH auth chain: ssh-agent (`SSH_AUTH_SOCK`) → `~/.ssh/id_ed25519` → `~/.ssh/id_rsa`. Failure message lists each method tried.
- SSH host verification via `~/.ssh/known_hosts` (handles hashed entries and `[host]:port`). TOFU prompt on first connection if tty; `TORII_SSH_STRICT=1` to disable TOFU.
- Actionable HTTPS error messages distinguishing 401 (no auth / bad creds), 403 (forbidden), 404 (not found / not visible).
- Internal `crate::url::encode` helper (no `urlencoding` dep).

### Changed
- **Build deps reduced to just a C compiler.** No more `perl`, `openssl-dev`, `libssh2-dev`, `make`, `cmake`. `pkg-config` optional (used to find system libgit2/zlib; falls back to vendored).
- **Runtime deps:** `libz` (zlib) and libc only. No openssl, no libssh2, no libcurl.
- `git2` builds with `default-features = false` — libgit2 vendored without HTTPS/SSH support (`GIT_HTTPS=0 GIT_SSH=0`).
- Bumped `reqwest` 0.11 → 0.12 with `rustls-tls`.
- `clap` pinned to `=4.5` to dodge a 4.6 crash in `Subcommand::augment_subcommands`.
- Direct deps trimmed: 18 → 14 (dropped `tokio` direct, `is-terminal`, `serde_yaml`, `urlencoding`).

### Notes for testers
This is a release candidate. The transport rewrite is a major internal change. Validated against GitHub clone/fetch/push over HTTPS (with token) and SSH (with ed25519 + known_hosts). Other forges (GitLab, Codeberg, Bitbucket, Gitea, Forgejo, Sourcehut) use the same Smart HTTP/SSH protocol so they should work, but have not been individually verified at push level. Please report issues at https://github.com/paskidev/gitorii/issues.

## [0.5.0] - 2026-04-28

### Added
- `.toriignore.local` — machine-private overlay for sensitive ignore rules. Auto-gitignored, never committed. Merges on top of `.toriignore`; tighter local size limits override public ones.
- `torii ignore add|secret|list` — manage rules from the CLI. `secret` defaults to `.local` (private); `--public` writes to committed `.toriignore` with a recon-warning.
- `[secrets]`, `[size]`, `[hooks]` sections in `.toriignore` for declarative pre-save/sync gates (custom regex secret rules, file-size limits, hook commands).
- Update banner in TUI header when a newer crates.io version is available.
- CLI update notifier on crates.io releases.

### Changed
- Command surface tightened: promoted `blame`/`scan`/`cherry-pick` to top-level; demoted `ls`/`unstage`/`repo`; consolidated `mirror primary`/`replica`.

### Fixed
- `torii pull` branch handling, `remote link`, `unstage`, `rebase --root`, `amend` after history rewrite, `branch --orphan`, `save` flag combinations.

## [0.1.16] - 2026-04-19

### Changed
- Updated LICENSE to TSAL-1.0 (Torii Source-Available License)
- CI pipeline now only triggers on version tags, no branch pipelines

## [0.1.15] - 2026-04-19

### Fixed
- `torii sync`, `torii fetch`, `torii push tags` now authenticate with HTTPS token (gitlab_token, github_token, etc.) — previously only SSH was supported, causing auth failures on HTTPS remotes
- GitLab CI pipeline no longer fails with exit code 22 when release already exists (409 treated as success)
- Pipeline now only triggers on version tags (`vX.Y.Z`), suppressing unwanted branch pipelines

## [0.1.8] - 2026-04-17

### Fixed
- Corrected license metadata in Cargo.toml to reference LICENSE file instead of MIT

## [0.1.7] - 2026-04-17

### Fixed
- `torii sync --force` and `torii sync --push` now also sync replica mirrors automatically
- Renamed remaining internal `slaves` variable references to `replicas` in mirror output
- Mirror list output now shows `PRIMARY` instead of `MASTER`

## [0.1.6] - 2026-04-16

### Added
- `torii sync` now automatically pushes to all configured replica mirrors after syncing with origin — no need to run `torii mirror sync` manually

### Fixed
- Removed unused `mut` on `index` variable in rebase loop
- Removed unused `repo_path` variable in `show` command

## [0.1.5] - 2025-04-16

### Fixed
- Full Windows and macOS native compatibility — removed all `HOME` env var hardcoding
- Replaced all `Command::new("git")` subprocesses with native `git2` API calls
- SSH credential resolution now uses `dirs::home_dir()` for cross-platform paths
- Config dir now uses platform-native path via `dirs::config_dir()` (Linux XDG, macOS `~/Library`, Windows `%APPDATA%`)
- `torii snapshot restore` uses git2 hard reset instead of subprocess
- `torii snapshot stash/unstash` uses git2 index and reset instead of subprocess
- `torii history reflog` uses git2 reflog API
- `torii history revert/reset/merge/rebase` fully ported to git2
- Tags pushed via git2 enumeration instead of `git push --tags` subprocess
- `OpenSSL` vendored in `git2` dependency for Windows native builds

### Changed
- `git2` dependency updated to include `vendored-openssl` feature for Windows support

## [0.1.4] - 2025-03-15

### Changed
- Renamed `master`/`slave` mirror terminology to `primary`/`replica` across all commands and output
  - `torii mirror add-master` → `torii mirror add-primary`
  - `torii mirror add-slave` → `torii mirror add-replica`

### Fixed
- Platform-native config path for token storage (was hardcoded to Linux `~/.config`)

## [0.1.3] - 2025-03-01

### Added
- Platform shorthand syntax for `torii clone`: `torii clone github user/repo`
- `torii ls [PATH]` — list tracked files in the index
- `torii show [OBJECT]` — show commit, tag or file details
- `torii history` subcommand group consolidating 7 previously top-level commands

### Fixed
- `torii history remove-file` now works on directories (`-r` flag)
- Wildcard matching in `.toriignore`
- Removed dead `integrate` code

## [0.1.2] - 2025-02-15

### Changed
- Collapsed 7 top-level history-related commands into `torii history` subcommands for a cleaner CLI surface

### Fixed
- Repo URL in Cargo.toml

## [0.1.1] - 2025-02-01

### Added
- `torii history remove-file` — permanently erase a file from the entire git history

### Fixed
- Scanner now detects sensitive filenames (`.env`, `*.pem`, `id_rsa`, etc.)
- Scanner extended with Spanish-language placeholder detection
- Reduced false positives in sensitive data scanner
- Mirror sync now pushes tags alongside branch refs
- GitHub remote creation uses REST API instead of shelling out to `gh` CLI
- Support for root commit in empty repositories
- `.toriignore` wildcard matching and ref handling
- Explicit SSH key used for mirror sync

### Changed
- `.gitignore` renamed to `.toriignore` — Torii manages its own ignore file
- Custom workflows moved to `torii-premium`
- Entire `.torii/` directory excluded from tracking
- Crate renamed to `gitorii` for crates.io publication

## [0.1.0] - 2025-01-15

### Added
- Core git operations: `torii init`, `torii clone`, `torii save`, `torii sync`, `torii status`, `torii diff`, `torii log`
- Branch management: `torii branch`, `torii switch`, `torii merge`
- Snapshot system: `torii snapshot create/list/restore/stash/unstash`
- Multi-platform mirror sync: `torii mirror add-primary/add-replica/list/sync`
- Remote repository management: `torii remote create/delete/list` (GitHub, GitLab, Gitea, Forgejo, Codeberg, Sourcehut, SourceForge)
- Tag management and auto-versioning: `torii tag`
- Built-in sensitive data scanner (pre-save and history scan)
- History rewriting: `torii history rewrite/rebase/cherry-pick/reflog/blame`
- Custom config system: global (`~/.config/torii`) and local (`.torii/`)
- `.toriignore` support synced to `.git/info/exclude`
- SSH authentication helper
- Duration parsing utilities (`10m`, `2h`, `1d`)
- Multi-platform URL generation (SSH and HTTPS)
- Autofetch configuration for mirrors

[Unreleased]: https://gitlab.com/paskidev/torii/-/compare/v0.1.8...HEAD
[0.1.8]: https://gitlab.com/paskidev/torii/-/compare/v0.1.7...v0.1.8
[0.1.7]: https://gitlab.com/paskidev/torii/-/compare/v0.1.6...v0.1.7
[0.1.6]: https://gitlab.com/paskidev/torii/-/compare/v0.1.5...v0.1.6
[0.1.5]: https://gitlab.com/paskidev/torii/-/compare/v0.1.4...v0.1.5
[0.1.4]: https://gitlab.com/paskidev/torii/-/compare/v0.1.3...v0.1.4
[0.1.3]: https://gitlab.com/paskidev/torii/-/compare/v0.1.2...v0.1.3
[0.1.2]: https://gitlab.com/paskidev/torii/-/compare/v0.1.1...v0.1.2
[0.1.1]: https://gitlab.com/paskidev/torii/-/compare/v0.1.0...v0.1.1
[0.1.0]: https://gitlab.com/paskidev/torii/-/releases/tag/v0.1.0
