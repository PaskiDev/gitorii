pub mod dashboard;
pub mod diff;
pub mod log;
pub mod branch;
pub mod commit;
pub mod snapshot;
pub mod sync;
pub mod tag;
pub mod remote;
pub mod mirror;
pub mod workspace;
pub mod pr;
pub mod issue;
pub mod config;
pub mod help;
pub mod worktree;
pub mod submodule;
pub mod bisect;
pub mod auth;
// 0.7.12 — unified Platform view (pipelines/jobs/releases/packages).
pub mod platform;
// `history` and `settings` modules removed in 0.7.3 — their renders are
// now served from `log` and `config` respectively (see ui.rs dispatcher).
