//! GitLab clients.

pub mod issue;
pub mod package;
pub mod pipeline;
pub mod pr;
pub mod release;
pub mod runner;
pub use issue::*;
pub use package::*;
pub use pipeline::*;
pub use pr::*;
pub use release::*;
pub use runner::*;
