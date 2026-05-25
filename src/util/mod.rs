//! Cross-cutting utilities: config + auth, URL parsing, ignore rules,
//! hooks, HTTP / SSH transport helpers, version probe, and the
//! subprocess wrappers for external binaries (`gpg`, `rad`).

pub mod config;
pub mod auth;
pub mod duration;
pub mod url;
pub mod toriignore;
pub mod hooks;
pub mod updater;
pub mod http;
pub mod ssh;
pub mod graph;
pub mod gpg;
pub mod radicle;
