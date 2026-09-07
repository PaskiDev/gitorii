//! Built-in keys, claimed per view.
//!
//! A letter may live in many views. It may not live twice in one — the
//! second action is unreachable, which is how `e` on the files tree
//! lost to the event log. Modes of a view are separate claims: space
//! stages a file in `files` and folds a folder in `files.tree`.
//!
//! Add the claim here before the handler. Building a map with the same
//! chord twice panics, and the test builds every map.

use std::collections::HashMap;
use std::sync::OnceLock;

use crossterm::event::KeyEvent;

use super::keys::{parse_chord, Chord};

/// Keys the main loop takes before any view sees them. Claiming one
/// in a view is the same bug as claiming it twice locally.
pub const GLOBALS: &[(&str, &str)] = &[
    ("e", "events"),
    ("tab", "cycle-focus"),
    ("W", "repo-picker"),
];

/// One view's (or one mode's) keys.
#[derive(Debug)]
pub struct ViewKeys {
    pub scope: &'static str,
    map: HashMap<Chord, &'static str>,
}

impl ViewKeys {
    pub fn new(scope: &'static str) -> Self {
        Self {
            scope,
            map: HashMap::new(),
        }
    }

    /// `q` / `?` / `ctrl+c`, as `handle_global_nav` takes them.
    pub fn with_nav(self) -> Self {
        self.on("q / ctrl+c", "quit").on("?", "help")
    }

    /// `keys` is `parse_chord` tokens joined by `/`, e.g. `"j / down"`.
    pub fn on(mut self, keys: &'static str, action: &'static str) -> Self {
        for raw in keys.split('/') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let chord = parse_chord(raw).unwrap_or_else(|e| {
                panic!("{}: `{raw}` is not a key: {e}", self.scope);
            });
            if let Some(prev) = self.map.insert(chord, action) {
                panic!("{}: `{raw}` is both `{prev}` and `{action}`", self.scope);
            }
        }
        self
    }

    pub fn action(&self, key: KeyEvent) -> Option<&'static str> {
        self.map.get(&Chord::from_event(key)).copied()
    }

    pub fn action_for(&self, chord: Chord) -> Option<&'static str> {
        self.map.get(&chord).copied()
    }

    pub fn chords(&self) -> impl Iterator<Item = (Chord, &'static str)> + '_ {
        self.map.iter().map(|(c, a)| (*c, *a))
    }
}

pub fn files() -> &'static ViewKeys {
    static M: OnceLock<ViewKeys> = OnceLock::new();
    M.get_or_init(files_map)
}

pub fn files_tree() -> &'static ViewKeys {
    static M: OnceLock<ViewKeys> = OnceLock::new();
    M.get_or_init(files_tree_map)
}

fn files_map() -> ViewKeys {
    ViewKeys::new("files")
        .with_nav()
        .on("t", "tree")
        .on("backtab", "prev-panel")
        .on("up / k", "up")
        .on("down / j", "down")
        .on("ctrl+r", "refresh")
        .on("space", "stage")
        .on("d", "diff")
}

fn files_tree_map() -> ViewKeys {
    ViewKeys::new("files.tree")
        .with_nav()
        .on("t / esc", "leave")
        .on("up / k", "up")
        .on("down / j", "down")
        .on("enter / space", "toggle-fold")
        .on("left / h / -", "fold")
        .on("right / l / + / =", "unfold")
        .on("ctrl+r", "refresh")
}

fn clone_map() -> ViewKeys {
    ViewKeys::new("clone")
        .with_nav()
        .on("up / k", "up")
        .on("down / j", "down")
        .on("p", "host")
        .on("/", "filter")
        .on("u", "url")
        .on("d", "dest")
        .on("t", "protocol")
        .on("r", "reload")
        .on("enter", "clone")
}

fn log_map() -> ViewKeys {
    ViewKeys::new("log")
        .with_nav()
        .on("up / k", "up")
        .on("down / j", "down")
        .on("o", "ops")
        .on("space", "pick")
        .on("/", "search")
        .on("A", "authors")
        .on("G", "signatures")
        .on("S", "armor")
}

fn branch_map() -> ViewKeys {
    ViewKeys::new("branch")
        .with_nav()
        .on("up / k", "up")
        .on("down / j", "down")
        .on("o", "ops")
        .on("/", "search")
}

fn tag_map() -> ViewKeys {
    ViewKeys::new("tag")
        .with_nav()
        .on("up / k", "up")
        .on("down / j", "down")
        .on("o", "ops")
        .on("/", "search")
}

fn commit_map() -> ViewKeys {
    ViewKeys::new("commit")
        .with_nav()
        .on("enter", "type")
        .on("i", "message")
        .on("a", "amend")
}

fn snapshot_map() -> ViewKeys {
    ViewKeys::new("snapshot")
        .with_nav()
        .on("up / k", "up")
        .on("down / j", "down")
        .on("o", "ops")
        .on("/", "search")
        .on("n", "create")
        .on("a", "auto")
        .on("d", "diff")
}

fn snapshot_auto_custom_map() -> ViewKeys {
    ViewKeys::new("snapshot.auto-custom")
        .on("enter", "set")
        .on("esc", "back")
        .on("ctrl+c", "quit")
}

fn sync_map() -> ViewKeys {
    ViewKeys::new("sync")
        .with_nav()
        .on("up / k", "up")
        .on("down / j", "down")
        .on("enter", "run")
}

fn diff_map() -> ViewKeys {
    ViewKeys::new("diff")
        .on("esc / q", "back")
        .on("up / k", "up")
        .on("down / j", "down")
        .on("pageup", "page-up")
        .on("pagedown", "page-down")
        .on("ctrl+c", "quit")
}

fn diff_snapshot_tree_map() -> ViewKeys {
    ViewKeys::new("diff.snapshot-tree")
        .on("esc / q", "back")
        .on("up / k", "up")
        .on("down / j", "down")
        .on("enter / space", "open")
        .on("left / h / -", "fold")
        .on("right / l / + / =", "unfold")
        .on("ctrl+c", "quit")
}

fn help_map() -> ViewKeys {
    ViewKeys::new("help")
        .on("esc / ? / q", "close")
        .on("ctrl+c", "quit")
}

fn issue_map() -> ViewKeys {
    ViewKeys::new("issue")
        .with_nav()
        .on("up / k", "up")
        .on("down / j", "down")
        .on("o", "ops")
        .on("p", "platform")
        .on("ctrl+r", "refresh")
}

fn stats_map() -> ViewKeys {
    ViewKeys::new("stats")
        .with_nav()
        .on("1", "repo")
        .on("2", "workspace")
        .on("3", "people")
        .on("up / k", "up")
        .on("down / j", "down")
        .on("r", "reload")
}

/// Every idle map. The test builds this; a duplicate panics here.
pub fn all() -> Vec<ViewKeys> {
    vec![
        files_map(),
        files_tree_map(),
        clone_map(),
        log_map(),
        branch_map(),
        tag_map(),
        commit_map(),
        snapshot_map(),
        snapshot_auto_custom_map(),
        sync_map(),
        diff_map(),
        diff_snapshot_tree_map(),
        help_map(),
        stats_map(),
        issue_map(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_view_builds() {
        let scopes: Vec<&str> = all().iter().map(|v| v.scope).collect();
        let mut unique = scopes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(scopes.len(), unique.len(), "duplicate scope name");
    }

    #[test]
    fn a_second_claim_on_the_same_key_is_refused() {
        let boom = std::panic::catch_unwind(|| {
            let _ = ViewKeys::new("test").on("j", "down").on("j", "also");
        });
        assert!(boom.is_err(), "a duplicate claim must panic");
    }

    #[test]
    fn aliases_of_one_action_are_fine() {
        let keys = ViewKeys::new("test").on("j / down", "down");
        assert_eq!(keys.action_for(parse_chord("j").unwrap()), Some("down"));
        assert_eq!(keys.action_for(parse_chord("down").unwrap()), Some("down"));
    }

    #[test]
    fn no_view_claims_a_global_key() {
        let globals: Vec<(Chord, &str)> = GLOBALS
            .iter()
            .map(|(k, a)| (parse_chord(k).unwrap(), *a))
            .collect();
        let mut clashes = Vec::new();
        for view in all() {
            for (chord, global) in &globals {
                if let Some(local) = view.action_for(*chord) {
                    clashes.push(format!(
                        "{}: `{}` is `{local}`, but globally `{global}`",
                        view.scope, chord
                    ));
                }
            }
        }
        assert!(clashes.is_empty(), "{}", clashes.join("\n"));
    }

    #[test]
    fn files_tree_does_not_take_the_event_log_key() {
        let e = parse_chord("e").unwrap();
        assert_eq!(files_tree().action_for(e), None);
        assert_eq!(files().action_for(e), None);
    }
}
