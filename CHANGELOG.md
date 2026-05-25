# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.16] - 2026-05-25

### Added

- **Radicle support** (5th platform, peer-to-peer). Detection: any
  remote URL starting with `rad://` or `rad@` routes through the
  Radicle client. RID is parsed into the `owner` slot; `repo` is left
  empty because Radicle projects are flat (no owner/repo split).
  - New `src/radicle.rs` module wraps the local `rad` binary via
    `run_rad(args)` and `run_rad_json(args)`. Same shape as
    `src/gpg.rs`: subprocess + clear error if the binary is missing
    (link to https://radicle.xyz).
  - **Issues**: `rad issue list / open` work. `close` and `comment`
    return clear errors because Radicle identifies issues by hash,
    not by `u64`, and torii's `IssueClient` trait still takes
    `number: u64`. A future revision of the trait (string-id variant)
    will close that gap.
  - **Patches** (Radicle's PR equivalent): `rad patch list / open`
    work. `get / merge / close / update / delete_branch` return clear
    errors pointing at `rad patch <op> <hash>` for the same hash-vs-
    number reason.
  - **Pipelines / Releases / Packages**: clear errors — Radicle has
    no native CI, no Release-page object, no package registry.
    Mirror to a host that does, or run CI locally.

### Notes

- Supported platforms now: **GitHub, GitLab, Gitea / Codeberg /
  Forgejo, Sourcehut, Radicle** — five. The roadmap target for the
  "0.7.13 → 0.7.16 multi-platform expansion" is complete.

## [0.7.15] - 2026-05-25

### Added

- **Sourcehut platform support** (4th platform after GitHub / GitLab /
  Gitea). Detection: `git.sr.ht` URLs auto-route through the Sourcehut
  client. Auth: `torii auth set sourcehut <oauth-token>` (env fallback
  `SOURCEHUT_TOKEN` / `SRHT_TOKEN`). What's wired:
  - **Issues** (`todo.sr.ht`): `list / create / close / comment` via
    the REST tracker API. Assumes `tracker_name == repo_name` — if your
    project uses split trackers (e.g. `~user/repo-bugs`,
    `~user/repo-features`), the `--remote` flag of `torii issue` points
    at the correct one.
  - **Pipelines** (`builds.sr.ht`): `list / cancel` + `log` work. Other
    surface is honest about the limits:
    - `retry` / `job_retry` return an error pointing at the web UI —
      builds.sr.ht doesn't expose a "resubmit finished job" endpoint
      over REST.
    - `delete` returns an error — builds keep retention-policy-managed
      on the server, not user-deletable.
    - `job_artifacts_download` and `job_erase` return clear errors.
    - "Jobs in a pipeline" is a flat concept on builds.sr.ht — a job
      *is* the pipeline. `list_jobs(pid)` returns the run as a single
      job entry to keep the CLI surface uniform.
  - **PRs**: returns a clear error explaining sourcehut's email-patch
    workflow (`*-devel@lists.sr.ht`) and pointing the user at
    `torii patch export`.
  - **Releases**: returns an error explaining that sourcehut has no
    native release object — a release is just an annotated git tag.
  - **Packages**: same — no package registry exists on sourcehut.

### Changed

- **GPG signing now applies everywhere a commit is created**, not just
  `torii save` and the TUI commit view. Routed `commit_inner` /
  `commit_inner_split` through:
  - `cherry-pick` (and `--continue`)
  - `revert`
  - `merge` (the merge commit itself)
  - `history reauthor` (rewritten commits)
  - `history rewrite` (date rewrites)
  - `history remove-file` (filter-branch replacement)

  Tag-create-annotated and the workspace.save fanout still go through
  the unsigned path — annotated tag signing needs `tag_create_buffer`
  + `tag_signed` (different libgit2 dance) and is tracked separately.

### Internal

- New `commit_inner_split(repo, ref, author, committer, msg, tree,
  parents)` variant of `commit_inner` for callers that preserve the
  original author when rewriting the committer.

## [0.7.14] - 2026-05-25

Bug-fix release for two long-standing config issues plus an internal
refactor that drops ~1500 lines from the platform-client surface.

### Fixed

- **`user.name` / `user.email` from `--local` config are now actually
  used at commit time.** Previously `resolve_signature` only read
  `~/.config/torii/config.toml` (the global file), so
  `torii config set user.email "X" --local` would write `.torii/config.toml`
  successfully but `torii save` would still use the global value at
  commit creation. Per-repo identity now wins, as expected for a
  jerárquico config system. Bug introduced in 0.7.3 when
  `resolve_signature` was added; surfaced by the user trying to set a
  work-only email on a single repo.
- **GPG-signed commits actually sign now.** `git.sign_commits = true`
  was accepted by the config layer since 0.6.x but never honoured at
  commit time — every commit went out as `repo.commit(...)` with no
  signature, leaving `gpgsig` absent from the object even though the
  flag was on. The fix:
  - New `src/gpg.rs` shells out to the system `gpg` binary
    (`--detach-sign --armor -u <key>`), reusing the user's existing
    keyring + agent + pinentry — same UX as `git commit -S`.
  - New `commit_inner` helper in `core.rs` routes both `torii save` and
    the TUI's commit view through the signed path when the flag is on
    (`commit_create_buffer` + `commit_signed` + manual ref update,
    since libgit2 doesn't update refs for signed commits).
  - Other commit sites (`cherry-pick`, `revert`, `merge`,
    `tag annotated`, `history reauthor`) **still go through the
    unsigned path** — extending `commit_inner` there is tracked for
    0.7.15.
  - Requires `gpg` (or `gpg2`) on `PATH`. Clear error if missing.

### Added

- **`user.signingkey` and `commit.gpgsign` config aliases.** They map
  to the existing `git.gpg_key` and `git.sign_commits` respectively.
  Either spelling works — pick the one you already use in `git config`.
  Reason: the previous `git.gpg_key` name was a torii-ism nobody
  guessed when migrating from git.

### Internal

- **HTTP boilerplate centralised** in a new `src/http.rs` module with
  four helpers (`make_client`, `send_json`, `send_empty`,
  `extract_array`). The 15 platform clients (`pr` × `issue` ×
  `pipeline` × `release` × `package` over GitHub × GitLab × Gitea) all
  go through it now. Net effect:

  | file        | before | after | Δ    |
  |-------------|-------:|------:|-----:|
  | pr.rs       |    750 |   566 | -184 |
  | issue.rs    |    463 |   331 | -132 |
  | pipeline.rs |   1017 |   901 | -116 |
  | release.rs  |    530 |   393 | -137 |
  | package.rs  |    300 |   260 |  -40 |
  | http.rs     |      0 |    89 |  +89 |
  | **total**   | **3060** | **2540** | **-520** |

  No behaviour changes; only collapses identical send / status-check /
  parse / format-error blocks. Error messages keep the same structure
  via the `ctx: &str` parameter.

- **Dropped obsolete in-repo docs and a stray debug log** (~1000 more
  lines): `docs/BUG_COMMIT_AUTHOR_FALLBACK.md` (FIXED in 0.7.3),
  `docs/BUG_SNAPSHOT_LEAKS_INTO_COMMITS.md` (FIXED in 0.7.7),
  `docs/FEATURE_FETCH_SPECIFIC_REMOTE.md` (shipped in 0.7.6),
  `log_gitorii.txt`, `.pr-test`.

## [0.7.13] - 2026-05-20

### Added

- **Gitea / Codeberg / Forgejo support** — first new platform since
  0.1.x. All four platform-side surfaces (`pr`, `issue`, `pipeline`,
  `release`) get a Gitea client; the four `torii pipeline / job /
  package / release` CLI commands and the corresponding TUI Platform
  view recognise `codeberg.org` remotes automatically and route
  through it. Self-hosted Gitea / Forgejo instances need explicit
  declaration via `~/.config/torii/platforms.toml` — that arrives in
  0.8.0; for now they fall through with "platform not detected".
- **`torii auth set codeberg <token>`** (or `gitea` / `forgejo`) all
  share the same client — set the token under whichever name fits
  your mental model. Env-var fallbacks: `CODEBERG_TOKEN`,
  `GITEA_TOKEN`, `FORGEJO_TOKEN`.
- **Gitea Actions** (CI runs in Gitea ≥ 1.19 / Forgejo) supported by
  `torii pipeline list / cancel / retry / delete` and `torii job
  list / log / retry`. `job cancel` / `job artifacts download` /
  `job erase` are not exposed by the Gitea v1 API and return a clear
  "use the run-level op instead" error.

### Notes

- This is the first of the planned **multi-host expansion** (0.7.13
  Gitea, 0.7.14 Sourcehut, 0.7.15 Radicle). The pattern lets the
  client surface grow without changing the CLI grammar.

## [0.7.12] - 2026-05-20

### Added

- **TUI: unified Platform view.** New `platform` entry in the sidebar
  (between `auth` and `config`) groups the four platform-side surfaces
  (`pipeline` / `job` / `release` / `package`) into one drill-down view
  with horizontal sub-tabs:
  - `1` Pipelines, `2` Jobs, `3` Releases, `4` Packages
  - **Enter on a pipeline** drills into its jobs (sub-tab auto-switches
    to Jobs, populated with that pipeline's jobs).
  - **Enter on a job** fetches and shows its log in a scrollable panel
    (PageUp / PageDown / Home for navigation; Esc walks back).
  - **`r` opens a centred popup** to switch the remote
    (auto-discovered from the repo); `Enter` selects, `Esc` cancels.
  - **`Ctrl-R` reloads** the active sub-tab.
  - All loads happen on background threads — the TUI stays responsive
    while pipelines / jobs / releases / packages are fetched.

### Fixed

- **TUI: `add mirror` was unreachable.** The "add mirror" entry only
  appeared in the ops dropdown when a mirror was already selected,
  making it impossible to create the first mirror from the TUI. It's
  now also exposed from the git-remote ops dropdown, so users can
  bootstrap mirroring from scratch.

### Docs

- README: refreshed the **Views** table (was missing worktrees /
  submodules / pr / issues / bisect / auth from earlier releases) and
  added the new `platform` row + short Platform-view description.
- README: cross-linked the **CI / platform management** CLI section to
  the new TUI Platform view.

## [0.7.11] - 2026-05-20

### Added

- **`--remote NAME` on every platform-management command.** The four CLI surfaces added in 0.7.7 and 0.7.10 (`torii pipeline`, `torii job`, `torii package`, `torii release`) all auto-detected the platform from the `origin` remote URL. That works when there's only one platform — most projects — but breaks the multi-platform case where a repo is mirrored across e.g. GitLab (`origin`) and GitHub (`github-paskidev`). Each backend has its own releases, its own pipeline runs, its own packages; both should be reachable from the CLI.

  All four commands now accept an optional `--remote NAME` flag (global within the subcommand, so it can appear before or after the operation verb). Default `origin` — no breaking change for existing users.

  ```sh
  # Default: origin
  torii release list

  # Explicit: same as default
  torii release list --remote origin

  # The mirror's side
  torii release list --remote github-paskidev

  # Works inside subcommands too thanks to `global = true`
  torii pipeline delete --status failed --remote github-paskidev --yes
  ```

  Internals: a new `detect_platform_from_remote_named(repo_path, remote_name)` in `src/pr.rs` wraps the existing detection logic with the remote name as a parameter; the original `detect_platform_from_remote` becomes a thin shim that delegates with `"origin"`. The four dispatch arms in `src/cli.rs` were updated to destructure `remote` from their `Commands::*` variant and pass it through.

### Why this release exists

This is a small targeted patch that lays the groundwork for two larger pieces of work coming up:

1. **TUI Platform hub (0.7.12)** — a single sidebar entry that wraps all four surfaces with tabs per remote. The auto-discovery of the user's configured remotes drives the per-remote tabs, and the underlying CLI now actually supports per-remote dispatch (which it didn't before).

2. **`torii ci configure` + `~/.config/torii/platforms.toml` (0.8.0)** — elevates platforms to first-class citizens with an explicit config rather than always inferring from `origin`. The `--remote NAME` flag stays as the per-invocation override.

## [0.7.10] - 2026-05-20

### Added — Platform Management Surface

Three new top-level commands giving CLI access to GitLab/GitHub platform-side state we previously had to reach with `curl` + the web UI. All three auto-detect the platform from the `origin` remote, mirroring the pattern from `torii pipeline` (0.7.7) and `torii pr` / `torii issue`.

- **`torii job {list, log, retry, cancel, artifacts, erase}` — individual CI job control.** Sibling to `torii pipeline` (which manages whole pipelines / workflow runs); `torii job` drills into the jobs inside a pipeline.
  - `torii job list --pipeline <id> [--status STATUS]` — enumerate jobs of one pipeline, optionally filtered. Output is a one-line-per-job table with icon, raw status, name, stage, duration. The status filter is normalized (`success | failed | running | canceled | pending`) and applied client-side after the fetch so the same flag means the same thing on both backends.
  - `torii job log <id> [--tail N]` — **the killer feature.** Fetches the raw trace and prints it. With `--tail N` only the last N lines are printed, which is the common case during failure post-mortems. Replaces the previous "open the UI, click into the job, scroll to the bottom of the log" round-trip. During the v0.7.9 saga this would have saved ~30 minutes of `curl /jobs/<id>/trace | tail -20` repetition.
  - `torii job retry <id>` — GitLab only. Re-runs a single failed job without re-running the entire pipeline (which on a 25-minute matrix is the difference between "5 min retry" and "25 min retry"). GitHub Actions doesn't support per-job retry (only `/runs/:run_id/rerun-failed-jobs` at the run level); the GitHub backend returns a hint pointing at `torii pipeline retry <run-id>`.
  - `torii job cancel <id>` — GitLab only, same asymmetry as `retry`.
  - `torii job artifacts <id> [-o <path>]` — GitLab only. Downloads the per-job artifacts archive to disk. On GitHub artifacts are run-scoped, not job-scoped; the backend returns an error explaining this. Default output path is `./<job-id>-artifacts.zip`.
  - `torii job erase <id> [--yes]` — GitLab only. Clears a job's log + artifacts but keeps the job entry visible in the UI (useful for storage cleanup when you want history). GitHub returns unsupported.
  - Trait impl in `src/pipeline.rs` — extends the existing `PipelineClient` trait rather than adding a parallel `JobClient` trait, since jobs are conceptually under pipelines and share the same auth + base URL. Asymmetric capabilities are handled by `Err` returns with self-explanatory hints (no silent fallback, no panics on unsupported ops).

- **`torii package {list, files, delete}` — GitLab Package Registry management.** GitLab's Generic Package Registry stores release binaries between runs (gitorii's release pipeline uploads three cross-compiled binaries per tag — linux x86_64, linux aarch64, windows x86_64). Without cleanup, this accumulates against the namespace's 5 GB free-tier storage cap.
  - `torii package list [--type TYPE] [--name SUBSTR] [--limit N]` — enumerate packages. `--type generic` is the gitorii case; the surface also supports the other types GitLab exposes (npm, maven, conan, pypi, composer, nuget, helm) without code changes — the parser is type-agnostic.
  - `torii package files <package-id>` — list files inside a package with their sizes (in MB). Useful for understanding what's stored before deleting.
  - `torii package delete <id> | --version vX.Y.Z | --older-than 90d --yes` — three modes: single-id, by-exact-version, by-age. The filter modes are mutually exclusive with the explicit id (clap-enforced). Batch mode previews up to 10 entries before confirmation, then iterates one-by-one with per-id success/failure reporting — same pattern as `torii pipeline delete`.
  - Implementation in `src/package.rs` (~280 lines, follows the `pipeline.rs` shape). GitLab-only on purpose: GitHub's binary-distribution model is Release Assets attached to Releases, which is `torii release`'s scope below.

- **`torii release {list, show, edit, delete}` — Release page management.** Both backends.
  - `torii release list [--limit N]` — recent releases. One-line-per-release.
  - `torii release show <tag>` — full details (description body, web URL, created date). Useful for previewing what's published before editing or for grabbing the URL to paste somewhere.
  - `torii release edit <tag> [--name X] [--notes notes.md | -]` — patch the release name and/or description without re-tagging or re-running CI. `--notes` accepts either a path to a markdown file or `-` for stdin (so you can pipe in dynamically-generated notes). Fixes the workflow of "oops I had a typo in the CHANGELOG section that got copied to the GitLab Release" without forcing a re-release.
  - `torii release delete <tag> [--yes]` — removes the Release entity (the underlying tag stays — use `torii tag delete <tag>` if you also want the tag gone). Useful when CI auto-created a release with garbage in the description because of an interrupted run.
  - Both backends supported with appropriate asymmetries: GitHub's edit API needs the numeric release id (fetched via the get-by-tag call); GitLab's API uses the tag directly in the path. CLI surface is identical from the user's POV.
  - Implementation in `src/release.rs` (~340 lines).

### Tests added

10+ inline unit tests across the three new modules — parsing logic for both backends, filter semantics (`older_than` keeps unparseable timestamps for safety), version matching, and `parse_github_job` status-normalization edge cases (`completed` + `failure` → `failed`, `completed` + `timed_out` → `failed`, `completed` + `cancelled` → `canceled`).

### Why this release

The v0.7.9 saga (~13 hours of CI debugging on 2026-05-19) exposed how thin our tooling was for the GitLab platform side — we had to reach for `curl` for every diagnostic (`/jobs/<id>/trace`, `/pipelines/<id>/jobs`, `/releases/<tag>`, etc.). 0.7.10 closes that gap so the next time something breaks, the diagnostic path is `torii job log <id> --tail 50` instead of three nested shell commands. The platform surface is now: `pipeline` (whole-pipeline ops, 0.7.7), `job` (individual job ops, this release), `package` (binary storage, this release), `release` (release-page metadata, this release).

## [0.7.9] - 2026-05-19

### Changed (CI only — no source-level changes)

- **CI moved back to GitLab.com shared runners.** A ~4h self-hosted runner experiment (registered as `void-torii` with tag `gitorii`, shell executor, Arch host) was abandoned after rustc 1.94.0 reproducibly SIGSEGV'd on this host even in trivial crates like `libc`, `idna`, `fiat-crypto`, `num-traits` — independent of `RUST_MIN_STACK` (tested up to 512 MB), `CARGO_BUILD_JOBS=1`, or `LimitSTACK=infinity` via systemd. The crash also reproduced in a standalone `cargo build` outside gitlab-runner, confirming it's a rustc-1.94/glibc/kernel interaction on that specific Arch host — unrelated to gitorii or the YAML.
- `.gitlab-ci.yml`: removed `default: tags: [gitorii]` so jobs go to `saas-linux-small-amd64` shared runners. The russh / rsa-rc / sec1 generic-tree stack pressure is covered by the `RUST_MIN_STACK="33554432"` already in the YAML's `variables:` block — shared runners satisfy the request without kernel-cap issues.
- Removed leftover project-level CI/CD variables (`RUST_MIN_STACK=536870912`, `CARGO_BUILD_JOBS=1`) introduced during the self-hosted debug iteration; they're no longer needed.

This release contains no source-level changes vs 0.7.8. It exists to put the updated `.gitlab-ci.yml` behind a tag so the release pipeline can actually fire — the workflow rules only trigger on tag pushes, so untagged main-branch commits don't validate the new CI shape. If you only consume `gitorii` via `cargo install`, 0.7.8 and 0.7.9 are equivalent.

## [0.7.8] - 2026-05-19

### Fixed

- **`torii sync --push` was re-pushing every local tag on every invocation, retriggering CI pipelines for every historical tag.** Severity: medium (no data loss; wasted runner time, polluted pipeline list). Observed in production on the gitorii repo itself: every release (`torii sync --push` after a new `torii tag create`) created stale "canceled" pipelines for `v0.7.0`, `v0.7.1`, `v0.7.2`, `v0.7.3` etc. They eventually got canceled by GitLab (concurrent tag pipelines hitting workflow:rules), but only after sitting queued and consuming runner attention — and they accumulated in the pipeline list like noise.

  Root cause: `GitRepo::push_all_tags_via_git2` (called unconditionally at the end of `push()`) built a refspec for **every** `refs/tags/*` in the local repo and pushed them all. libgit2 happily issued a push even for tag refs whose OID matched the remote already — and GitLab's `workflow:rules` evaluates the *receiving end's tag-push event*, not the underlying object delta, so an idempotent re-push still fires the pipeline. Mirror replication (`MirrorManager::sync_replicas_if_any`) had the same pattern for its SSH-based tag push.

  Fix: both call sites now do an `ls-remote` equivalent (`Remote::connect_auth` → `Remote::list`) before deciding what to push. Only tags whose local OID differs from the remote's (or aren't on the remote at all) get a refspec. Cost: one extra round-trip per push to enumerate remote tag OIDs. Benefit: N stale pipelines per release avoided on a repo with N historical release tags.

  Edge cases:
  - `force=true` (`torii sync --push --force`) still works — the comparison still happens, but tags that differ get the `+` prefix so rewritten OIDs (e.g. after `torii history reauthor --since ...`) still get pushed over the remote's ref.
  - Annotated tags' peeled `refs/tags/<name>^{}` entries that libgit2 sometimes surfaces in `list()` output are filtered out — only the tag object's own ref matters for the comparison.
  - First push of a brand-new tag still works (it's missing remotely → `None != Some(oid)` → included in the refspec list).

## [0.7.7] - 2026-05-18

### Fixed

- **Safety snapshots no longer leak into the next commit.** Severity: **high**. History-rewriting commands (`torii history reauthor`, `rebase`, `mailmap apply`) wrote their pre-op backups to `.torii/snapshots/<id>/git_backup/` *inside the working tree*. Nothing in `torii init` or in the snapshot-writing path added `.torii/` to `.gitignore`, so the directory looked like ordinary untracked project files. The very next `torii save -am ...` then staged everything new — including the full `.git` clone the snapshot contains. In the wild on `syrakon/tramuntana` this produced a single commit carrying 10,269 unrelated objects (~681 MB); the push died with `failed to finish zlib inflation: stream aborted prematurely`. Git's own `push --dry-run` reported "fast-forward, 1 commit", hiding the payload (dry-run enumerates refs, not packfile contents) — making the cause non-obvious during debugging. Full report in `docs/BUG_SNAPSHOT_LEAKS_INTO_COMMITS.md`.

  Two fixes landed, the second as defense-in-depth so future torii-internal files can't repeat the pattern:

  1. **Snapshots moved out of the working tree.** `SnapshotManager::new` now writes to `<gitdir>/torii/snapshots/<id>/` — i.e. under `.git/` itself, where `git add` cannot reach. The gitdir is resolved via `git2::Repository::discover().path()` so worktrees (`.git` is a file pointing into `.git/worktrees/<name>`) and submodules (similar gitlink indirection) end up with their own private snapshot directory rather than fighting over a shared one. This is the canonical convention git itself uses for tool-private state (hooks, refs, packed-refs, objects).

  2. **`torii save -a` now skips `.torii/`** the same way it implicitly skips `.git/`. Implemented via the `IndexMatchedPath` callback on `index.add_all()`: any path equal to `.torii` or starting with `.torii/` returns 1 (skip). When something is skipped, `add_all` prints a one-line hint telling the user — explicit paths still work (`torii save .torii/config.json -m "..."` will stage that one file). This protects the remaining legitimate working-tree files like `.torii/config.json` and `.torii/mirrors.json` from accidental capture too, since the `-a` flag is what users actually reach for day-to-day.

- **Auto-migration for pre-0.7.7 snapshots.** When `SnapshotManager::new` finds an old `.torii/snapshots/` directory in a working tree, it moves every snapshot under it into the new `<gitdir>/torii/snapshots/` location, then removes the empty `.torii/snapshots/` (and the `.torii/` parent if nothing else lives there). Idempotent — destinations that already exist are preserved (the new location wins). Cross-filesystem rename failures fall back to a recursive copy + remove. Prints a one-line `ℹ Migrating N snapshot(s) from … → …` notice. Runs on every `SnapshotManager` instantiation but is a no-op when the legacy directory is empty or absent.

### Added

- **`torii pipeline {list, cancel, retry, delete}` — CI pipeline / workflow-run management for GitLab Pipelines and GitHub Actions.** Symmetric surface across both platforms, auto-detects which one from the `origin` remote URL. Common shapes:

  ```sh
  torii pipeline list                              # recent on current repo
  torii pipeline list --status failed              # only failed
  torii pipeline list --limit 50                   # up to 50 (clamped to 100)
  torii pipeline cancel 12345                      # one
  torii pipeline retry 12345                       # one
  torii pipeline delete 12345                      # one (prompts unless --yes)
  torii pipeline delete --status failed --yes      # batch: every failed
  torii pipeline delete --status failed --older-than 7d --yes
  ```

  `--status` is a single normalized value (`success | failed | running | canceled | pending`) that each backend translates to its own filter parameter — GitLab uses `?status=` directly, GitHub Actions accepts `status=failure|in_progress|...` on the `workflow_runs` endpoint. The `Pipeline` struct keeps both the normalized status (for filtering) and the platform-native `raw_status` string (for display, so "timed_out" stays "timed_out" in the list output instead of collapsing into a generic "failed").

  `delete` has two mutually exclusive shapes: `delete <id>` (single pipeline, prompts unless `--yes`) and `delete --status ... [--older-than ...]` (filter-driven batch). Batch mode lists candidates, prints a preview of the first 10 + count of remaining, prompts unless `--yes`, then iterates one-by-one — per-id failures are reported but do **not** abort the rest, so a single 403 doesn't strand the remaining deletions. Exit code reflects "any failures" so CI/scripts can detect partial success.

  `--older-than` reuses the existing `crate::duration::parse_duration` (`7d`, `12h30m`, etc.). The filter is applied client-side over the listed page, so it composes cleanly with `--status` without needing platform-specific query params.

  Token resolution goes through the same `torii auth set <platform>` path used by `pr` and `issue` — no new env-var or config knob.

  Internals live in `src/pipeline.rs` (~370 lines, mirrors the `src/pr.rs` shape: `PipelineClient` trait + `GitHubPipelineClient` + `GitLabPipelineClient` + `get_pipeline_client` factory). Status-normalization tables in `parse_github_run` / `parse_gitlab_pipeline` cover the platform-specific values; unit tests in the same file verify the normalization (`completed`+`failure` → `failed`, `in_progress` → `running`, etc.) and the `filter_older_than` semantics — including the conservative "keep entries with unparseable timestamps" rule (we don't act on state we can't reason about).

### Audit — non-changes

- **Fix #4 from the snapshot bug report (warn on suspicious commit size — `> 50 MB` or `> 500 files`) was deferred.** The root cause is gone after fixes #1 + #3; a generic large-commit warning is general hardening worth doing on its own merit and not as a backstop for this bug. Tracked for a future release.

## [0.7.6] - 2026-05-18

### Added

- **`torii sync --fetch` accepts a remote name and `--all`.** The fork workflow — `origin` for our work, a separate `upstream` for read-only mirror sync — had no path through torii: `sync --fetch` always hit the tracking remote, and there was no way to point it elsewhere without dropping to `git fetch upstream` and bypassing the gitorii-skill invariant ("every VCS op goes through torii"). 0.7.6 closes this gap by overloading the existing positional argument on `sync`: when `--fetch` is present, the positional argument is the remote name, not a branch.

  ```sh
  torii sync --fetch                       # tracking remote (unchanged)
  torii sync --fetch upstream              # explicit remote
  torii sync --fetch --all                 # every configured remote
  ```

  Implementation:
  - `fetch_named(name)` validates the remote exists up-front and surfaces a hint listing configured remotes if it doesn't (instead of libgit2's generic error).
  - `fetch_all()` iterates `Repository::remotes()`, prints one line per remote with the per-remote status, returns Err if any single remote failed (the others are still attempted before the error surfaces).
  - Both share `fetch_one(name)`, which uses the default refspec from `.git/config` and the same auth callbacks + progress display the existing single-remote `fetch()` used.
  - `--all` is mutually exclusive with the positional remote (clap-enforced via `conflicts_with = "branch"`).

  Origin: tramuntana fork of servo/servo. Spec in `docs/FEATURE_FETCH_SPECIFIC_REMOTE.md`. Top-level `torii fetch` subcommand was considered and rejected — keeping the surface inside `sync` reuses the existing mental model.

### Audit — non-changes

- The companion request "add `torii remote add` / `remove`" was audited and dropped: `torii remote link` / `unlink` already cover the gap (URL form and platform shorthand both work). Renaming a public CLI surface for mental-model alignment with git costs more than it buys.

## [0.7.5] - 2026-05-18

### Fixed

- **PR / Issue views failed with the generic `Invalid configuration: Unexpected GitLab response`** for every non-200 reply. Both `pr::list` and `issue::list` (GitHub + GitLab variants) parsed the response body straight into `serde_json::Value::as_array()` without inspecting the HTTP status, so a `401 Unauthorized`, `404 Not Found`, or `403` (the actual common failure modes) was indistinguishable from a malformed body. Now each list call:
  1. Captures `resp.status()` before consuming the body.
  2. Parses the body as `serde_json::Value` (works for both the array success case and the `{ "message": "…" }` / `{ "error": "…" }` error case).
  3. On non-success, surfaces a real diagnostic: `GitLab API 401 Unauthorized: <message> (url: <full url>)` / `GitHub API 404 Not Found: <message> (url: <full url>)`. The URL is included so misconfigured `gitlab.host` / `github.owner` are obvious from the error alone.

- **`Auth` config keys lived in two places at once.** Since 0.7.2 the `Auth` view became the source of truth for tokens (`~/.config/torii/auth.toml`), but `load_config()` in `src/tui/app.rs` still listed `auth.cloud_key`, `auth.github_token`, `auth.gitlab_token`, `auth.codeberg_token`, `auth.bitbucket_token` in `ALL_KEYS` (and in `SENSITIVE`). Result: the `auth` section appeared in the Config view with `[set]` / `[not set]` placeholders that read from the old `config.toml [auth]` table — stale, and editing them did nothing because the resolver routes through `auth.toml` now. The five keys were removed from `ALL_KEYS` and `SENSITIVE`; the `Auth` view is now the only place credentials are managed. `worktree.base_dir` and `worktree.inherit_paths` were added in their place (real keys the Config view should expose).

### Changed — TUI

- **Config view "status" box removed; its hints live in the global hint bar.** The view used to render a three-row layout (sections | entries | status), where the bottom box duplicated the bottom hint legend used by every other view. The status box is gone; `render_hint` for `View::Config` is now mode-aware:
  - Navigating: `[↑↓/jk] navigate  [Enter] edit  [Tab] toggle scope`
  - Editing: `[Enter] save  [Esc] cancel`

  The transient save-confirmation message ("saved ✓") moved to the App-wide status line that already exists for every other view's transient feedback. Removing `render_status` outright (rather than `#[allow(dead_code)]`) was deliberate — copy-pasted hint strings drift fast and the canonical legend is in `ui.rs::render_hint`.

## [0.7.4] - 2026-05-17

### Fixed

- **TUI sidebar couldn't reach the four new views** added in 0.7.2 (Worktree, Submodule, Bisect, Auth). `App::sidebar_down` hard-coded `if self.sidebar_idx < 13` — the pre-0.7.2 max — so navigating with `↓` / `j` capped at index 13 instead of 15 and the bottom four entries (Bisect, Auth, Config, and the slot they pushed Pr/Issue past) were unreachable through the sidebar. The mapping in `view_for_idx` was also stale: indexes 7–15 still pointed at the pre-0.7.2 layout (`History`, `Remote`, `Workspace`, …) so even when navigation worked the rendered view was wrong.

  Fix:
  - `view_for_idx` updated to the post-0.7.2 16-entry order, kept in sync with `TABS` in `src/tui/ui.rs` and the `sidebar_idx` assignments in `App::go_to`. A new constant `App::SIDEBAR_LEN = 16` is used as the bound for `sidebar_down`.
  - `App::go_back` mapping rewritten with the same order; the deprecated `View::History` / `Mirror` / `Settings` variants map to their fused destinations (4, 9, 15 respectively).

- **`torii publish` printed a literal `$(name)`** after a successful upload (`View at https://crates.io/crates/$(name)`) — the placeholder was never substituted. Now reads `[package].name` from `Cargo.toml` and prints the correct URL.

### Changed (CI)

- **`.github/workflows/release.yml` publish-crates job rebuilt around an OIDC-primary + PAT-secret fallback chain.** The previous version layered `rust-lang/crates-io-auth-action@v1` (which injects `CARGO_REGISTRY_TOKEN` via Trusted Publishing OIDC) and then immediately overwrote that token with `${{ env.CRATES_IO_TOKEN }}` — a never-defined env var that resolved to empty string. Net effect: `cargo publish` always ran with no token and failed. Now:
  1. The OIDC action runs with `continue-on-error: true`. If trusted publishing is set up on crates.io for `PaskiDev/gitorii` + workflow `release.yml`, it injects the token and `cargo publish` uses it.
  2. If OIDC didn't fire, the publish step's env reads `secrets.CARGO_REGISTRY_TOKEN` (repository or org secret) as a fallback.
- **Note for users updating their GitHub allowlist**: the workflow uses three external actions (`actions/checkout`, `softprops/action-gh-release`, `dtolnay/rust-toolchain`, `rust-lang/crates-io-auth-action`). The org-level "Actions permissions" must either allow Marketplace-verified creators or list these explicitly under "Allow specified actions".

## [0.7.3] - 2026-05-17

### Fixed

- **`torii save` no longer falls back to `"Torii User" <user@torii.local>`** when the user's identity is configured via `torii config` but not in git's own config chain. The previous `GitRepo::get_signature` read **only** from `repo.config()` (libgit2's `.git/config` → `~/.gitconfig` → `/etc/gitconfig`) and substituted a hardcoded placeholder on miss. Tracked in `docs/BUG_COMMIT_AUTHOR_FALLBACK.md` (now annotated as FIXED). The fix introduces a single `crate::core::resolve_signature(&repo)` with documented precedence:
  1. Torii config (`~/.config/torii/config.toml [user]` — `user.name` / `user.email`). Treated as the source of truth for the user's identity.
  2. Git's own config chain (kept as fallback so existing `git config user.name` setups keep working).
  3. **Hard error** with a fix-it hint pointing at `torii config set user.name "…"`. Bogus authorship is worse than failing fast.
- **Every commit-writing call site routes through the new resolver**: `GitRepo::get_signature` (used by `save` / `save --revert` paths), `core_tag::cherry_pick` and its continue/conflict paths, `tag::create_tag` (annotated tags), `core_extensions` (rebase apply, revert commit, merge commit, generic-commit helper x2), `snapshot::stash` (which previously had its own `"torii"/"torii@local"` placeholder fallback — same anti-pattern, now removed), and the TUI's direct-commit path. Seven sites in total, all unified.
- **Empty strings treated as "not configured".** `torii config set user.name ""` previously slipped past the lookup and passed an empty name straight to libgit2, which rejected the commit with the generic `Signature cannot have an empty name or email` error. Now the resolver filters empties and returns the same fix-it error as a missing key.

### Migration

Users with the bug today (commits attributed to `Torii User`) can clean up after upgrading via:

- Single bad commit: `torii config set user.name "…"` + `torii config set user.email "…"`, then `torii save --amend -m "<same message>"`.
- Multiple bad commits: configure as above, then `torii history reauthor --old "Torii User" --new "Real Name <real@email>"` (committer pass-through with `--committer` if needed).

## [0.7.2] - 2026-05-17

**Headline:** TUI catches up with the CLI surface. The four big feature blocks from 0.6.6 → 0.7.1 (worktrees, submodules, bisect, auth) now have first-class views in `torii tui`, and the sidebar is reshuffled so related concepts live together.

### Added

- **`Worktree` view** — lists every linked working copy with branch, clean/dirty count, and lock status. Sidebar key `k`. Refreshes on entry via libgit2's `repo.worktrees()` directly (no CLI shell-out).
- **`Submodule` view** — every registered submodule with HEAD oid, working-tree oid, URL, and state string (clean/modified/staged/untracked/not initialised). Sidebar key `m`.
- **`Bisect` view** — surfaces the state of an active `git bisect` session (current commit, good/bad refs from `.git/BISECT_LOG`). When no session is active, points to the CLI commands to start one. Sidebar key `v`.
- **`Auth` view** — masked list of every credential torii knows about (cloud key + per-platform tokens) with the source of each (`env: $VAR` / `local` / `global` / `(not set)`). Mirrors `torii auth doctor` from the CLI. Sidebar key `a`.

### Changed — sidebar reorganisation

Three view-pairs that were technically separate but functionally one have been **fused**, with the deprecated variants kept in the `View` enum for back-compat redirects:

- **`Log` absorbs `History`.** The natural flow is "browse log → modify the commit I see", and separating them forced sidebar hops. History rewriting ops will surface inside the log view in 0.7.3 (no functional regression — `torii history …` from the shell remains the canonical write path). Sidebar entry "history" removed.
- **`Remote` absorbs `Mirror`.** The previous dispatcher already redirected `View::Mirror → views::remote::render`, so the separation was artificial. Mirrors become a panel/tab inside Remote in 0.7.3. Sidebar entry "mirror" removed.
- **`Config` absorbs `Settings`.** Both end up presenting key-value editors; they'll share a single view with two tabs ("TUI prefs" / "Repo config") in 0.7.3. Sidebar entry "settings" removed.

Net sidebar count: 14 (pre-0.7.2) → 16 entries, with four new feature surfaces and three removed-but-redirected.

### Notes

- The four new views are **informative in 0.7.2** — they show state and point at CLI commands for actions. Interactive ops dropdowns (add/remove/lock/start-bisect/set-token) will land in 0.7.3 once we've validated the layout. Same pattern other views (tag/snapshot) follow.
- Deprecated `View::Mirror`, `View::History`, `View::Settings` variants still match in the dispatcher and `go_to` so any old code that constructs them keeps working — they just redirect to the fused view. Will be removed when the 0.8 deprecation cycle lands.
- Old per-view event handlers (`handle_history`, `handle_mirror`, `handle_settings`) generate dead-code warnings for now; cleaning them up is bundled with the 0.7.3 interactive sweep so the diff stays focused.

### Out of scope, deferred to 0.7.3

- Interactive keybinds on the four new views (n=new, d=delete, l=lock, …).
- `Log` getting `--tracked` toggle + notes overlay + patch-export modal.
- `Snapshot` view ops dropdown (`apply` / `pop` / `drop` / `clear` / `show`).
- `Tag` view force-push toggle.
- `Remote` view: mirrors panel + subtree subpanel.
- `Config` view: TUI/Repo tabs absorbing the old Settings view.

## [0.7.1] - 2026-05-17

**Headline:** credential management gets a coherent home. `torii auth` now manages every secret torii uses (cloud key + platform tokens). The old split (`torii auth login` for cloud, `torii config set auth.X_token` for platforms) was confusing and had three real bugs.

### Fixed

- **`torii config set auth.X_token … --local` had no effect at runtime.** The local store was never read by transport/PR/issue/remote code — they all called `ToriiConfig::load_global()`, which ignores `<repo>/.torii/config.toml`. Tokens set with `--local` got persisted but never resolved when pushing or hitting platform APIs. Fixed by centralising every token read through `crate::auth::resolve_token`, which checks env > local > global in that order.
- **`torii config set … --local` was duplicating the global config into the local file.** `ToriiConfig::load_local()` merges global+local and returns the result; the dispatcher then called `save_local()` on the *merged* config, writing every global setting (including tokens) into the local file. From that moment on the local clone shadowed any subsequent global change. Auth state now lives in a separate `auth.toml` and the new `load_local_raw` returns local-only without merging.
- **Mixed precedence between commands.** `pr` and `issue` honoured env vars (`GITHUB_TOKEN`, `GITLAB_TOKEN`), but the HTTPS transport that actually does the pushing did not. So `cargo install`-style CI setups would create PRs fine but fail to push. Single resolver, same precedence everywhere.

### Added

- **`torii auth` becomes the single entry point for credentials.**
  - `torii auth set <provider> <token>` (provider: `github`, `gitlab`, `gitea`, `forgejo`, `codeberg`, `bitbucket`, `sourcehut`, `cargo`). Use `-` as the token to read from stdin (CI-safe).
  - `torii auth get <provider>` prints the resolved token, masked (`ghp_xxxx****`). `--unsafe-show` for the raw value.
  - `torii auth list` shows every provider with masked value and source (env / local / global / not set).
  - `torii auth remove <provider>` deletes from global; `--local` for the per-repo store.
  - `torii auth doctor` prints exactly where each provider's token is being resolved from — the missing tool when "torii doesn't use my token" hits. Also surfaces a stale legacy `[auth]` block if it lingers in `config.toml`.
- **`torii publish`** — thin wrapper over `cargo publish` that injects `auth.cargo` automatically. No more `.env` juggling. Flags `--dry-run / --no-verify / --allow-dirty / --token <X>` pass through.
- **New env vars recognised**: `BITBUCKET_TOKEN`, `SOURCEHUT_TOKEN` / `SRHT_TOKEN`, `GL_TOKEN`. Already-supported ones (`GITHUB_TOKEN`, `GH_TOKEN`, `GITLAB_TOKEN`, `GITEA_TOKEN`, `FORGEJO_TOKEN`, `CODEBERG_TOKEN`, `CARGO_REGISTRY_TOKEN`, `TORII_HTTPS_TOKEN`) now apply uniformly across every code path.

### Changed

- **Storage moved**: platform tokens migrate from `~/.config/torii/config.toml [auth]` to `~/.config/torii/auth.toml [tokens]`. Auto-migration: the first time `auth` is consulted, it reads the legacy `[auth]` block, rewrites it into the new file, and `torii auth doctor` reminds you the legacy section can be deleted from `config.toml`. No data loss; old configs keep working until you clean them up.
- **`torii config set auth.<provider>_token …`** still works but is now a deprecated alias that redirects to `torii auth set <provider> …` and prints a one-line hint. Scheduled removal: 0.8.
- **All error messages and `-h` examples** updated to point at `torii auth set` instead of `torii config set auth.X_token`.

### Storage format

`~/.config/torii/auth.toml` (chmod 600) now uses two sections:

```toml
[cloud]
key = "gitorii_sk_…"
endpoint = "https://api.gitorii.com"

[tokens]
github = "ghp_…"
gitlab = "glpat-…"
cargo = "cio_…"
```

Both the old top-level `key = …` format and the legacy `config.toml [auth]` are still parsed for backwards-compat.

### Out of scope

- `cargo-dist` installer setup — still deferred to 0.7.2.
- GPG re-sign in `torii history reauthor` — same.

## [0.7.0] - 2026-05-17

**Headline:** ~93 % porcelain coverage. 0.6.9 covered the structural gaps (worktrees, submodules, subtrees); 0.7.0 finishes the surface area with the eight commands a vanilla `git` user expects to find. Also pulls the existing names into plainer English where it didn't break git habits.

### Added (top-level)

- **`torii bisect`** — binary-search the commit that introduced a regression. Subcommands `start / bad / good / skip / reset / log / run <cmd>`. State-machine wrapper over `git bisect` (libgit2 has no bisect primitives).
- **`torii describe`** — pretty name for HEAD based on the nearest tag, e.g. `v0.6.9-3-gabc1234`. Flags `--tags / --long / --dirty / --candidates N`.
- **`torii archive`** — export a tree or commit as tarball/zip. Wrapper over `git archive` to inherit decades of format edge-cases.
- **`torii remove`** (alias `rm`) — remove tracked files from index and working tree. Flags `--cached / -r / --force`.
- **`torii rename`** (alias `mv`) — rename/move tracked files; both filesystem and index updated atomically. `--force` to overwrite.
- **`torii grep`** — search tracked content for a pattern. Wrapper over `git grep` (faster than ripgrep on tracked-only content; different concern from `torii scan`).
- **`torii notes`** — annotations on commits stored in `refs/notes/commits`. Subcommands `list / add / append / show / edit / copy / remove`.
- **`torii patch`** — export commit ranges as `.patch` files (`export`) and apply them as new commits (`apply`). Wrappers over `git format-patch` / `git am` with `--3way / --continue / --abort / --skip` plumbed through.
- **`torii clean`** — remove untracked files (≡ `git clean`). Defaults to dry-run for safety. `-f / -d / -x / -X` flags.

### Added (extensions to existing commands)

- **`torii tag push --force`** — finally a way to force-push a single tag (or all tags) from torii without falling back to `git`. Refspec gets the standard `+oldref:newref` prefix on the wire.
- **`torii submodule update --recursive`** and **`torii submodule add --recursive`** — descend into nested submodules so `update` mirrors `git submodule update --init --recursive` when both flags are passed.
- **`torii worktree lock`** / **`unlock`** / **`move`** / **`repair`** — fills in the rest of `git worktree` parity. `move` patches both the `.git` link inside the worktree and the `.git/worktrees/<name>/gitdir` admin file by hand because libgit2 has no `worktree_move`.
- **`torii snapshot apply / pop / drop / clear / show`** — completes the `git stash` family on top of the existing `stash` / `unstash`. `apply` and `pop` are aliases of `unstash --keep` / `unstash` for users coming from git. `clear` deletes all snapshots (asks unless `--yes`); `show` prints metadata + bundle contents.
- **`torii status --tracked` (`-z` for NUL-separated)** — `git ls-files` equivalent. Walks the index and prints every tracked file.
- **`torii remote refs <target>` (`--heads / --tags`)** — `git ls-remote` equivalent. Hits the network using configured auth.

### Renamed (with aliases — old names still work, prints deprecation in some cases)

- **`torii history clean` → `torii history compact`** (alias `gc`). "GC" is jargon, "compact" reads. `clean` (history) → deprecated alias with warning; will be removed in 0.8. Frees up the word `clean` for the new top-level untracked-cleanup command.
- **`torii history fsck` → `torii history orphans`** (alias `fsck`). "fsck" is hostile Unix-filesystem jargon; "orphans" describes exactly what the command finds.
- **`torii rm` → `torii remove`** (alias `rm`). Plain English first, `rm` kept for muscle memory.
- **`torii mv` → `torii rename`** (alias `mv`). "rename" is more accurate than "move" 95 % of the time and friendlier; `mv` kept for muscle memory.

### Deprecated

- **`torii blame <file>`** → use `torii show <file> --blame` (already existed; was a duplicate top-level). Old form prints a warning and still works through 0.7.x; will be removed in 0.8.
- **`torii history clean`** → use `torii history compact` (or alias `gc`). Old form prints a warning.

### Notes

- **`torii notes` and `torii bisect`** intentionally wrap their git counterparts rather than reimplementing on top of libgit2. The state machines and edge-case handling involved (mailbox parsing, BISECT_* file ceremony, notes-tree merge semantics) are decades-refined upstream; reimplementing them would be 1k+ LOC of risk for behaviour already correct.
- **Out of 0.7.0, deferred to 0.7.1:**
  - `cargo-dist` installer setup — the README still mentions a `gitorii-installer.sh` that no CI generates. Tracked.
  - GPG re-sign during `torii history reauthor` / `mailmap apply` — needs libgit2 `commit_signed` callback wiring + a real key in tests. Documented limitation since 0.6.7.
- **`rust-toolchain.toml` stays pinned at 1.94.0.** rustc 1.96 (with the mono-partitioning ICE fix) is currently in beta with a stable release expected in ~11 days; we'll validate against it then and unpin.

### Porcelain coverage

After 0.7.0, gitorii covers **~93 %** of git porcelain commands (excluding GUIs and ploumbing). What remains intentionally out:

- `sparse-checkout` (edge case for monorepos),
- `mergetool` / `gui` / `citool` (interactive UIs — `torii tui` occupies that space),
- `range-diff` (rare; comparing commit series),
- `restore` at file-level (parcial via `save --reset`; explicit form pending if asked),
- `shortlog` (parcial via `log --author`).

Nothing structural is missing.

## [0.6.9] - 2026-05-17

### Added
- **`torii submodule` — seven-subcommand MVP** for embedding another git repo at a pinned commit inside this one. Mirrors `git submodule` with torii's UX layer on top.
  - **`torii submodule add <url> <path> [--branch <b>] [--name <n>]`** registers the entry in `.gitmodules`+`.git/config`, clones the contents, stages the result, and writes the optional tracking branch. The user finishes the operation with their own commit.
  - **`torii submodule status` (or just `torii submodule`)** lists every submodule with HEAD oid, working-tree oid, URL, and a state string (`clean`, `modified`, `not initialised`, `dirty working tree`, etc.).
  - **`torii submodule init [--force]`** copies `.gitmodules` URLs into `.git/config` so `update` knows where to fetch from. Idempotent.
  - **`torii submodule update [--init]`** fetches and checks out the commit each submodule is pinned at. `--init` runs `init` first for uninitialised entries (mirrors `git submodule update --init`).
  - **`torii submodule sync`** re-copies `.gitmodules` URLs into `.git/config` (useful after an upstream URL change).
  - **`torii submodule foreach <cmd>`** runs `<cmd>` via `$SHELL -c` in each submodule's working directory, exporting `TORII_SUBMODULE_NAME` and `TORII_SUBMODULE_PATH`. Stops at the first non-zero exit (matches `git submodule foreach` default).
  - **`torii submodule remove <path>`** scrubs all four places submodule state lives: `.gitmodules` section, `.git/config` section, `.git/modules/<name>/` cached gitdir, and the super-repo's index (via libgit2 directly — `git rm --cached` refuses when `.gitmodules` already has staged changes; libgit2's index API doesn't care).
- **`torii subtree` — five-subcommand thin wrapper** around `git subtree` for merging another project's history into a subdirectory of this repo. `add`/`pull`/`push`/`split`/`merge`, all forwarding to the upstream contrib script. `--squash` exposed on the operations that support it.
  - Why a wrapper, not a reimplementation: `git subtree` is ~800 lines of bash refined since 2009 with a long tail of edge cases (orphan commits, parent detection, --squash semantics, history rewrites through merge bases). Reimplementing those in Rust on top of libgit2 (no subtree primitives) would be 1k+ LOC of risk. Torii provides the UX skin and clear error message when `git-subtree` is missing.
- **Worktree polish — four follow-ups to 0.6.8:**
  - **`torii worktree` with no subcommand defaults to `list`** (git/cargo/npm convention).
  - **`torii worktree list` now shows ahead/behind vs upstream** when the worktree's branch tracks one. Reads `dirty · 2 ahead, 1 behind` style; silently omits the second segment when there's no upstream (very common for fresh feature branches).
  - **New config key `worktree.inherit_paths`** (comma-separated): paths from the main repo to drop into every freshly-created worktree. Files are copied (real fresh writable copy); directories are symlinked (typically large build caches like `target/` or `node_modules/`); missing entries are silent. Solves the #1 pain of worktrees in practice — no more rebuilding from scratch in every linked checkout.
  - **Snapshot module now handles worktrees correctly.** Previously the pre-remove safety snapshot in `torii worktree remove` failed silently with "Not a directory (os error 20)" because the module assumed `.git` was a directory; in a worktree it's a one-line link file pointing at a shared gitdir in the main repo's `.git/modules/<name>/`. The module now detects the file case, copies the link plus a `RESOLVED-GITDIR` marker, and leaves the shared metadata alone.

### Notes
- **Submodule recursion (`--recursive`)** is intentionally not in 0.6.9; nested submodules need a manual loop for now. Tracked for follow-up.
- **Subtree** depends on `git-subtree` being on PATH. On Arch/Fedora it ships with `git`; on Debian/Ubuntu it's a separate `git-subtree` package. Torii surfaces a precise error message if it's missing.
- **Index manipulation in `submodule remove`** is now done via libgit2 directly (`Index::remove_path`/`remove_dir` + `Index::write`), not by shelling out to `git rm --cached`. The shell-out path stayed brittle in practice because git refuses to operate on the index when `.gitmodules` has uncommitted edits, which is precisely the state we're in mid-remove.

## [0.6.8] - 2026-05-16

### Added
- **`torii worktree` — five-subcommand MVP**: linked working copies of the same repository, each on its own branch, sharing the underlying object database. Useful for "let me hot-fix without disturbing my in-progress branch" and similar workflows that `git worktree` covers — with torii ergonomics on top.
  - **`torii worktree add [<path>] [-b <new-branch>] [<existing-branch>]`** creates a new worktree. Path is optional: when omitted it's derived from `worktree.base_dir` (new config key, default `..`) + `<repo>-<branch-sanitized>/`. `-b` creates a branch off HEAD; positional names an existing local branch.
  - **`torii worktree list`** prints every worktree (main + linked) with branch name and clean/dirty status in one shot. `📍` marks the current one; locked worktrees show their lock reason. Faster mental model than the per-worktree text dump from `git worktree list`.
  - **`torii worktree remove <path> [--force] [--no-snapshot]`** deletes a worktree's directory and prunes its libgit2 metadata. Refuses if the working tree is dirty unless `--force`. Always attempts a safety snapshot first (snapshot of the worktree itself, not the main repo). The snapshot may silently fail on worktrees because the existing snapshot module assumes `.git` is a directory and a worktree's `.git` is a link file — graceful warning, removal proceeds. Snapshot module fix tracked for a later release.
  - **`torii worktree prune`** clears metadata for worktrees whose directories were deleted out-of-band (e.g. via `rm -rf`). Only fires on already-invalid entries; never touches live worktrees.
  - **`torii worktree open <path>`** launches `$SHELL` (fallback `/bin/bash`) in the worktree directory and blocks until you exit — same gesture as `(cd <path> && $SHELL)` but rejected if the path isn't a known worktree of the current repo. `git worktree` has no equivalent.
- **New config key `worktree.base_dir`** (default `..`) controls where `torii worktree add` puts new worktrees when no path is provided. Honors `~` expansion. Set with `torii config set worktree.base_dir ~/worktrees` to centralise them.

### Notes
- Lock / unlock / move / repair are intentionally not in 0.6.8; the design review picked an MVP plus `open` as the first cut. Filling them in is a straight-line addition on top of `Worktree::lock`/`unlock` from git2 + path manipulation; pull request welcome.
- Unit tests cover branch-name sanitisation, `~` expansion, worktree-name derivation. Walker tested end-to-end against toy repos: add (new + existing branch), list (with status), remove (clean + dirty + force), prune (stale entries).

## [0.6.7] - 2026-05-16

### Added
- **`torii history reauthor --old <id> --new <id>`** — rewrite author identity across reachable history with a single CLI pair. Auto-detects the `--old` format: `"Name <email>"` for full match, a bare email for email-only match, or a bare name for name-only match. The replacement `--new` must always be `"Name <email>"`. Flags: `--committer` (also rewrite committer; default off), `--since <rev>` (limit to a range), `--dry-run` (preview without writing), `--no-snapshot` (skip the automatic safety snapshot), `--allow-dirty` (proceed past uncommitted changes).
- **`torii history mailmap apply [--file <path>]`** — batch identity rewrite driven by a [standard git `.mailmap`](https://git-scm.com/docs/gitmailmap) at the repo root (or any path). Supports all four mailmap line shapes: `Name <commit-email>`, `<proper-email> <commit-email>`, `Name <proper-email> <commit-email>`, `Name <proper-email> Commit Name <commit-email>`. Shares every flag with `reauthor` (`--since`, `--dry-run`, `--no-snapshot`, `--committer`, `--allow-dirty`).
- **Shared behaviour for both commands**:
  - Safety snapshot (`pre-reauthor-<timestamp>`) taken automatically; revert with `torii snapshot restore <id>`.
  - Annotated-tag taggers are rewritten to match the new identity (not preserved) so tag metadata stays consistent with the rewritten commit.
  - Original author/committer timestamps preserved — only *who* changes, never *when*. Use `torii history rewrite` for dates.
  - Refuses to run if the repository has a pending merge/rebase/cherry-pick or a dirty working tree (override with `--allow-dirty`).
  - HEAD and local branches re-point at the new OIDs; lightweight tags retarget; annotated tags get rebuilt.

### Documentation
- **`COMMANDS.md` adds an "Identity rewrite details" subsection** under `torii history` covering snapshot behaviour, timestamp preservation, GPG-signature invalidation, mailmap format, and the `--force` push needed after rewriting shared branches.
- **`README.md` History section** gains the new commands and a one-paragraph caveat block.

### Build / toolchain
- **`.github/workflows/release.yml` pins `dtolnay/rust-toolchain@1.94.0`** for the `cargo publish` job (was `@stable`). Without this the CI's verify-build step would ICE on rustc 1.95.0 against the russh→rsa-rc chain and the publish would never reach crates.io. Also passes `--locked`, `RUST_MIN_STACK=16777216` and `CARGO_BUILD_JOBS=2` to mirror the README workarounds for the codegen-pressure path. Revert to `@stable` once upstream rustc fixes the regression.

### Documentation
- **README "Install" expanded** with a fallback to the GitLab Generic Package Registry direct URL (`gitlab.com/api/v4/projects/paskidev%2Fgitorii/packages/generic/gitorii/<tag>/torii-<arch>`). The `gitorii-installer.sh` wrapper referenced in the top install snippet doesn't exist yet — no CI generates it — so users currently hitting the 404 have an explicit working path. `cargo install gitorii --locked` is the new from-source recommendation.
- **README "Known issue" rewritten** to separate the two failure modes (rustc 1.95 ICE vs. LLVM codegen SIGSEGV / stack overflow) and give the concrete flags that resolve each one: `cargo +1.94.0 install gitorii --locked` for the ICE, plus `RUST_MIN_STACK=16777216 ... -j 2` for the codegen path. Adds a third "skip the compiler entirely" path with the GitLab binary URL.

### Known limitations
- **GPG-signed commits**: signatures invalidate after rewrite because they're computed over the original author. Re-sign manually (or set up a key and re-run `torii save --amend` on each commit) — automatic re-signing during rewrite is not yet wired.
- **`gitorii-installer.sh` doesn't exist yet** — README mentions GitHub Releases but no CI generates the wrapper script. Tracked for follow-up (cargo-dist or equivalent). Direct binary download from GitLab works in the meantime; see Install section.

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
