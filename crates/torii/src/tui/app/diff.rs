//! Diff view state + loaders.

use std::collections::BTreeSet;

use super::*;
use crate::snapshot::{SnapshotDiffFile, SnapshotDiffKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffMode {
    #[default]
    Patch,
    SnapshotTree,
    SnapshotFile,
}

#[derive(Default)]
pub struct DiffState {
    pub title: String,
    pub lines: Vec<DiffLine>,
    pub scroll: usize,
    pub mode: DiffMode,
    pub snapshot_files: Vec<SnapshotDiffFile>,
    pub tree_rows: Vec<BrowseRow>,
    pub tree_idx: usize,
    pub tree_collapsed: BTreeSet<String>,
}

// ── Log state ────────────────────────────────────────────────────────────────

impl App {
    fn clear_snapshot_diff(&mut self) {
        self.diff.mode = DiffMode::Patch;
        self.diff.snapshot_files.clear();
        self.diff.tree_rows.clear();
        self.diff.tree_idx = 0;
        self.diff.tree_collapsed.clear();
    }

    pub(crate) fn load_diff(&mut self) {
        self.clear_snapshot_diff();
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
        self.clear_snapshot_diff();
        let idx = self.log.idx;
        if let Some(commit) = self.commits.get(idx) {
            self.diff.title = format!("{} {}", commit.hash, commit.message);
            self.diff.lines = read_commit_diff(&self.repo_path, &commit.full_hash);
            self.diff.scroll = 0;
        }
    }

    pub(crate) fn load_commit_diff(&mut self) {
        self.clear_snapshot_diff();
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

    pub fn rebuild_diff_tree(&mut self) {
        let paths: Vec<String> = self
            .diff
            .snapshot_files
            .iter()
            .map(|f| f.path.clone())
            .collect();
        let keep = self
            .diff
            .tree_rows
            .get(self.diff.tree_idx)
            .map(|r| r.path.clone());
        self.diff.tree_rows = tree_rows(&paths, &self.diff.tree_collapsed);
        if let Some(path) = keep {
            if let Some(i) = self.diff.tree_rows.iter().position(|r| r.path == path) {
                self.diff.tree_idx = i;
                return;
            }
        }
        let len = self.diff.tree_rows.len();
        if len == 0 {
            self.diff.tree_idx = 0;
        } else if self.diff.tree_idx >= len {
            self.diff.tree_idx = len - 1;
        }
    }

    pub fn diff_tree_move_up(&mut self) {
        if self.diff.tree_idx > 0 {
            self.diff.tree_idx -= 1;
        }
    }

    pub fn diff_tree_move_down(&mut self) {
        if self.diff.tree_idx + 1 < self.diff.tree_rows.len() {
            self.diff.tree_idx += 1;
        }
    }

    pub fn diff_tree_toggle_fold(&mut self) {
        let Some(row) = self.diff.tree_rows.get(self.diff.tree_idx) else {
            return;
        };
        if !row.is_dir {
            self.diff_tree_open_file();
            return;
        }
        let path = row.path.clone();
        if !self.diff.tree_collapsed.remove(&path) {
            self.diff.tree_collapsed.insert(path);
        }
        self.rebuild_diff_tree();
    }

    pub fn diff_tree_fold(&mut self) {
        let Some(row) = self.diff.tree_rows.get(self.diff.tree_idx) else {
            return;
        };
        let path = row.path.clone();
        let is_dir = row.is_dir;
        if is_dir && !self.diff.tree_collapsed.contains(&path) {
            self.diff.tree_collapsed.insert(path);
            self.rebuild_diff_tree();
            return;
        }
        if let Some(slash) = path.rfind('/') {
            let parent = &path[..slash];
            if let Some(i) = self.diff.tree_rows.iter().position(|r| r.path == parent) {
                self.diff.tree_idx = i;
            }
            self.diff.tree_collapsed.insert(parent.to_string());
            self.rebuild_diff_tree();
        }
    }

    pub fn diff_tree_unfold(&mut self) {
        let Some(row) = self.diff.tree_rows.get(self.diff.tree_idx) else {
            return;
        };
        if row.is_dir {
            let path = row.path.clone();
            if self.diff.tree_collapsed.remove(&path) {
                self.rebuild_diff_tree();
            }
        } else {
            self.diff_tree_open_file();
        }
    }

    pub fn diff_tree_open_file(&mut self) {
        let Some(row) = self.diff.tree_rows.get(self.diff.tree_idx) else {
            return;
        };
        if row.is_dir {
            self.diff_tree_toggle_fold();
            return;
        }
        let path = row.path.clone();
        let Some(file) = self
            .diff
            .snapshot_files
            .iter()
            .find(|f| f.path == path)
            .cloned()
        else {
            return;
        };
        self.diff.title = path;
        self.diff.lines = if file.kind == SnapshotDiffKind::Binary {
            vec![DiffLine {
                kind: DiffLineKind::Header,
                content: "Binary files differ".to_string(),
                line_no: None,
            }]
        } else {
            parse_unified_diff(&file.patch)
        };
        self.diff.scroll = 0;
        self.diff.mode = DiffMode::SnapshotFile;
    }

    pub fn diff_file_kind(&self, path: &str) -> Option<SnapshotDiffKind> {
        self.diff
            .snapshot_files
            .iter()
            .find(|f| f.path == path)
            .map(|f| f.kind)
    }

    pub fn leave_snapshot_file(&mut self) {
        if self.diff.mode == DiffMode::SnapshotFile {
            self.diff.mode = DiffMode::SnapshotTree;
            self.diff.lines.clear();
            self.diff.scroll = 0;
        }
    }

    // ── Log helpers ──────────────────────────────────────────────────────────
}
