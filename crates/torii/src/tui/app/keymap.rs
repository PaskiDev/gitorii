//! The keymap as the app sees it: what is bound, what is half-pressed, and
//! the palette that keeps every action reachable without a binding.

use super::*;
use crate::tui::keys::{self, Chord, Resolution};

/// What the overlay is listing.
///
/// One widget, three sources: the same filter-and-pick gesture works for
/// running an action, checking out a branch and moving to another repo of the
/// workspace, which is the point — the switch is the same muscle everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteMode {
    #[default]
    Actions,
    Branches,
    Repos,
}

/// One row of the overlay.
#[derive(Clone)]
pub struct PaletteItem {
    /// What gets acted on: an action id, a branch name, a repo path.
    pub id: String,
    pub label: String,
    /// The dim text on the right: a binding, a marker, a path.
    pub hint: String,
}

/// The overlay — a list filtered by what is typed.
#[derive(Clone, Default)]
pub struct PaletteState {
    pub open: bool,
    pub mode: PaletteMode,
    pub query: String,
    pub idx: usize,
    /// Rows for the branch and repo modes, taken when the overlay opens: the
    /// list must not shift under the cursor while it is being filtered.
    pub items: Vec<PaletteItem>,
    /// Set while the config screen is waiting for a binding to be pressed.
    pub capturing_for: Option<String>,
}

impl App {
    /// Whether the focused thing is a text field.
    ///
    /// This is the rule that keeps a bound letter from ending up in a commit
    /// message: while it holds, only chords carrying ctrl or alt resolve. Miss
    /// a mode here and typing in that view starts running commands, so a new
    /// text mode must be added to this list.
    pub fn is_typing(&self) -> bool {
        use crate::tui::app::*;
        match self.view {
            View::Commit => self.commit_view.focus == CommitFocus::Input,
            View::Log | View::History => self.log.search_mode,
            View::Snapshot => {
                self.snapshot_view.search_mode || self.snapshot_view.focus == SnapshotFocus::Create
            }
            View::Ignore => self.ignore_view.focus == IgnoreFocus::Input,
            View::Config | View::Settings => self.config_view.editing,
            View::Branch => self.branch_view.confirm == BranchConfirm::NewBranch,
            View::Issue => !matches!(self.issue_view.confirm, IssueConfirm::None),
            View::Remote => self.remote_view.confirm == RemoteConfirm::EditUrl,
            View::Auth => self.auth_view.focus == AuthFocus::InputToken,
            View::Worktree => self.worktree_view.focus == WorktreeFocus::InputArgs,
            View::Submodule => self.submodule_view.focus == SubmoduleFocus::InputArgs,
            View::Bisect => {
                self.bisect_view.focus == BisectFocus::InputArgs
                    || self.bisect_view.focus == BisectFocus::RefPicker
            }
            _ => false,
        }
    }

    /// Feed one keypress to the keymap. `true` means the key was consumed and
    /// the view must not see it.
    pub fn keymap_consume(&mut self, chord: Chord) -> bool {
        // The palette's own key wins everywhere, including inside a text
        // field: it is the way out when a binding is wrong or forgotten.
        if self.pending_chords.is_empty() && self.keymap.leader.0 == vec![chord] {
            self.open_switcher(PaletteMode::Actions);
            return true;
        }

        let typing = self.is_typing();
        match self.keymap.resolve(&self.pending_chords, chord, typing) {
            Resolution::Pending => {
                self.pending_chords.push(chord);
                true
            }
            Resolution::Fire(action) => {
                self.pending_chords.clear();
                self.run_action(&action);
                true
            }
            Resolution::Unbound => {
                // A sequence that went nowhere swallows the stray key rather
                // than letting `g` `z` reach the view as a bare `z`.
                let was_pending = !self.pending_chords.is_empty();
                self.pending_chords.clear();
                was_pending
            }
        }
    }

    /// Run an action by id. Unknown ids are reported rather than ignored: they
    /// come from a hand-edited file, and silence would look like a dead key.
    pub fn run_action(&mut self, id: &str) {
        if let Some(view) = view_for_action(id) {
            self.go_to(view);
            self.sidebar_focused = false;
            return;
        }
        match id {
            "app:events" => self.show_event_log = !self.show_event_log,
            "app:help" => self.go_to(View::Help),
            "app:back" => self.go_back(),
            "app:refresh" => {
                if let Err(e) = self.refresh() {
                    self.log_event(format!("refresh failed: {e}"), EventKind::Error);
                }
            }
            "app:quit" => self.should_quit = true,
            "app:palette" => self.open_switcher(PaletteMode::Actions),
            "repo:scan" => self.run_secret_scan(false),
            "repo:scan-history" => self.run_secret_scan(true),
            "repo:switch-branch" => self.open_switcher(PaletteMode::Branches),
            "repo:switch-repo" => self.open_switcher(PaletteMode::Repos),
            other => self.log_event(
                format!("no such action: `{other}` — check ~/.torii/keys.toml"),
                EventKind::Error,
            ),
        }
    }

    // ── Palette and switchers ────────────────────────────────────────────────

    /// Open the overlay on one of its three lists.
    pub fn open_switcher(&mut self, mode: PaletteMode) {
        self.palette.mode = mode;
        self.palette.query.clear();
        self.palette.idx = 0;
        self.palette.items.clear();

        match mode {
            PaletteMode::Actions => {}
            PaletteMode::Branches => {
                self.load_branches();
                self.palette.items = self
                    .branch_view
                    .branches
                    .iter()
                    // A remote branch is a checkout of a different kind and
                    // needs the branch view's own flow; this is the fast path
                    // between branches that already exist here.
                    .filter(|b| !b.is_remote)
                    .map(|b| PaletteItem {
                        id: b.name.clone(),
                        label: b.name.clone(),
                        hint: if b.is_current {
                            "current".into()
                        } else {
                            String::new()
                        },
                    })
                    .collect();
                if self.palette.items.is_empty() {
                    self.log_event("no local branches to switch to", EventKind::Info);
                    return;
                }
            }
            PaletteMode::Repos => {
                let paths = self.workspace_repo_paths();
                if paths.len() <= 1 {
                    self.log_event(
                        "no workspace open — `torii workspace add <name> <path>`",
                        EventKind::Info,
                    );
                    return;
                }
                let current = std::fs::canonicalize(&self.repo_path).ok();
                self.palette.items = paths
                    .iter()
                    .map(|p| PaletteItem {
                        id: p.clone(),
                        label: std::path::Path::new(p)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.clone()),
                        hint: if std::fs::canonicalize(p).ok() == current {
                            "current".into()
                        } else {
                            p.clone()
                        },
                    })
                    .collect();
            }
        }
        self.palette.open = true;
    }

    /// Check out a local branch from wherever the user happens to be.
    pub fn switch_to_branch(&mut self, name: &str) {
        let result =
            crate::core::GitRepo::open(&self.repo_path).and_then(|r| r.switch_branch(name));
        match result {
            Ok(()) => {
                self.log_event(format!("checkout: {name}"), EventKind::Success);
                self.set_status(format!("on {name}"));
                if let Err(e) = self.refresh() {
                    self.log_event(format!("refresh failed: {e}"), EventKind::Error);
                }
            }
            Err(e) => {
                // A dirty tree is the usual reason, and the message says so.
                self.log_event(format!("checkout failed: {e}"), EventKind::Error);
                self.set_status(format!("checkout failed: {e}"));
            }
        }
    }

    /// Move to another repo of the workspace, keeping the workspace itself.
    pub fn switch_to_repo(&mut self, path: &str) {
        self.repo_path = path.to_string();
        if let Err(e) = self.refresh() {
            self.log_event(format!("refresh failed: {e}"), EventKind::Error);
        }
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        self.log_event(format!("repo: {name}"), EventKind::Success);
        self.set_status(format!("repo {name}"));
    }

    // ── Palette ──────────────────────────────────────────────────────────────

    /// The rows matching what has been typed, in source order.
    pub fn palette_matches(&self) -> Vec<PaletteItem> {
        let q = self.palette.query.to_lowercase();
        let matches = |label: &str, id: &str| {
            q.is_empty() || label.to_lowercase().contains(&q) || id.to_lowercase().contains(&q)
        };
        match self.palette.mode {
            PaletteMode::Actions => keys::ACTIONS
                .iter()
                .filter(|a| matches(a.label, a.id))
                .map(|a| PaletteItem {
                    id: a.id.to_string(),
                    label: a.label.to_string(),
                    hint: self
                        .keymap
                        .binding_for(a.id)
                        .map(|b| b.to_string())
                        .unwrap_or_default(),
                })
                .collect(),
            _ => self
                .palette
                .items
                .iter()
                .filter(|i| matches(&i.label, &i.id))
                .cloned()
                .collect(),
        }
    }

    /// What the overlay calls itself.
    pub fn palette_title(&self) -> &'static str {
        match self.palette.mode {
            PaletteMode::Actions => " actions ",
            PaletteMode::Branches => " switch branch ",
            PaletteMode::Repos => " switch repo ",
        }
    }

    pub fn palette_move(&mut self, delta: isize) {
        let len = self.palette_matches().len();
        if len == 0 {
            self.palette.idx = 0;
            return;
        }
        let idx = self.palette.idx as isize + delta;
        self.palette.idx = idx.clamp(0, len as isize - 1) as usize;
    }

    /// Run what the palette has selected, or — when the config screen opened
    /// it to pick an action — take that action back to the capture flow.
    pub fn palette_accept(&mut self) {
        let Some(picked) = self.palette_matches().get(self.palette.idx).cloned() else {
            return;
        };
        let mode = self.palette.mode;
        self.palette.open = false;
        self.palette.query.clear();

        match mode {
            PaletteMode::Branches => self.switch_to_branch(&picked.id),
            PaletteMode::Repos => self.switch_to_repo(&picked.id),
            PaletteMode::Actions => match self.palette.capturing_for.take() {
                // Picked from the config screen: leave it selected there.
                Some(_) => self.config_view.pending_action = Some(picked.id),
                None => self.run_action(&picked.id),
            },
        }
    }

    pub fn palette_close(&mut self) {
        self.palette.open = false;
        self.palette.query.clear();
        self.palette.capturing_for = None;
        self.palette.mode = PaletteMode::Actions;
    }
}

/// The views an action can jump to. Kept beside the catalogue so a new view is
/// one line in each.
fn view_for_action(id: &str) -> Option<View> {
    Some(match id {
        "goto:files" => View::Dashboard,
        "goto:save" => View::Commit,
        "goto:sync" => View::Sync,
        "goto:snapshot" => View::Snapshot,
        "goto:ignore" => View::Ignore,
        "goto:log" => View::Log,
        "goto:branch" => View::Branch,
        "goto:tag" => View::Tag,
        "goto:pr" => View::Pr,
        "goto:issue" => View::Issue,
        "goto:platform" => View::Platform,
        "goto:remote" => View::Remote,
        "goto:workspace" => View::Workspace,
        "goto:worktree" => View::Worktree,
        "goto:submodule" => View::Submodule,
        "goto:bisect" => View::Bisect,
        "goto:auth" => View::Auth,
        "goto:config" => View::Config,
        _ => return None,
    })
}
