//! Workspace view state + ops.

use super::*;

pub struct WorkspaceRepo {
    pub path: String,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub dirty: bool,
}

pub struct WorkspaceEntry {
    pub name: String,
    pub repos: Vec<WorkspaceRepo>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceFocus {
    Workspaces,
    Repos,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceConfirm {
    None,
    DeleteWorkspace,
    RemoveRepo,
    SaveMessage,
    AddRepoPath,
    RenameWorkspace,
}

pub struct WorkspaceState {
    pub workspaces: Vec<WorkspaceEntry>,
    pub ws_idx: usize,
    pub repo_idx: usize,
    pub focus: WorkspaceFocus,
    pub status: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub confirm: WorkspaceConfirm,
    pub input: String,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            workspaces: vec![],
            ws_idx: 0,
            repo_idx: 0,
            focus: WorkspaceFocus::Workspaces,
            status: None,
            ops_mode: false,
            ops_idx: 0,
            confirm: WorkspaceConfirm::None,
            input: String::new(),
        }
    }
}

// ── Config state ─────────────────────────────────────────────────────────────

impl App {
    pub fn workspace_repo_paths(&self) -> Vec<String> {
        let name = match &self.active_workspace {
            Some(n) => n,
            None => return vec![],
        };
        if let Some(ws) = self
            .workspace_view
            .workspaces
            .iter()
            .find(|ws| &ws.name == name)
        {
            return ws.repos.iter().map(|r| r.path.clone()).collect();
        }
        vec![]
    }

    pub fn workspace_has_siblings(&self) -> bool {
        self.workspace_repo_paths().len() > 1
    }

    pub fn open_repo_picker(&mut self) {
        let paths = self.workspace_repo_paths();
        if paths.len() <= 1 {
            return;
        }
        let current = std::fs::canonicalize(&self.repo_path).ok();
        self.repo_picker_idx = paths
            .iter()
            .position(|p| std::fs::canonicalize(p).ok() == current)
            .unwrap_or(0);
        self.repo_picker_open = true;
    }

    pub(crate) fn load_workspaces(&mut self) {
        self.workspace_view.workspaces.clear();
        // 0.7.39 fix — `torii workspace add` (CLI) writes to
        // `~/.config/torii/workspaces.toml` (canonical via dirs's
        // config_dir), but the TUI loader used to read from
        // `~/.torii/workspaces.toml` (legacy). The two paths never
        // matched, so any workspace created from the shell stayed
        // invisible in the TUI. Now we prefer the canonical path and
        // fall back to the legacy one for installs that pre-date the
        // move.
        let Some(ws_path) = workspaces_toml_path() else {
            return;
        };
        if !ws_path.exists() {
            return;
        }
        let Ok(content) = std::fs::read_to_string(&ws_path) else {
            return;
        };
        let mut current_ws: Option<WorkspaceEntry> = None;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                if let Some(ws) = current_ws.take() {
                    self.workspace_view.workspaces.push(ws);
                }
                let name = line.trim_matches(|c| c == '[' || c == ']').to_string();
                current_ws = Some(WorkspaceEntry {
                    name,
                    repos: vec![],
                });
            } else if line.starts_with("path") {
                if let Some(ws) = current_ws.as_mut() {
                    let path = line
                        .split('=')
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .trim_matches('"')
                        .to_string();
                    let (branch, ahead, behind, dirty) = repo_quick_status(&path);
                    ws.repos.push(WorkspaceRepo {
                        path,
                        branch,
                        ahead,
                        behind,
                        dirty,
                    });
                }
            }
        }
        if let Some(ws) = current_ws.take() {
            self.workspace_view.workspaces.push(ws);
        }
        self.workspace_view.ws_idx = 0;
        self.workspace_view.repo_idx = 0;
    }

    pub fn workspace_move_up(&mut self) {
        match self.workspace_view.focus {
            WorkspaceFocus::Workspaces => {
                if self.workspace_view.ws_idx > 0 {
                    self.workspace_view.ws_idx -= 1;
                }
                self.workspace_view.repo_idx = 0;
            }
            WorkspaceFocus::Repos => {
                if self.workspace_view.repo_idx > 0 {
                    self.workspace_view.repo_idx -= 1;
                }
            }
        }
    }

    pub fn workspace_move_down(&mut self) {
        match self.workspace_view.focus {
            WorkspaceFocus::Workspaces => {
                if self.workspace_view.ws_idx + 1 < self.workspace_view.workspaces.len() {
                    self.workspace_view.ws_idx += 1;
                }
                self.workspace_view.repo_idx = 0;
            }
            WorkspaceFocus::Repos => {
                let repo_len = self
                    .workspace_view
                    .workspaces
                    .get(self.workspace_view.ws_idx)
                    .map(|ws| ws.repos.len())
                    .unwrap_or(0);
                if self.workspace_view.repo_idx + 1 < repo_len {
                    self.workspace_view.repo_idx += 1;
                }
            }
        }
    }

    pub fn workspace_focus_repos(&mut self) {
        self.workspace_view.focus = WorkspaceFocus::Repos;
        self.workspace_view.repo_idx = 0;
    }

    pub fn workspace_focus_workspaces(&mut self) {
        self.workspace_view.focus = WorkspaceFocus::Workspaces;
    }

    // ── Config helpers ───────────────────────────────────────────────────────
}
