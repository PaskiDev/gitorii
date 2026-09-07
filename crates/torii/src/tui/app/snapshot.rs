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
    /// Typing a duration for the custom auto-snapshot row.
    AutoCustom,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoSnapshotInterval {
    Off,
    Min5,
    Min15,
    Min30,
    Hour1,
    /// Minutes, always ≥ 1 when saved. `Custom(0)` is the picker
    /// placeholder for "type one", never written to config.
    Custom(u32),
}

impl AutoSnapshotInterval {
    pub fn label(&self) -> String {
        match self {
            Self::Off => "off".to_string(),
            Self::Min5 => "every 5 min".to_string(),
            Self::Min15 => "every 15 min".to_string(),
            Self::Min30 => "every 30 min".to_string(),
            Self::Hour1 => "every 1 hour".to_string(),
            Self::Custom(0) => "custom…".to_string(),
            Self::Custom(m) => format!("every {}", crate::duration::format_duration(*m)),
        }
    }
    pub fn minutes(&self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Min5 => Some(5),
            Self::Min15 => Some(15),
            Self::Min30 => Some(30),
            Self::Hour1 => Some(60),
            Self::Custom(0) => None,
            Self::Custom(m) => Some(*m),
        }
    }
    pub fn secs(&self) -> Option<u64> {
        self.minutes().map(|m| m as u64 * 60)
    }
    pub fn from_minutes(m: u32) -> Self {
        match m {
            5 => Self::Min5,
            15 => Self::Min15,
            30 => Self::Min30,
            60 => Self::Hour1,
            n if n >= 1 => Self::Custom(n),
            _ => Self::Off,
        }
    }
    /// Presets plus a custom row. The custom row carries the current
    /// custom minutes so the checkmark lands on it.
    pub fn picker_rows(current: &Self) -> Vec<Self> {
        vec![
            Self::Off,
            Self::Min5,
            Self::Min15,
            Self::Min30,
            Self::Hour1,
            match current {
                Self::Custom(m) if *m > 0 => Self::Custom(*m),
                _ => Self::Custom(0),
            },
        ]
    }
    pub fn input_hint(minutes: u32) -> String {
        if minutes == 0 {
            String::new()
        } else if minutes % (60 * 24) == 0 {
            format!("{}d", minutes / (60 * 24))
        } else if minutes % 60 == 0 {
            format!("{}h", minutes / 60)
        } else {
            format!("{minutes}m")
        }
    }
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom(_))
    }
}

pub struct SnapshotState {
    pub snapshots: Vec<SnapshotEntry>,
    pub idx: usize,
    pub focus: SnapshotFocus,
    pub create_name: String,
    pub auto_interval: AutoSnapshotInterval,
    pub auto_interval_idx: usize,
    pub auto_custom_input: String,
    pub auto_custom_error: Option<String>,
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
            auto_custom_input: String::new(),
            auto_custom_error: None,
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
        self.snapshot_view.snapshots.clear();
        let snap_dir = std::path::Path::new(&self.repo_path).join(".git/torii/snapshots");
        if !snap_dir.exists() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(&snap_dir) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let meta_path = entry.path().join("metadata.json");
                let Ok(raw) = std::fs::read_to_string(&meta_path) else {
                    continue;
                };
                let Ok(meta) = serde_json::from_str::<crate::snapshot::SnapshotMetadata>(&raw)
                else {
                    continue;
                };
                let timestamp = meta.timestamp.timestamp();
                let name = meta.name.unwrap_or_else(|| meta.id.clone());
                self.snapshot_view.snapshots.push(SnapshotEntry {
                    id: meta.id,
                    name,
                    time: format_age(timestamp),
                    timestamp,
                });
            }
        }
        self.snapshot_view
            .snapshots
            .sort_by_key(|s| std::cmp::Reverse(s.timestamp));
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

    pub fn selected_snapshot(&self) -> Option<&SnapshotEntry> {
        let i = if self.snapshot_view.filtered.is_empty()
            && self.snapshot_view.search_query.is_empty()
        {
            self.snapshot_view.idx
        } else {
            *self.snapshot_view.filtered.get(self.snapshot_view.idx)?
        };
        self.snapshot_view.snapshots.get(i)
    }

    pub fn go_to_diff_from_snapshot(&mut self) {
        let Some(snap) = self.selected_snapshot() else {
            return;
        };
        let id = snap.id.clone();
        let name = snap.name.clone();
        match crate::snapshot::SnapshotManager::new(&self.repo_path)
            .and_then(|m| m.diff_files_against_current(&id))
        {
            Ok(files) => {
                self.prev_view = Some(self.view.clone());
                self.diff.title = format!("{name} → now");
                self.diff.snapshot_files = files;
                let paths: Vec<String> = self
                    .diff
                    .snapshot_files
                    .iter()
                    .map(|f| f.path.clone())
                    .collect();
                self.diff.tree_collapsed = folder_paths(&paths);
                self.diff.tree_idx = 0;
                self.diff.tree_rows = tree_rows(&paths, &self.diff.tree_collapsed);
                self.diff.lines.clear();
                self.diff.scroll = 0;
                self.diff.mode = DiffMode::SnapshotTree;
                self.view = View::Diff;
            }
            Err(e) => self.set_status(format!("diff failed: {e}")),
        }
    }

    // ── Sync helpers ─────────────────────────────────────────────────────────
}

#[cfg(test)]
mod interval_tests {
    use super::AutoSnapshotInterval;

    #[test]
    fn from_minutes_keeps_values_the_presets_do_not_cover() {
        assert_eq!(
            AutoSnapshotInterval::from_minutes(5),
            AutoSnapshotInterval::Min5
        );
        assert_eq!(
            AutoSnapshotInterval::from_minutes(60),
            AutoSnapshotInterval::Hour1
        );
        assert_eq!(
            AutoSnapshotInterval::from_minutes(120),
            AutoSnapshotInterval::Custom(120)
        );
        assert_eq!(
            AutoSnapshotInterval::from_minutes(45),
            AutoSnapshotInterval::Custom(45)
        );
        assert_eq!(AutoSnapshotInterval::from_minutes(120).secs(), Some(7200));
    }

    #[test]
    fn picker_rows_surface_the_current_custom() {
        let rows = AutoSnapshotInterval::picker_rows(&AutoSnapshotInterval::Custom(90));
        assert!(rows.contains(&AutoSnapshotInterval::Custom(90)));
        assert!(!rows.contains(&AutoSnapshotInterval::Custom(0)));
        let rows = AutoSnapshotInterval::picker_rows(&AutoSnapshotInterval::Off);
        assert_eq!(rows.last(), Some(&AutoSnapshotInterval::Custom(0)));
    }
}
