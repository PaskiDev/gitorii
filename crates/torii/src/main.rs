// ── Binary-side modules ──────────────────────────────────────────────────────
//
//   - cli/ : clap parser + per-command handlers (presentation layer)
//   - cmd/ : thin wrappers around `git` subcommands
//   - tui/ : interactive terminal UI
mod cli;
mod cmd;
mod tui;

// ── Library re-exports ───────────────────────────────────────────────────────
//
// All domain logic lives in the `torii-core` crate. The binary-side code
// (cli/, cmd/, tui/) predates the split and references `crate::core::`,
// `crate::error::`, etc. — re-exposing the library modules at this crate's
// root keeps every one of those call-sites compiling. New code should
// prefer `torii_core::…` paths directly.
pub use torii_core::{cloud, error, platforms, transport, util, vcs, versioning, workspace};

// platforms/
pub use torii_core::{issue, package, pipeline, platforms_registry, pr, release, runner};

// vcs/
pub use torii_core::{
    commit_scan, core, core_extensions, core_tag, history_reauthor, patch, scanner, sign, snapshot,
    tag,
};

// workspace/
pub use torii_core::{mirror, remote};

// util/
pub use torii_core::{
    auth, config, duration, gpg, graph, hooks, http, oauth, radicle, ssh, toriignore, updater, url,
};

// cmd/ (binary-local)
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

use anyhow::Result;
use clap::Parser;
use cli::Cli;

/// Restore the default SIGPIPE disposition (terminate). The Rust runtime
/// sets SIGPIPE to ignore before `main()`, which turns writes to a closed
/// pipe into `Err(EPIPE)` — and `println!` panics on that. With SIG_DFL,
/// `torii log | head` dies quietly mid-pipe, exactly like git does.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: trivial signal-disposition change before any other code
    // runs; no handlers or signal-unsafe state involved.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

fn main() -> Result<()> {
    reset_sigpipe();
    transport::register_all();
    let cli = Cli::parse();
    let result = cli.execute();
    updater::maybe_notify();
    result
}
