//! Whether the library may write to stdout.
//!
//! Core carries a handful of CLI conveniences — a checkout progress line, a
//! "fetching…" banner — that print straight to stdout. The TUI calls the very
//! same functions, and there stdout is the alternate screen: the text lands
//! wherever the cursor happens to be, and ratatui never paints over it,
//! because its own buffer never changed. The result is a progress line stuck
//! under the sidebar until the next full redraw.
//!
//! A front end that owns the screen says so once, and core keeps quiet.

use std::sync::atomic::{AtomicBool, Ordering};

static SILENT: AtomicBool = AtomicBool::new(false);

/// Tell the library it does not own stdout. The TUI calls this before it
/// enters the alternate screen.
pub fn set_silent(silent: bool) {
    SILENT.store(silent, Ordering::Relaxed);
}

/// Whether the library has been told to keep off stdout.
pub fn silent() -> bool {
    SILENT.load(Ordering::Relaxed)
}

/// A line core wants to say to a human running the CLI. Dropped when a front
/// end owns the screen.
pub fn note(line: impl std::fmt::Display) {
    if !silent() {
        println!("{}", line);
    }
}
