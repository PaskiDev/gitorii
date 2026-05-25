use std::io::IsTerminal;
use std::time::Duration;
use update_informer::{registry, Check};

use crate::config::ToriiConfig;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check crates.io for a newer version. Returns `Some(version_string)` if one exists.
/// Silent on errors, opt-out, or timeouts. Result is cached locally for `interval_hours`.
pub fn check() -> Option<String> {
    let cfg = ToriiConfig::load_global().unwrap_or_default();
    if !cfg.update.check {
        return None;
    }
    let interval = Duration::from_secs(cfg.update.interval_hours.saturating_mul(3600));
    let informer = update_informer::new(registry::Crates, PKG_NAME, PKG_VERSION)
        .interval(interval)
        .timeout(Duration::from_secs(2));
    informer.check_version().ok().flatten().map(|v| v.to_string())
}

/// CLI banner — printed on stderr after command execution if an update is available.
/// Silent if stderr is not a tty.
pub fn maybe_notify() {
    if !std::io::stderr().is_terminal() {
        return;
    }
    if let Some(new_version) = check() {
        let cmd = update_command();
        eprintln!();
        eprintln!("💡 New version of torii available: {} → {}", PKG_VERSION, new_version);
        eprintln!("   Update: {}", cmd);
        eprintln!("   Disable: torii config set update.check false");
    }
}

/// Suggested update command based on install method.
pub fn update_command() -> &'static str {
    let exe = std::env::current_exe().ok();
    let path = exe.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();

    if path.contains("/.cargo/bin/") {
        "cargo install gitorii  (or: cargo binstall gitorii)"
    } else if path.starts_with("/usr/local/bin")
        || path.starts_with("/usr/bin")
        || path.starts_with("/opt/")
    {
        "curl -LsSf https://github.com/paskidev/gitorii/releases/latest/download/gitorii-installer.sh | sh"
    } else {
        "cargo binstall gitorii  (or re-run your installer)"
    }
}
