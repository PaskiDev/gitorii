//! Branch view state + ops.

use super::*;
use git2::Repository;

pub struct BranchEntry {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BranchConfirm {
    None,
    Delete,
    NewBranch,
}

pub struct BranchState {
    pub branches: Vec<BranchEntry>,
    pub idx: usize,
    pub confirm: BranchConfirm,
    pub new_name: String,
    pub status: Option<String>,
    pub current_has_upstream: bool,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
}

impl Default for BranchState {
    fn default() -> Self {
        Self {
            branches: vec![],
            idx: 0,
            confirm: BranchConfirm::None,
            new_name: String::new(),
            status: None,
            current_has_upstream: false,
            ops_mode: false,
            ops_idx: 0,
            search_mode: false,
            search_query: String::new(),
            filtered: vec![],
        }
    }
}

// ── Commit state ─────────────────────────────────────────────────────────────

impl App {
    pub(crate) fn load_branches(&mut self) {
        let Ok(repo) = Repository::discover(&self.repo_path) else {
            return;
        };
        let Ok(branches) = repo.branches(None) else {
            return;
        };

        self.branch_view.branches.clear();
        for branch in branches.flatten() {
            let (b, btype) = branch;
            let Ok(name) = b.name() else { continue };
            let Some(name) = name else { continue };
            let is_current = b.is_head();
            let is_remote = btype == git2::BranchType::Remote;
            self.branch_view.branches.push(BranchEntry {
                name: name.to_string(),
                is_current,
                is_remote,
            });
        }
        self.branch_view.idx = self
            .branch_view
            .branches
            .iter()
            .position(|b| b.is_current)
            .unwrap_or(0);

        self.branch_view.current_has_upstream = repo
            .branches(Some(git2::BranchType::Local))
            .ok()
            .map(|branches| {
                branches
                    .flatten()
                    .any(|(b, _)| b.is_head() && b.upstream().is_ok())
            })
            .unwrap_or(false);
    }

    pub fn branch_move_up(&mut self) {
        if self.branch_view.idx > 0 {
            self.branch_view.idx -= 1;
        }
    }

    pub fn branch_move_down(&mut self) {
        if self.branch_view.idx + 1 < self.branch_view.branches.len() {
            self.branch_view.idx += 1;
        }
    }

    // ── Commit helpers ───────────────────────────────────────────────────────

    pub fn branch_update_filter(&mut self) {
        let q = self.branch_view.search_query.to_lowercase();
        self.branch_view.filtered = self
            .branch_view
            .branches
            .iter()
            .enumerate()
            .filter(|(_, b)| b.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.branch_view.idx = self.branch_view.filtered.first().copied().unwrap_or(0);
    }
}
