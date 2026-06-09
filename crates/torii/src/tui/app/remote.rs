//! Remote + mirror view state + ops.

use super::*;
use git2::Repository;

pub struct RemoteEntry {
    pub name: String,
    pub git_name: String,
    pub url: String,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteConfirm {
    None,
    Remove,
    AddName,
    AddUrl,
    Rename,
    EditUrl,
    MirrorRename,
    MirrorAddPlatform,
    MirrorAddAccount,
    MirrorAddRepo,
    MirrorAddType,
}

pub struct RemoteState {
    pub remotes: Vec<RemoteEntry>,
    pub mirrors: Vec<MirrorEntry>,
    pub idx: usize,
    pub status: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub confirm: RemoteConfirm,
    pub new_name: String,
    pub new_url: String,
    pub new_mirror_platform: String,
    pub new_mirror_account: String,
    pub new_mirror_repo: String,
    pub new_mirror_type: usize, // 0=replica, 1=primary
}

impl RemoteState {
    pub fn selected_is_mirror(&self) -> bool {
        self.idx >= self.remotes.len()
    }
    pub fn selected_remote(&self) -> Option<&RemoteEntry> {
        if self.selected_is_mirror() {
            return None;
        }
        self.remotes.get(self.idx)
    }
    pub fn selected_mirror(&self) -> Option<&MirrorEntry> {
        if !self.selected_is_mirror() {
            return None;
        }
        self.mirrors.get(self.idx - self.remotes.len())
    }
    pub fn total_len(&self) -> usize {
        self.remotes.len() + self.mirrors.len()
    }
}

impl Default for RemoteState {
    fn default() -> Self {
        Self {
            remotes: vec![],
            mirrors: vec![],
            idx: 0,
            status: None,
            ops_mode: false,
            ops_idx: 0,
            confirm: RemoteConfirm::None,
            new_name: String::new(),
            new_url: String::new(),
            new_mirror_platform: String::new(),
            new_mirror_account: String::new(),
            new_mirror_repo: String::new(),
            new_mirror_type: 0,
        }
    }
}

// ── Mirror state ──────────────────────────────────────────────────────────────

pub struct MirrorEntry {
    pub name: String,
    pub platform: String,
    pub url: String,
    pub kind: String,
    pub account: String,
    pub repo: String,
}

#[derive(Default)]
pub struct MirrorState {
    pub mirrors: Vec<MirrorEntry>,
    pub idx: usize,
    pub status: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
}

// ── PR state ──────────────────────────────────────────────────────────────────

impl App {
    pub(crate) fn load_remotes(&mut self) {
        self.remote_view.remotes.clear();
        self.remote_view.mirrors.clear();
        // git remotes
        if let Ok(repo) = Repository::discover(&self.repo_path) {
            if let Ok(remotes) = repo.remotes() {
                for name in remotes.iter().flatten() {
                    let url = repo
                        .find_remote(name)
                        .ok()
                        .and_then(|r| r.url().map(|u| u.to_string()))
                        .unwrap_or_default();
                    let platform = detect_platform(&url);
                    let display_name = shorten_remote_name(name, &platform);
                    self.remote_view.remotes.push(RemoteEntry {
                        name: display_name,
                        git_name: name.to_string(),
                        url,
                        platform,
                    });
                }
            }
        }
        // torii mirrors
        let mirrors_path = std::path::Path::new(&self.repo_path).join(".torii/mirrors.json");
        if mirrors_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&mirrors_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(arr) = json["mirrors"].as_array() {
                        for m in arr {
                            let name = m["name"].as_str().unwrap_or("").to_string();
                            let platform = m["platform"].as_str().unwrap_or("").to_string();
                            let url = m["url"].as_str().unwrap_or("").to_string();
                            let kind = match m["mirror_type"].as_str().unwrap_or("Replica") {
                                "Primary" | "Master" => "primary",
                                _ => "replica",
                            }
                            .to_string();
                            let account = m["account_name"].as_str().unwrap_or("").to_string();
                            let repo = m["repo_name"].as_str().unwrap_or("").to_string();
                            self.remote_view.mirrors.push(MirrorEntry {
                                name,
                                platform,
                                url,
                                kind,
                                account,
                                repo,
                            });
                        }
                    }
                }
            }
        }
        self.remote_view.idx = 0;
    }

    pub fn reload_remotes(&mut self) {
        self.load_remotes();
    }

    pub fn remote_move_up(&mut self) {
        if self.remote_view.idx > 0 {
            self.remote_view.idx -= 1;
        }
    }

    pub fn remote_move_down(&mut self) {
        if self.remote_view.idx + 1 < self.remote_view.total_len() {
            self.remote_view.idx += 1;
        }
    }

    pub fn mirror_move_up(&mut self) {
        if self.mirror_view.idx > 0 {
            self.mirror_view.idx -= 1;
        }
    }

    pub fn mirror_move_down(&mut self) {
        if self.mirror_view.idx + 1 < self.mirror_view.mirrors.len() {
            self.mirror_view.idx += 1;
        }
    }

    // ── Workspace helpers ────────────────────────────────────────────────────
}
