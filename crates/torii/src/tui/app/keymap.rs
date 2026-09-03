//! The keymap as the app sees it: what is bound, what is half-pressed, and
//! the palette that keeps every action reachable without a binding.

use super::*;
use crate::tui::keys::{self, Chord, Resolution};

/// The action palette — a list of every action, filtered by what is typed.
#[derive(Clone, Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub idx: usize,
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
            self.palette.open = true;
            self.palette.query.clear();
            self.palette.idx = 0;
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
            "app:palette" => {
                self.palette.open = true;
                self.palette.query.clear();
                self.palette.idx = 0;
            }
            "repo:scan" => self.run_secret_scan(false),
            "repo:scan-history" => self.run_secret_scan(true),
            other => self.log_event(
                format!("no such action: `{other}` — check ~/.torii/keys.toml"),
                EventKind::Error,
            ),
        }
    }

    // ── Palette ──────────────────────────────────────────────────────────────

    /// Actions matching what has been typed, in catalogue order.
    pub fn palette_matches(&self) -> Vec<&'static keys::ActionDef> {
        let q = self.palette.query.to_lowercase();
        keys::ACTIONS
            .iter()
            .filter(|a| {
                q.is_empty()
                    || a.label.to_lowercase().contains(&q)
                    || a.id.to_lowercase().contains(&q)
            })
            .collect()
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
        let Some(action) = self
            .palette_matches()
            .get(self.palette.idx)
            .map(|a| a.id.to_string())
        else {
            return;
        };
        self.palette.open = false;
        self.palette.query.clear();
        match self.palette.capturing_for.take() {
            Some(_) => {
                // Picked from the config screen: leave it selected there.
                self.config_view.pending_action = Some(action);
            }
            None => self.run_action(&action),
        }
    }

    pub fn palette_close(&mut self) {
        self.palette.open = false;
        self.palette.query.clear();
        self.palette.capturing_for = None;
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
