//! Cross-cutting utilities: config + auth, URL parsing, ignore rules,
//! hooks, HTTP / SSH transport helpers, version probe, and the
//! subprocess wrappers for external binaries (`gpg`, `rad`).

pub mod auth;
pub mod config;
pub mod duration;
pub mod gpg;
pub mod graph;
pub mod hooks;
pub mod http;
pub mod oauth;
pub mod radicle;
pub mod ssh;
pub mod toriignore;
pub mod updater;
pub mod url;
