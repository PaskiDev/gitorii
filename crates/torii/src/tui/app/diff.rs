//! Diff view state + loaders.

use super::*;

pub struct DiffState {
    pub title: String,
    pub lines: Vec<DiffLine>,
    pub scroll: usize,
}

impl Default for DiffState {
    fn default() -> Self {
        Self {
            title: String::new(),
            lines: vec![],
            scroll: 0,
        }
    }
}

// ── Log state ────────────────────────────────────────────────────────────────

impl App {
    pub(crate) fn load_diff(&mut self) {
        let panel = &self.dashboard.selected_panel;
        let idx = match panel {
            Panel::Staged => self.dashboard.staged_idx,
            Panel::Unstaged => self.dashboard.unstaged_idx,
            Panel::Untracked => self.dashboard.untracked_idx,
            Panel::Log => {
                self.load_commit_diff();
                return;
            }
        };

        let files = match panel {
            Panel::Staged => &self.staged,
            Panel::Unstaged => &self.unstaged,
            Panel::Untracked => &self.untracked,
            Panel::Log => unreachable!(),
        };

        if let Some(entry) = files.get(idx) {
            self.diff.title = entry.path.clone();
            self.diff.lines = read_file_diff(
                &self.repo_path,
                &entry.path,
                entry.status == FileStatus::Staged,
            );
            self.diff.scroll = 0;
        }
    }

    pub(crate) fn load_commit_diff_from_log(&mut self) {
        let idx = self.log.idx;
        if let Some(commit) = self.commits.get(idx) {
            self.diff.title = format!("{} {}", commit.hash, commit.message);
            self.diff.lines = read_commit_diff(&self.repo_path, &commit.full_hash);
            self.diff.scroll = 0;
        }
    }

    pub(crate) fn load_commit_diff(&mut self) {
        let idx = self.dashboard.log_idx;
        if let Some(commit) = self.commits.get(idx) {
            self.diff.title = format!("{} {}", commit.hash, commit.message);
            self.diff.lines = read_commit_diff(&self.repo_path, &commit.full_hash);
            self.diff.scroll = 0;
        }
    }

    pub fn diff_scroll_up(&mut self) {
        if self.diff.scroll > 0 {
            self.diff.scroll -= 1;
        }
    }

    pub fn diff_scroll_down(&mut self) {
        let max = self.diff.lines.len().saturating_sub(1);
        if self.diff.scroll < max {
            self.diff.scroll += 1;
        }
    }

    pub fn diff_page_up(&mut self) {
        self.diff.scroll = self.diff.scroll.saturating_sub(20);
    }

    pub fn diff_page_down(&mut self) {
        let max = self.diff.lines.len().saturating_sub(1);
        self.diff.scroll = (self.diff.scroll + 20).min(max);
    }

    // ── Log helpers ──────────────────────────────────────────────────────────
}
