//! Safety view state — everything that keeps a secret out of the repo.
//!
//! Two things that were living apart and belong together: the `.toriignore`
//! pair, and the scanner that reads it. A deny pattern is only meaningful
//! next to the scanner it feeds, and the scanner's own settings — the size
//! gate, the hooks — live in the same two files.
//!
//! The rules are read with their provenance (see `cmd::ignore_rules`), so the
//! view can say which file each one lives in and can remove exactly the line
//! it showed. `.toriignore` is committed and public; `.toriignore.local` is
//! private to this machine, which is where a rule describing the shape of
//! your secrets belongs.

use super::*;
use crate::ignore_rules::{Kind, Origin, Rule};

/// Which half of the screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SafetyTab {
    /// The `.toriignore` rules, editable.
    #[default]
    Rules,
    /// What the scanner is made of and what it enforces.
    Scanner,
}

/// What the view is doing: reading, typing a new rule, or confirming a delete.
#[derive(Debug, Clone, PartialEq)]
pub enum IgnoreFocus {
    List,
    /// Typing the pattern of a new rule.
    Input,
    ConfirmDelete,
}

#[derive(Clone)]
pub struct IgnoreState {
    pub tab: SafetyTab,
    pub rules: Vec<Rule>,
    /// The scanner's own settings, read from the same two files.
    pub size: crate::toriignore::SizeRules,
    pub hooks: crate::toriignore::HookRules,
    /// Row of the scanner tab.
    pub scanner_idx: usize,
    pub idx: usize,
    pub focus: IgnoreFocus,
    /// What a new rule would be: a path or a secret.
    pub new_kind: Kind,
    /// Which file a new rule would be written to.
    pub new_origin: Origin,
    pub input: String,
    pub cursor: usize,
    pub status: Option<String>,
}

impl Default for IgnoreState {
    fn default() -> Self {
        Self {
            tab: SafetyTab::default(),
            rules: Vec::new(),
            size: Default::default(),
            hooks: Default::default(),
            scanner_idx: 0,
            idx: 0,
            focus: IgnoreFocus::List,
            new_kind: Kind::Path,
            // A path rule is ordinary and public; the moment the kind flips to
            // Secret the target follows it to the private file (see
            // `ignore_toggle_kind`), which is the default `torii ignore
            // secret` uses.
            new_origin: Origin::Public,
            input: String::new(),
            cursor: 0,
            status: None,
        }
    }
}

impl App {
    /// Everything the safety screen shows: the rules, and what the scanner
    /// enforces beyond them.
    pub fn load_safety(&mut self) {
        self.load_ignore_rules();
        // The size gate and the hooks come from the merged pair, because that
        // is what the scanner itself reads.
        match crate::toriignore::ToriIgnore::load(std::path::Path::new(&self.repo_path)) {
            Ok(ti) => {
                self.ignore_view.size = ti.size;
                self.ignore_view.hooks = ti.hooks;
            }
            Err(e) => {
                self.ignore_view.status = Some(format!("could not read the rules: {e}"));
            }
        }
    }

    pub fn safety_toggle_tab(&mut self) {
        self.ignore_view.tab = match self.ignore_view.tab {
            SafetyTab::Rules => SafetyTab::Scanner,
            SafetyTab::Scanner => SafetyTab::Rules,
        };
    }

    pub fn load_ignore_rules(&mut self) {
        let root = std::path::PathBuf::from(&self.repo_path);
        match crate::ignore_rules::load(&root) {
            Ok(mut rules) => {
                // Read file by file, the two files interleave their sections
                // and the view would print "path", "secret", "path", "secret".
                // Sorting by kind is stable, so file order survives inside a
                // group and a rule's line number still means something.
                rules.sort_by_key(|r| match r.kind {
                    Kind::Path => 0,
                    Kind::Secret => 1,
                    Kind::Size => 2,
                    Kind::Hook => 3,
                });
                self.ignore_view.rules = rules;
                let last = self.ignore_view.rules.len().saturating_sub(1);
                self.ignore_view.idx = self.ignore_view.idx.min(last);
            }
            Err(e) => {
                self.ignore_view.rules.clear();
                self.ignore_view.status = Some(format!("could not read the rules: {e}"));
            }
        }
    }

    pub fn ignore_selected(&self) -> Option<&Rule> {
        self.ignore_view.rules.get(self.ignore_view.idx)
    }

    /// Flip what a new rule would be. A secret pattern describes the shape of
    /// your credentials, so choosing it also moves the target to the private
    /// file; the user can still send it public deliberately.
    pub fn ignore_toggle_kind(&mut self) {
        let (kind, origin) = match self.ignore_view.new_kind {
            Kind::Secret => (Kind::Path, Origin::Public),
            _ => (Kind::Secret, Origin::Local),
        };
        self.ignore_view.new_kind = kind;
        self.ignore_view.new_origin = origin;
    }

    pub fn ignore_toggle_origin(&mut self) {
        self.ignore_view.new_origin = match self.ignore_view.new_origin {
            Origin::Public => Origin::Local,
            Origin::Local => Origin::Public,
        };
    }

    /// Write what was typed, then reload so the new rule appears with the line
    /// number it actually got.
    pub fn ignore_commit_input(&mut self) {
        let root = std::path::PathBuf::from(&self.repo_path);
        let pattern = self.ignore_view.input.trim().to_string();
        let origin = self.ignore_view.new_origin;

        let result = match self.ignore_view.new_kind {
            Kind::Secret => crate::ignore_rules::add_secret(&root, &pattern, None, origin),
            _ => crate::ignore_rules::add_path(&root, &pattern, origin),
        };

        match result {
            Ok(()) => {
                self.ignore_view.input.clear();
                self.ignore_view.cursor = 0;
                self.ignore_view.focus = IgnoreFocus::List;
                self.load_ignore_rules();
                // Land on what was just written.
                if let Some(pos) = self
                    .ignore_view
                    .rules
                    .iter()
                    .position(|r| r.pattern == pattern && r.origin == origin)
                {
                    self.ignore_view.idx = pos;
                }
                self.log_event(
                    format!("ignore: added `{pattern}` to {}", origin.file_name()),
                    EventKind::Success,
                );
            }
            Err(e) => {
                // The regex was rejected, or the file could not be written:
                // keep what was typed so it can be corrected.
                self.ignore_view.status = Some(format!("{e}"));
                self.log_event(format!("ignore: {e}"), EventKind::Error);
            }
        }
    }

    pub fn ignore_delete_selected(&mut self) {
        let root = std::path::PathBuf::from(&self.repo_path);
        let Some(rule) = self.ignore_selected().cloned() else {
            self.ignore_view.focus = IgnoreFocus::List;
            return;
        };
        match crate::ignore_rules::remove(&root, &rule) {
            Ok(()) => {
                self.log_event(
                    format!(
                        "ignore: removed `{}` from {}",
                        rule.pattern,
                        rule.origin.file_name()
                    ),
                    EventKind::Success,
                );
                self.load_ignore_rules();
                let last = self.ignore_view.rules.len().saturating_sub(1);
                self.ignore_view.idx = self.ignore_view.idx.min(last);
            }
            Err(e) => {
                self.ignore_view.status = Some(format!("{e}"));
                self.log_event(format!("ignore: {e}"), EventKind::Error);
            }
        }
        self.ignore_view.focus = IgnoreFocus::List;
    }

    pub fn ignore_move_up(&mut self) {
        self.ignore_view.idx = self.ignore_view.idx.saturating_sub(1);
    }

    pub fn ignore_move_down(&mut self) {
        let last = self.ignore_view.rules.len().saturating_sub(1);
        self.ignore_view.idx = (self.ignore_view.idx + 1).min(last);
    }
}
