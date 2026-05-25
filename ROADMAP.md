# Torii ⛩️ — Roadmap

Living document. Sections are prioritized; within a section items are ordered by impact.
"Released" = on crates.io. "In progress" = work started or imminent. "Future" = direction, not commitment.

---

## Released

### v0.7.18 — Azure DevOps (May 2026)
- 7th platform: Azure DevOps Repos / Pipelines / Releases / Work Items
- URL detection: `dev.azure.com`, `ssh.dev.azure.com`, legacy `*.visualstudio.com`. Three-level path (`org/project/repo`) packed into the standard `(platform, owner, repo)` triple via `split_azure_owner()`
- Auth: PAT via Basic with empty user (`Basic base64(":PAT")`). New provider `azure` in `torii auth`; env fallbacks `AZURE_DEVOPS_TOKEN` / `AZURE_DEVOPS_EXT_PAT` / `AZDO_TOKEN`
- PR: full surface, merge methods map to noFastForward / squash / rebase
- Work Items (≈ issues): list / create / close / comment via WIQL + JSON-Patch
- Pipelines (Builds API): list / cancel / retry / delete + list_jobs / job_log / artifacts download
- Releases (classic on `vsrm.dev.azure.com`): list / get / delete
- Artifacts: stub — feeds live at the org level, addressing model needs a separate design pass

### v0.7.17 — Bitbucket Cloud (May 2026)
- 6th platform: Bitbucket Cloud detected from `bitbucket.org` URLs
- Auth heuristic: tokens containing `:` are treated as `user:app_password` (Basic header, base64); else `Bearer` (OAuth). New dep `base64 = "0.22"`
- PR: full surface, merge methods map to merge_commit / squash / fast_forward
- Issues (deprecated-but-functional): list / create / close / comment
- Pipelines (Bitbucket Pipelines REST): list / cancel / list_jobs / job_log. retry / delete / per-step retry+cancel surface clear "not exposed via REST" errors
- Releases / Packages: stubs — Bitbucket Cloud has no Release-page object, no Package Registry

### v0.7.16 — Radicle + `src/` reorg (May 2026)
- 5th platform: Radicle peer-to-peer hosting via `rad` subprocess
- Detection: `rad://` or `rad@` URLs. RID stored in `owner`, `repo` empty (Radicle projects are flat)
- New `src/radicle.rs` wraps the `rad` binary (`run_rad`, `run_rad_json` with NDJSON support)
- Issues + Patches (≈ PRs) wired via `rad issue / patch`. close / comment / merge return clear errors because torii's traits take `u64` ids and Radicle uses content-addressed hashes
- Pipelines / Releases / Packages: clear "not native" errors (no CI, no release object, no package registry on Radicle)
- **Internal**: `src/` reorganised from 42 flat files into 5 sub-modules (`platforms/`, `vcs/`, `cmd/`, `workspace/`, `util/`) + the existing `cloud/`, `transport/`, `tui/`, `versioning/`. `pub use` re-exports in `main.rs` keep all old `crate::pr::`-style call-sites compiling without a sweep

### v0.7.15 — Sourcehut + GPG sign full coverage (May 2026)
- 4th platform: Sourcehut detected from `git.sr.ht` URLs. Issues (`todo.sr.ht`) and Pipelines (`builds.sr.ht`) wired; PR returns email-patch workflow guidance, Release/Package return "not native" errors
- GPG signing extended to **every** commit-creation site: cherry-pick / revert / merge / history reauthor / history rewrite / history remove-file. Routed via `commit_inner` / new `commit_inner_split` (author ≠ committer for the rewrite ops)
- Annotated-tag signing still goes unsigned; `tag_create_buffer` + `tag_signed` flow tracked for later

### v0.7.14 — Config & GPG bug fixes (May 2026)
- **Fix**: `user.name` / `user.email` set with `--local` were silently ignored at commit time. `resolve_signature` only read the global config — now loads local merged over global. Bug introduced in 0.7.3
- **Fix**: `git.sign_commits = true` was a no-op since 0.6.x — the config flag was accepted but never honoured. New `src/gpg.rs` shells out to the system `gpg` binary (same UX as `git commit -S`); `commit_inner` helper threads the signed path through `torii save` and the TUI commit view
- Aliases `user.signingkey` / `commit.gpgsign` map to `git.gpg_key` / `git.sign_commits` (git-friendly spellings)
- **Refactor**: shared HTTP helpers in new `src/http.rs` (`make_client`, `send_json`, `send_empty`, `extract_array`). Platform clients drop ~520 lines of duplicated send/parse/error boilerplate. Cleanup of obsolete in-repo docs and stray logs (~1000 more lines)

### v0.7.13 — Gitea / Codeberg / Forgejo (May 2026)
- 3rd platform: detection of `codeberg.org` routes through a new Gitea client; the same client serves Forgejo and self-hosted Gitea (API is identical)
- Auth: `torii auth set codeberg|gitea|forgejo` — all three names share a single client (`resolve_gitea_token` tries them in order)
- PR / Issue / Pipeline (Gitea Actions ≥ 1.19) / Release: full surface

### v0.7.12 — TUI Platform view + mirror fix (May 2026)
- Unified Platform view in the sidebar groups pipelines / jobs / releases / packages into one drill-down interface with horizontal sub-tabs. `r` opens a centred remote-selector popup; `Ctrl+R` reloads
- Drill-down: Enter on a pipeline → its jobs; Enter on a job → scrollable log
- Fix: `add mirror` was only reachable when a mirror was already selected (catch-22). Now exposed from the git-remote ops dropdown too

### v0.7.0 – v0.7.11 — Porcelain coverage + CI control plane (May 2026)
- `torii pipeline / job / package / release` CLI commands wrapping GitHub Actions and GitLab Pipelines surfaces (list / cancel / retry / delete + log / artifacts)
- `--remote NAME` global flag on the four platform commands — addresses multi-platform mirrored repos
- `torii sync --fetch [remote] [--all]` for fork workflows
- `torii auth` becomes the single home for cloud key + per-platform tokens. `torii publish` wrapper around `cargo publish` that injects `auth.cargo`. Storage migrated to `~/.config/torii/auth.toml`
- `resolve_signature()` ends the "Torii User" fallback bug — torii config / git config chain / hard error
- Snapshot leak fix: snapshots moved to `<gitdir>/torii/snapshots/` (inside `.git/`) so `add_all` can't pull them into commits. Bug reported in tramuntana (681 MB commit, push aborted)
- 4 new TUI views (Worktree / Submodule / Bisect / Auth), 3 sidebar fusions (Log+History / Remote+Mirror / Config+Settings)
- CI / build hardening: pinned `russh = "=0.60.2"` (avoids NASM mandate from aws-lc-sys 0.41), Windows cfg gates on SSH-agent calls, shared GitLab runners, cross-compile to linux x86_64 + linux aarch64 + windows x86_64

### v0.6.0 — Pure-Rust transports (May 2026)
- Custom HTTPS transport over `reqwest` + `rustls`, replacing libgit2's libcurl path
- Custom SSH transport over `russh` + `aws-lc-rs`, replacing libgit2's libssh2 path
- libgit2 vendored without HTTPS/SSH support (`GIT_HTTPS=0 GIT_SSH=0`)
- HTTPS auth via env vars; SSH auth chain (agent → ed25519 → rsa); host verification via known_hosts with TOFU prompt
- Build deps reduced to a C compiler — no perl, no openssl-dev, no libssh2-dev, no pkg-config
- Fix: silent push rejections — `remote.push()` returns Ok even when the server rejects; now reported as `push rejected by remote: <ref> → <reason>`

### v0.5.0 — Declarative gates + machine-private overlay (April 2026)
- `.toriignore.local` — machine-private overlay, auto-gitignored
- `torii ignore add | secret | list`
- `[secrets]`, `[size]`, `[hooks]` sections in `.toriignore`
- TUI update banner / CLI update notifier
- Command surface tightened: promoted `blame`/`scan`/`cherry-pick`, demoted `ls`/`unstage`/`repo`

### v0.4.0 and earlier — Core git surface
- `save / sync / status / log / diff / branch / clone / cherry-pick / blame`
- Rebase: `--continue / --abort / --skip`, interactive, `--todo-file`, `--root`
- Snapshots: `create / list / restore / delete / stash / unstash / undo`, auto-snapshot
- History: `rewrite / clean / verify-remote / reflog / remove-file`
- Tags: `create / list / delete / push / show / release` (auto-bump from conventional commits, `--bump`, `--dry-run`)
- Scanner: staged + history (`--history`), pre-save hook
- Mirrors: GitHub, GitLab, Codeberg, Bitbucket, Gitea, Forgejo, Sourcehut, SourceForge, custom servers; primary/replica model; autofetch
- Remote management: `remote create / delete / visibility / configure / info / list` (GitHub + GitLab only; Gitea/Forgejo/Codeberg stubs since 0.4.0)
- Workspace: batch operations across multiple repos
- Config: global + local, `set / get / list / edit / reset`
- Custom workflow aliases: `custom add / list / run / remove`
- TUI: PR/MR view, commit amend from TUI, branch/tag search, background loading

---

## In progress / next

### v0.7.19 — Visibility everywhere
Right now `torii remote visibility` only works on GitHub and GitLab. Wire the same surface across:
- Gitea / Codeberg / Forgejo (shared API: `PATCH /api/v1/repos/{owner}/{repo}` with `private: true|false`)
- Bitbucket Cloud (`PUT /2.0/repositories/{ws}/{repo}` with `is_private: true|false`)
- Sourcehut (PUT on `meta.sr.ht` GraphQL or REST shape, depending on what's stable)
- Azure DevOps: visibility is a *project*-level setting on Azure, not per-repo — surface a clear error pointing the user at the project settings
- Radicle: peer-to-peer, no central visibility — clear error

### v0.8.0 — `platforms.toml` + `torii ci configure`
- `~/.config/torii/platforms.toml` — user-declared platform hosts (self-hosted Gitea, self-hosted Sourcehut, Bitbucket Data Center). URL → platform mapping is currently hardcoded; this opens it up
- `torii ci configure` — guided setup for each platform's CI from a single command. Replaces the manual `.gitlab-ci.yml` / `.github/workflows/*` boilerplate
- Recommend (don't enforce) `cargo install --locked gitorii` in README — without `--locked`, install ignores the published Cargo.lock and may resolve fresh transitives including broken `-rc` crypto chains

### v0.6.1 — Static binary (branch `feat/static-binary`, ready)
- `static` Cargo feature → vendored zlib via `libz-sys/static`
- Build target `x86_64-unknown-linux-musl` produces a binary with **zero runtime libs** (runs on Alpine, scratch, busybox, any glibc/musl mix)
- GitLab CI gains `build-linux-x86_64-musl` job

### Validation and polish
- Push validation against Bitbucket, Gitea, Forgejo, Sourcehut (transports expected to work — same Smart HTTP/SSH protocol — but not individually verified end-to-end yet)
- SSH passphrase prompt for encrypted disk keys (currently only unencrypted keys + agent)
- `~/.ssh/config` parsing (HostName / User / IdentityFile / Port aliases)
- Annotated-tag GPG signing (`tag_create_buffer` + `tag_signed`)
- Secret scanner `--skip-secrets` / inline allowlist for false positives (test fixtures with regex patterns currently trigger the pre-commit hook)
- Re-enable GitHub Actions for auto-publish (currently manual `cargo publish` from local; rustc 1.95 ICEa on the build verify path with the russh crypto-rc tree — `--no-verify` workaround)
- Integration tests against a local git server (so transport regressions are caught in CI)

### Platform follow-ups (post-0.8.0)
- Azure Artifacts: org-level feed addressing doesn't fit cleanly into torii's owner/repo abstraction; needs a separate design pass
- Bitbucket Data Center (self-hosted Bitbucket): different URL shape, partly different API at `/rest/api/1.0/`
- Gitea Package Registry: API exists, just not wired

### Monetization (gitorii.com)
- Paddle integration (schema ready, integration pending)
- Indie (free) / Scale (20€/mo) / Teams (30€/mo) / Seed (10€/mo) / Enterprise (custom)
- Target: 17–20k€ ARR

---

## Future

### Gate — CI/CD transpiler (separate repo: `gate`)
A DSL that compiles to GitHub Actions / GitLab CI / CircleCI / Azure / Bitbucket Pipelines YAML. Not a runner — pure transpiler. Open-source docs, no paywall. Integrates with torii so `torii sync` can validate CI is in sync with the source DSL.

### torii-cloud (premium plugin)
Signed-binary plugin discovered in PATH that gates premium features on a renewable license:
- AES-GCM-encrypted plugin binary, Ed25519-signed license JWT
- Online activation, online renewal, offline grace period
- Hardware fingerprint with N-device limit, force-reactivate UX
- Open-core relationship: `gitorii` keeps working forever without the plugin

### AI generation
- AI-assisted commit messages from staged diff
- Conventional-commits validator + suggester
- Natural-language "what changed in the last week" summaries

### TUI / GUI roadmap
- TUI: PR/MR creation flow, conflict resolution UI, snapshot diff browser
- TUI Tier B + C: ops dropdowns (worktree / submodule / auth / bisect), interactive bisect, log+history dropdown, auto-tagging toggle in Tags view
- Tauri GUI long-term — natural-language interface with embedded console fallback

### New VCS
Long horizon. Torii is currently a git wrapper. The long-term goal is a VCS designed around human workflows rather than git's technical model. Torii would provide the migration path.

### Scanner improvements
- `scan --fix` — auto-remove detected secrets from staged files
- Custom regex patterns via `.toriignore` `[secrets]` (already partially shipped in 0.5.0; expand)
- Pre-push scan, not just pre-save
- Org-wide policy file shared across repos

### Interactive staging
- `torii save --patch` — stage hunks interactively (`git add -p` equivalent)

### Log improvements
- `torii log --graph` — visual branch graph (already in TUI; CLI option pending)
- `torii log -S <string>` — pickaxe search

### Snapshot improvements
- Compression for old entries
- Remote snapshot backup (private bucket, opt-in)
- Diff between two snapshots
- Fix: `torii snapshot stash` known bug — sometimes reports success without saving (use `snapshot create -n` as workaround)

### Migration paths
- `torii migrate` — guided import from a `git` workflow (rename familiar commands, set up `.toriignore` from existing `.gitignore`, etc.)
- Long-term: migration paths to the new VCS once it exists

---

## Considered, not planned

These come up but are deliberately out of scope.

- **gitoxide migration.** Evaluated May 2026: push incomplete, rebase API unstable, no production users. Revisit late 2026 / early 2027 once stabilized.
- **AWS CodeCommit support.** AWS deprecated new repos in 2024.
- **Mercurial / Bazaar / Pijul / Fossil interop.** git-only for the foreseeable future.
- **Web UI hosted on gitorii.com that browses your repos.** That's GitHub/GitLab/Codeberg's job. gitorii.com stays a marketing + docs + billing site.
- **`gitorii-experimental` AUR package.** Considered alongside `gitorii` stable; dropped to keep the maintenance surface small (1 dev).

---

*Last updated: 2026-05-25 (after v0.7.18 ship — 7 platforms supported)*
