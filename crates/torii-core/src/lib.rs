//! torii-lib — domain library for the torii (Gitorii) ecosystem.
//!
//! Everything the CLI, the TUI and the IDE share lives here: VCS
//! operations over the object database, platform clients
//! (GitHub / GitLab / Gitea / Sourcehut / Radicle / Bitbucket / Azure),
//! multi-repo workspaces, mirrors, config, auth and versioning helpers.
//!
//! ## API tiers
//!
//! - **Domain API** — the documented surface. Returns plain data
//!   (`RepoStatus`, `PullRequest`, `Pipeline`, …) and never exposes
//!   `git2` types. This is the contract the IDE builds on, and the
//!   boundary where the future `VcsEngine` trait (torii-vcs) plugs in.
//! - **Porcelain helpers** — a handful of `git2`-typed functions
//!   (`commit_inner`, `resolve_signature`, `signature_letter`) still
//!   consumed by the TUI in the twin `gitorii` crate. Internal contract,
//!   slated for migration behind the domain API.

// ── Top-level modules (organised by concern) ─────────────────────────────────
//
// Each subdir owns one slice of the codebase:
//   - platforms/ : github/gitlab/gitea/sourcehut/radicle clients
//   - vcs/       : libgit2 wrappers around the object database
//   - workspace/ : remote, mirror, multi-repo orchestration
//   - util/      : config, auth, URL parsing, HTTP/SSH/gpg/radicle helpers
//   - cloud/     : gitorii.com cloud (api key, sync)
//   - transport/ : libgit2 transport plumbing
//   - versioning/: SemVer + conventional commits helpers
pub mod cloud;
pub mod error;
pub mod platforms;
pub mod transport;
pub mod util;
pub mod vcs;
pub mod versioning;
pub mod workspace;

// ── Backwards-compatible re-exports ─────────────────────────────────────────
//
// Until 0.7.16 every module sat directly under `src/`. Hundreds of
// `use crate::pr::` / `use crate::core::` / etc. call-sites depend on
// those paths — both inside this library and in the `gitorii` binary
// crate, which mirrors this list with `pub use torii_core::…`. New code
// should prefer the canonical `crate::platforms::pr` etc. paths.

// platforms/
pub use platforms::issue;
pub use platforms::package;
pub use platforms::pipeline;
pub use platforms::pr;
pub use platforms::registry as platforms_registry;
pub use platforms::release;
pub use platforms::runner;

// vcs/
pub use vcs::commit_scan;
pub use vcs::core;
pub use vcs::core_extensions;
pub use vcs::core_tag;
pub use vcs::history_reauthor;
pub use vcs::history_reword;
pub use vcs::patch;
pub use vcs::scanner;
pub use vcs::sign;
pub use vcs::snapshot;
pub use vcs::tag;

// workspace/
pub use workspace::mirror;
pub use workspace::remote;
// `workspace::workspace` keeps its nested name to avoid colliding with
// the parent module; the call-sites that need `WorkspaceManager`
// already reference the long path.

// util/
pub use util::auth;
pub use util::config;
pub use util::duration;
pub use util::gpg;
pub use util::graph;
pub use util::hooks;
pub use util::http;
pub use util::oauth;
pub use util::radicle;
pub use util::ssh;
pub use util::toriignore;
pub use util::updater;
pub use util::url;
