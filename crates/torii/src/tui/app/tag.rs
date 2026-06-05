//! Tag view state + ops.

use super::*;
use git2::Repository;

pub struct TagEntry {
    pub name: String,
    pub message: String,
    pub hash: String,
    pub time: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TagConfirm {
    None,
    Delete,
    CreateName,
    CreateMessage,
}

pub struct TagState {
    pub tags: Vec<TagEntry>,
    pub idx: usize,
    pub confirm: TagConfirm,
    pub new_name: String,
    pub new_message: String,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
}

impl Default for TagState {
    fn default() -> Self {
        Self {
            tags: vec![],
            idx: 0,
            confirm: TagConfirm::None,
            new_name: String::new(),
            new_message: String::new(),
            ops_mode: false,
            ops_idx: 0,
            search_mode: false,
            search_query: String::new(),
            filtered: vec![],
        }
    }
}

// ── History state ─────────────────────────────────────────────────────────────

impl App {
    pub(crate) fn load_tags(&mut self) {
        self.tag_view.tags.clear();
        let Ok(repo) = Repository::discover(&self.repo_path) else {
            return;
        };
        let _ = repo.tag_foreach(|oid, name| {
            let name = String::from_utf8_lossy(name).to_string();
            let name = name.trim_start_matches("refs/tags/").to_string();
            let commit = repo
                .find_object(oid, None)
                .ok()
                .and_then(|obj| obj.peel_to_commit().ok());
            let (message, hash, time, timestamp) = commit
                .map(|c| {
                    (
                        c.summary().unwrap_or("").to_string(),
                        format!("{:.7}", c.id()),
                        format_age(c.time().seconds()),
                        c.time().seconds(),
                    )
                })
                .unwrap_or_default();
            self.tag_view.tags.push(TagEntry {
                name,
                message,
                hash,
                time,
                timestamp,
            });
            true
        });
        self.tag_view
            .tags
            .sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.tag_view.idx = self
            .tag_view
            .idx
            .min(self.tag_view.tags.len().saturating_sub(1));
    }

    pub fn tag_move_up(&mut self) {
        if self.tag_view.idx > 0 {
            self.tag_view.idx -= 1;
        }
    }

    pub fn tag_move_down(&mut self) {
        if self.tag_view.idx + 1 < self.tag_view.tags.len() {
            self.tag_view.idx += 1;
        }
    }

    // ── History helpers ──────────────────────────────────────────────────────

    pub fn tag_update_filter(&mut self) {
        let q = self.tag_view.search_query.to_lowercase();
        self.tag_view.filtered = self
            .tag_view
            .tags
            .iter()
            .enumerate()
            .filter(|(_, t)| t.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.tag_view.idx = self.tag_view.filtered.first().copied().unwrap_or(0);
    }
}
