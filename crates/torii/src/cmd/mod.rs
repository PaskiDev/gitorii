//! Thin wrappers around `git` subcommands that don't yet have a
//! native libgit2 implementation in torii: archive, bisect, clean,
//! describe, fileops (remove/rename), grep, notes, submodule, subtree,
//! worktree. Each module exposes the same CLI grammar as the `git`
//! original but plugged into torii's error type + `.toriignore`
//! handling.

pub mod archive;
pub mod bisect;
pub mod clean;
pub mod describe;
pub mod fileops;
pub mod grep;
pub mod notes;
pub mod submodule;
pub mod subtree;
pub mod worktree;
