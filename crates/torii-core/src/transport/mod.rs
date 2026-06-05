//! Custom git transports — pure-Rust HTTPS via reqwest, SSH via russh.
//!
//! Registered globally at process start so libgit2 (compiled without HTTPS/SSH)
//! delegates network ops to our handlers.

mod https;
mod ssh;

use std::sync::Once;

static REGISTER: Once = Once::new();

/// Register all custom transports. Idempotent. Call once at process start.
pub fn register_all() {
    REGISTER.call_once(|| {
        unsafe {
            git2::transport::register("https", https::factory)
                .expect("register https transport");
            git2::transport::register("http", https::factory)
                .expect("register http transport");
            git2::transport::register("ssh", ssh::factory)
                .expect("register ssh transport");
            git2::transport::register("ssh+git", ssh::factory)
                .expect("register ssh+git transport");
            git2::transport::register("git+ssh", ssh::factory)
                .expect("register git+ssh transport");
        }
    });
}
