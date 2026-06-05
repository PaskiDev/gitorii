//! Snapshot view state + ops.

use super::*;

pub struct SnapshotEntry {
    pub id: String,
    pub name: String,
    pub time: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotFocus {
    List,
    Create,
    AutoConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoSnapshotInterval {
    Off,
    Min5,
    Min15,
    Min30,
    Hour1,
}

impl AutoSnapshotInterval {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Min5 => "every 5 min",
            Self::Min15 => "every 15 min",
            Self::Min30 => "every 30 min",
            Self::Hour1 => "every 1 hour",
        }
    }
    pub fn secs(&self) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::Min5 => Some(300),
            Self::Min15 => Some(900),
            Self::Min30 => Some(1800),
            Self::Hour1 => Some(3600),
        }
    }
    pub fn all() -> &'static [AutoSnapshotInterval] {
        &[Self::Off, Self::Min5, Self::Min15, Self::Min30, Self::Hour1]
    }
}

pub struct SnapshotState {
    pub snapshots: Vec<SnapshotEntry>,
    pub idx: usize,
    pub focus: SnapshotFocus,
    pub create_name: String,
    pub auto_interval: AutoSnapshotInterval,
    pub auto_interval_idx: usize,
    pub last_auto_snapshot: u64,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
}

impl Default for SnapshotState {
    fn default() -> Self {
        Self {
            snapshots: vec![],
            idx: 0,
            focus: SnapshotFocus::List,
            create_name: String::new(),
            auto_interval: AutoSnapshotInterval::Off,
            auto_interval_idx: 0,
            last_auto_snapshot: 0,
            ops_mode: false,
            ops_idx: 0,
            search_mode: false,
            search_query: String::new(),
            filtered: vec![],
        }
    }
}

// ── Sync state ───────────────────────────────────────────────────────────────

impl App {
    pub fn load_snapshots(&mut self) {
        // Snapshots stored in .git/torii-snapshots/ — read metadata
        self.snapshot_view.snapshots.clear();
        let snap_dir = std::path::Path::new(&self.repo_path).join(".git/torii-snapshots");
        if !snap_dir.exists() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(&snap_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".meta") {
                    let id = name.trim_end_matches(".meta").to_string();
                    let timestamp = entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .map(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64
                        })
                        .unwrap_or(0);
                    let time = if timestamp > 0 {
                        format_age(timestamp)
                    } else {
                        String::new()
                    };
                    let label = std::fs::read_to_string(entry.path())
                        .unwrap_or_else(|_| id.clone())
                        .trim()
                        .to_string();
                    self.snapshot_view.snapshots.push(SnapshotEntry {
                        id: id.clone(),
                        name: label,
                        time,
                        timestamp,
                    });
                }
            }
        }
        self.snapshot_view
            .snapshots
            .sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.snapshot_view.idx = 0;
    }

    pub fn snapshot_move_up(&mut self) {
        if self.snapshot_view.idx > 0 {
            self.snapshot_view.idx -= 1;
        }
    }

    pub fn snapshot_move_down(&mut self) {
        let len = if self.snapshot_view.filtered.is_empty()
            && self.snapshot_view.search_query.is_empty()
        {
            self.snapshot_view.snapshots.len()
        } else {
            self.snapshot_view.filtered.len()
        };
        if self.snapshot_view.idx + 1 < len {
            self.snapshot_view.idx += 1;
        }
    }

    // ── Sync helpers ─────────────────────────────────────────────────────────
}
