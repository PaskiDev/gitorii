//! History/reflog view state + ops.

use super::*;
use git2::Repository;

#[allow(dead_code)]
pub struct ReflogEntry {
    pub id: String,
    /// Reflog `update message` (e.g. "commit: feat: …"). Surfaced in the
    /// reflog panel when the history view gains its interactive sweep
    /// (0.7.3 just renders the id column for now).
    pub message: String,
    /// Wall-clock time of the reflog entry, rendered relative.
    pub time: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HistoryConfirm {
    None,
    CherryPick,
    Clean,
    RemoveFile,
    Rebase,
    RewriteStart,
    RewriteEnd,
    Blame,
    Scan,
}

pub struct HistoryState {
    pub reflog: Vec<ReflogEntry>,
    pub idx: usize,
    pub confirm: HistoryConfirm,
    pub input: String,
    pub input2: String,
    pub scan_full: bool,
    pub ops_mode: bool,
    pub ops_idx: usize,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            reflog: vec![],
            idx: 0,
            confirm: HistoryConfirm::None,
            input: String::new(),
            input2: String::new(),
            scan_full: false,
            ops_mode: false,
            ops_idx: 0,
        }
    }
}

// ── Remote state ──────────────────────────────────────────────────────────────

impl App {
    #[allow(dead_code)]
    pub(crate) fn load_reflog(&mut self) {
        self.history_view.reflog.clear();
        let Ok(repo) = Repository::discover(&self.repo_path) else {
            return;
        };
        let Ok(reflog) = repo.reflog("HEAD") else {
            return;
        };
        for entry in reflog.iter() {
            let id = entry.id_new().to_string()[..7].to_string();
            let message = entry.message().unwrap_or("").to_string();
            let time = format_age(entry.committer().when().seconds());
            self.history_view
                .reflog
                .push(ReflogEntry { id, message, time });
        }
        self.history_view.idx = 0;
    }

    pub fn history_move_up(&mut self) {
        if self.history_view.idx > 0 {
            self.history_view.idx -= 1;
        }
    }

    pub fn history_move_down(&mut self) {
        if self.history_view.idx + 1 < self.history_view.reflog.len() {
            self.history_view.idx += 1;
        }
    }

    // ── Remote helpers ───────────────────────────────────────────────────────
}
