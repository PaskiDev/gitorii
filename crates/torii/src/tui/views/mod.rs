pub mod auth;
pub mod bisect;
pub mod branch;
pub mod commit;
pub mod config;
pub mod dashboard;
pub mod diff;
pub mod help;
pub mod issue;
pub mod log;
pub mod mirror;
pub mod pr;
pub mod remote;
pub mod snapshot;
pub mod submodule;
pub mod sync;
pub mod tag;
pub mod workspace;
pub mod worktree;
// 0.7.12 — unified Platform view (pipelines/jobs/releases/packages).
pub mod platform;
// `history` and `settings` modules removed in 0.7.3 — their renders are
// now served from `log` and `config` respectively (see ui.rs dispatcher).
