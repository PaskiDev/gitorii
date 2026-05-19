// Copyright (c) 2026 Torii Project. All Rights Reserved.
// Licensed under the Torii Source-Available License (Non-Commercial Fork-Friendly) v1.0.
// See LICENSE file in the project root for full license information.
// Commercial use is prohibited without explicit written permission from the copyright holder.

mod auth;
mod cli;
mod cloud;
mod config;
mod core;
mod core_extensions;
mod core_tag;
mod duration;
mod error;
mod mirror;
mod remote;
mod hooks;
mod scanner;
mod snapshot;
mod ssh;
mod tag;
mod commit_scan;
mod graph;
mod history_reauthor;
mod toriignore;
mod transport;
mod updater;
mod url;
mod versioning;
mod pr;
mod issue;
mod pipeline;
mod package;
mod release;
mod tui;
mod archive;
mod bisect;
mod clean;
mod describe;
mod fileops;
mod grep;
mod notes;
mod patch;
mod submodule;
mod subtree;
mod workspace;
mod worktree;

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
