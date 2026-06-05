//! Dashboard view state.

use super::*;

pub struct DashboardState {
    pub selected_panel: Panel,
    pub staged_idx: usize,
    pub unstaged_idx: usize,
    pub untracked_idx: usize,
    pub log_idx: usize,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            selected_panel: Panel::Unstaged,
            staged_idx: 0,
            unstaged_idx: 0,
            untracked_idx: 0,
            log_idx: 0,
        }
    }
}

// ── Diff state ───────────────────────────────────────────────────────────────
