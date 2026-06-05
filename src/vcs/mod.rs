//! Version-control core: libgit2-based wrappers around the operations
//! that touch the object database (commit, tag, snapshot, reauthor,
//! scanner, patch). Everything in here is purely local; network ops
//! live in [`crate::workspace`] (mirror / remote / workspace) and
//! platform-side APIs live in [`crate::platforms`].

pub mod core;
pub mod core_extensions;
pub mod core_tag;
pub mod sign;
pub mod tag;
pub mod snapshot;
pub mod patch;
pub mod history_reauthor;
pub mod commit_scan;
pub mod scanner;
