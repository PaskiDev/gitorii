// Copyright (c) 2026 Torii Project. All Rights Reserved.
// Licensed under the Torii Source-Available License (Non-Commercial Fork-Friendly) v1.0.
// See LICENSE file in the project root for full license information.
// Commercial use is prohibited without explicit written permission from the copyright holder.

// ── Core ─────────────────────────────────────────────────────────────────────
mod cli;
mod error;

// ── Top-level modules (organised by concern) ─────────────────────────────────
//
// Each subdir owns one slice of the codebase:
//   - platforms/ : github/gitlab/gitea/sourcehut/radicle clients
//   - vcs/       : libgit2 wrappers around the object database
//   - cmd/       : thin wrappers around `git` subcommands
//   - workspace/ : remote, mirror, multi-repo orchestration
//   - util/      : config, auth, URL parsing, HTTP/SSH/gpg/radicle helpers
//   - tui/       : interactive terminal UI
//   - cloud/     : gitorii.com cloud (api key, sync)
//   - transport/ : libgit2 transport plumbing
//   - versioning/: SemVer + conventional commits helpers
mod platforms;
mod vcs;
mod cmd;
mod workspace;
mod util;
mod tui;
mod cloud;
mod transport;
mod versioning;

// ── Backwards-compatible re-exports ─────────────────────────────────────────
//
// Until 0.7.16 every module sat directly under `src/`. Hundreds of
// `use crate::pr::` / `use crate::core::` / etc. call-sites depend on
// those paths. Rather than sweep every file, we re-expose each moved
// module at the crate root with `pub use`. New code should prefer the
// canonical `crate::platforms::pr` etc. paths.

// platforms/
pub use platforms::pr;
pub use platforms::issue;
pub use platforms::pipeline;
pub use platforms::package;
pub use platforms::release;
pub use platforms::runner;
pub use platforms::registry as platforms_registry;

// vcs/
pub use vcs::core;
pub use vcs::core_extensions;
pub use vcs::core_tag;
pub use vcs::tag;
pub use vcs::snapshot;
pub use vcs::patch;
pub use vcs::history_reauthor;
pub use vcs::commit_scan;
pub use vcs::scanner;

// cmd/
pub use cmd::archive;
pub use cmd::bisect;
pub use cmd::clean;
pub use cmd::describe;
pub use cmd::fileops;
pub use cmd::grep;
pub use cmd::notes;
pub use cmd::submodule;
pub use cmd::subtree;
pub use cmd::worktree;

// workspace/
pub use workspace::mirror;
pub use workspace::remote;
// `workspace::workspace` keeps its nested name to avoid colliding with
// the parent module; the single call-site that needs `WorkspaceManager`
// already references the long path.

// util/
pub use util::auth;
pub use util::config;
pub use util::duration;
pub use util::url;
pub use util::toriignore;
pub use util::hooks;
pub use util::updater;
pub use util::http;
pub use util::oauth;
pub use util::ssh;
pub use util::graph;
pub use util::gpg;
pub use util::radicle;

use anyhow::Result;
use cli::Cli;
use clap::Parser;

fn main() -> Result<()> {
    transport::register_all();
    let cli = Cli::parse();
    let result = cli.execute();
    updater::maybe_notify();
    result
}
