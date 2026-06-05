//! Multi-repo and mirror management. Whereas [`crate::vcs`] is about a
//! single repository's object database, this module is about the
//! constellation of remotes and the workspace concept (N repos
//! treated as one for status / sync / save fan-out).

pub mod mirror;
pub mod remote;
// The single-file workspace logic kept its old `workspace.rs` name; it
// gets re-exported at crate root via main.rs so call-sites that say
// `crate::workspace::...` still resolve.
pub mod workspace;
