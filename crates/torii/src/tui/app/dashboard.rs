//! Dashboard view state.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub struct DashboardState {
    pub selected_panel: Panel,
    pub staged_idx: usize,
    pub unstaged_idx: usize,
    pub untracked_idx: usize,
    pub log_idx: usize,
    /// The files view showing HEAD's tree instead of status columns.
    pub browse: bool,
    pub browse_paths: Vec<String>,
    pub browse_rows: Vec<BrowseRow>,
    pub browse_idx: usize,
    pub browse_error: Option<String>,
    /// Folder paths whose children are hidden.
    pub browse_collapsed: BTreeSet<String>,
}

/// One visible row of the HEAD tree: a folder or a file, already indented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseRow {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub last: bool,
    /// For each ancestor, whether a sibling follows that ancestor —
    /// so the prefix can draw `│` or a blank column.
    pub continues: Vec<bool>,
}

impl BrowseRow {
    pub fn prefix(&self) -> String {
        let mut s = String::new();
        for &cont in &self.continues {
            s.push_str(if cont { "│   " } else { "    " });
        }
        s.push_str(if self.last {
            "└── "
        } else {
            "├── "
        });
        s
    }
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            selected_panel: Panel::Unstaged,
            staged_idx: 0,
            unstaged_idx: 0,
            untracked_idx: 0,
            log_idx: 0,
            browse: false,
            browse_paths: Vec::new(),
            browse_rows: Vec::new(),
            browse_idx: 0,
            browse_error: None,
            browse_collapsed: BTreeSet::new(),
        }
    }
}

#[derive(Default)]
struct Dir {
    children: BTreeMap<String, Entry>,
}

enum Entry {
    File,
    Dir(Dir),
}

fn insert(dir: &mut Dir, parts: &[&str]) {
    match parts {
        [] => {}
        [name] => {
            dir.children
                .entry((*name).to_string())
                .or_insert(Entry::File);
        }
        [name, rest @ ..] => {
            let entry = dir
                .children
                .entry((*name).to_string())
                .or_insert_with(|| Entry::Dir(Dir::default()));
            match entry {
                Entry::Dir(child) => insert(child, rest),
                Entry::File => {
                    *entry = Entry::Dir(Dir::default());
                    if let Entry::Dir(child) = entry {
                        insert(child, rest);
                    }
                }
            }
        }
    }
}

fn flatten(
    dir: &Dir,
    prefix: &str,
    continues: &[bool],
    collapsed: &BTreeSet<String>,
    out: &mut Vec<BrowseRow>,
) {
    // Folders first, then files; each group keeps BTreeMap's byte order.
    let ordered: Vec<_> = dir
        .children
        .iter()
        .filter(|(_, e)| matches!(e, Entry::Dir(_)))
        .chain(
            dir.children
                .iter()
                .filter(|(_, e)| matches!(e, Entry::File)),
        )
        .collect();
    let n = ordered.len();
    for (i, (name, entry)) in ordered.into_iter().enumerate() {
        let last = i + 1 == n;
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let is_dir = matches!(entry, Entry::Dir(_));
        out.push(BrowseRow {
            path: path.clone(),
            name: name.clone(),
            is_dir,
            last,
            continues: continues.to_vec(),
        });
        if let Entry::Dir(child) = entry {
            if !collapsed.contains(&path) {
                let mut next = continues.to_vec();
                next.push(!last);
                flatten(child, &path, &next, collapsed, out);
            }
        }
    }
}

/// Fold HEAD blob paths into the rows a tree widget walks.
pub(crate) fn tree_rows(paths: &[String], collapsed: &BTreeSet<String>) -> Vec<BrowseRow> {
    let mut root = Dir::default();
    for path in paths {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        insert(&mut root, &parts);
    }
    let mut out = Vec::new();
    flatten(&root, "", &[], collapsed, &mut out);
    out
}

/// Every folder implied by the blob paths — the tree opens with these folded.
pub(crate) fn folder_paths(paths: &[String]) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();
    for path in paths {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() < 2 {
            continue;
        }
        let mut acc = String::new();
        for part in &parts[..parts.len() - 1] {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            dirs.insert(acc.clone());
        }
    }
    dirs
}

impl App {
    /// Swap the files view between status columns and the HEAD tree.
    /// The same key leaves the tree, so the layout you came from is
    /// the one you return to.
    pub fn toggle_files_browse(&mut self) {
        if self.dashboard.browse {
            self.leave_files_browse();
        } else {
            self.enter_files_browse();
        }
    }

    pub fn enter_files_browse(&mut self) {
        self.dashboard.browse_error = None;
        self.dashboard.browse_idx = 0;
        match crate::core::GitRepo::open(&self.repo_path).and_then(|r| r.list_head_paths()) {
            Ok(paths) => {
                self.dashboard.browse_paths = paths;
                self.dashboard.browse_collapsed = folder_paths(&self.dashboard.browse_paths);
                self.rebuild_browse_rows();
                self.dashboard.browse = true;
                self.sidebar_focused = false;
            }
            Err(e) => {
                self.dashboard.browse_paths.clear();
                self.dashboard.browse_rows.clear();
                self.dashboard.browse_error = Some(e.to_string());
                self.dashboard.browse = true;
                self.sidebar_focused = false;
            }
        }
    }

    pub fn leave_files_browse(&mut self) {
        self.dashboard.browse = false;
        self.dashboard.browse_error = None;
        self.dashboard.browse_paths.clear();
        self.dashboard.browse_rows.clear();
        self.dashboard.browse_collapsed.clear();
    }

    pub(crate) fn rebuild_browse_rows(&mut self) {
        self.dashboard.browse_rows = tree_rows(
            &self.dashboard.browse_paths,
            &self.dashboard.browse_collapsed,
        );
        let len = self.dashboard.browse_rows.len();
        if len == 0 {
            self.dashboard.browse_idx = 0;
        } else if self.dashboard.browse_idx >= len {
            self.dashboard.browse_idx = len - 1;
        }
    }

    pub fn files_browse_move_up(&mut self) {
        if self.dashboard.browse_idx > 0 {
            self.dashboard.browse_idx -= 1;
        }
    }

    pub fn files_browse_move_down(&mut self) {
        let len = self.dashboard.browse_rows.len();
        if self.dashboard.browse_idx + 1 < len {
            self.dashboard.browse_idx += 1;
        }
    }

    pub fn files_browse_toggle_fold(&mut self) {
        let Some(row) = self.dashboard.browse_rows.get(self.dashboard.browse_idx) else {
            return;
        };
        if !row.is_dir {
            return;
        }
        let path = row.path.clone();
        if !self.dashboard.browse_collapsed.remove(&path) {
            self.dashboard.browse_collapsed.insert(path);
        }
        self.rebuild_browse_rows();
    }

    /// `h` / Left / `-`: collapse an open folder, or jump to the parent.
    pub fn files_browse_fold(&mut self) {
        let Some(row) = self.dashboard.browse_rows.get(self.dashboard.browse_idx) else {
            return;
        };
        let path = row.path.clone();
        let is_dir = row.is_dir;
        if is_dir && !self.dashboard.browse_collapsed.contains(&path) {
            self.dashboard.browse_collapsed.insert(path);
            self.rebuild_browse_rows();
            return;
        }
        if let Some(slash) = path.rfind('/') {
            let parent = &path[..slash];
            if let Some(i) = self
                .dashboard
                .browse_rows
                .iter()
                .position(|r| r.path == parent)
            {
                self.dashboard.browse_idx = i;
            }
        }
    }

    /// `l` / Right / `+`: unfold a collapsed folder.
    pub fn files_browse_unfold(&mut self) {
        let Some(row) = self.dashboard.browse_rows.get(self.dashboard.browse_idx) else {
            return;
        };
        if row.is_dir && self.dashboard.browse_collapsed.contains(&row.path) {
            let path = row.path.clone();
            self.dashboard.browse_collapsed.remove(&path);
            self.rebuild_browse_rows();
        }
    }
}

#[cfg(test)]
mod tree_rows_tests {
    use super::*;
    use std::collections::BTreeSet;

    fn names(rows: &[BrowseRow]) -> Vec<&str> {
        rows.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn folders_come_before_files() {
        let paths = [
            "README.md".into(),
            "src/lib.rs".into(),
            "src/main.rs".into(),
            "crates/torii/Cargo.toml".into(),
        ];
        let rows = tree_rows(&paths, &BTreeSet::new());
        assert_eq!(
            names(&rows),
            [
                "crates",
                "torii",
                "Cargo.toml",
                "src",
                "lib.rs",
                "main.rs",
                "README.md",
            ]
        );
        assert!(rows[0].is_dir && !rows[0].last);
        assert_eq!(rows[0].path, "crates");
        assert_eq!(rows[1].path, "crates/torii");
        assert!(rows[3].is_dir && !rows[3].last);
        assert_eq!(rows[3].prefix(), "├── ");
        assert_eq!(rows[4].prefix(), "│   ├── ");
        assert_eq!(rows[5].prefix(), "│   └── ");
        assert!(!rows[6].is_dir && rows[6].last);
        assert_eq!(rows[6].prefix(), "└── ");
    }

    #[test]
    fn collapsing_a_folder_hides_its_children() {
        let paths = ["src/a.rs".into(), "src/b.rs".into(), "z.md".into()];
        let mut collapsed = BTreeSet::new();
        collapsed.insert("src".into());
        let rows = tree_rows(&paths, &collapsed);
        assert_eq!(names(&rows), ["src", "z.md"]);
        assert!(rows[0].is_dir);
    }

    #[test]
    fn empty_paths_are_an_empty_tree() {
        assert!(tree_rows(&[], &BTreeSet::new()).is_empty());
    }

    #[test]
    fn folders_start_collapsed() {
        let paths = [
            "README.md".into(),
            "src/lib.rs".into(),
            "crates/torii/Cargo.toml".into(),
        ];
        let rows = tree_rows(&paths, &folder_paths(&paths));
        assert_eq!(names(&rows), ["crates", "src", "README.md"]);
    }
}

// ── Diff state ───────────────────────────────────────────────────────────────
