//! Platform-side surfaces: pull requests, issues, CI pipelines, releases
//! and package registries across GitHub / GitLab / Gitea (Codeberg /
//! Forgejo) / Sourcehut / Radicle.
//!
//! Each module owns one trait (e.g. [`pr::PrClient`]) and one factory
//! (e.g. [`pr::get_pr_client`]). The factory dispatches on the platform
//! name string returned by [`pr::detect_platform_from_remote_named`],
//! which is the canonical URL → (`platform`, `owner`, `repo`) parser
//! shared by every CLI command that accepts `--remote`.

pub mod pr;
pub mod issue;
pub mod pipeline;
pub mod release;
pub mod package;
pub mod runner;
pub mod registry;

// Per-platform client implementations.
pub mod azure;
pub mod bitbucket;
pub mod gitea;
pub mod github;
pub mod gitlab;
pub mod radicle;
pub mod sourcehut;
