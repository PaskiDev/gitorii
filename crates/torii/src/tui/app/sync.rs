//! Sync view state + ops.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum SyncOp {
    PullPush,
    PullOnly,
    PushOnly,
    ForcePush,
    Fetch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    Idle,
    Running,
    Done(String),
    Error(String),
}

pub struct SyncState {
    pub selected_op: SyncOp,
    pub status: SyncStatus,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            selected_op: SyncOp::PullPush,
            status: SyncStatus::Idle,
        }
    }
}

// ── Tag state ────────────────────────────────────────────────────────────────

impl App {
    pub fn sync_op_next(&mut self) {
        self.sync_view.selected_op = match self.sync_view.selected_op {
            SyncOp::PullPush => SyncOp::PullOnly,
            SyncOp::PullOnly => SyncOp::PushOnly,
            SyncOp::PushOnly => SyncOp::ForcePush,
            SyncOp::ForcePush => SyncOp::Fetch,
            SyncOp::Fetch => SyncOp::PullPush,
        };
    }

    pub fn sync_op_prev(&mut self) {
        self.sync_view.selected_op = match self.sync_view.selected_op {
            SyncOp::PullPush => SyncOp::Fetch,
            SyncOp::PullOnly => SyncOp::PullPush,
            SyncOp::PushOnly => SyncOp::PullOnly,
            SyncOp::ForcePush => SyncOp::PushOnly,
            SyncOp::Fetch => SyncOp::ForcePush,
        };
    }

    // ── Tag helpers ──────────────────────────────────────────────────────────
}
