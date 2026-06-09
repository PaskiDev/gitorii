//! Commit (save) view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_commit(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    const N_TYPES: usize = 8;
    match app.commit_view.focus {
        CommitFocus::List => {
            if let Some(a) = handle_global_nav(key, app) {
                return Some(a);
            }
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => app.commit_view.focus = CommitFocus::TypeSelector,
                (_, KeyCode::Char('i')) => {
                    app.commit_view.focus = CommitFocus::Input;
                }
                (_, KeyCode::Char('a')) => {
                    app.commit_view.amend = !app.commit_view.amend;
                    if app.commit_view.amend && app.commit_view.message.is_empty() {
                        // Pre-fill with last commit message
                        if let Some(c) = app.commits.first() {
                            app.commit_view.message = c.message.clone();
                            app.commit_view.cursor = c.message.len();
                        }
                    }
                }
                _ => {}
            }
        }
        CommitFocus::TypeSelector => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.commit_view.type_idx > 0 => {
                    app.commit_view.type_idx -= 1;
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                    if app.commit_view.type_idx < N_TYPES - 1 =>
                {
                    app.commit_view.type_idx += 1;
                }
                (_, KeyCode::Enter) => {
                    let prefix = COMMIT_TYPES[app.commit_view.type_idx].0;
                    let prefix_str = format!("{}: ", prefix);
                    if !app.commit_view.message.starts_with(&prefix_str) {
                        // Strip any existing type prefix first
                        let base = if let Some(colon) = app.commit_view.message.find(": ") {
                            app.commit_view.message[colon + 2..].to_string()
                        } else {
                            app.commit_view.message.clone()
                        };
                        app.commit_view.message = format!("{}{}", prefix_str, base);
                        app.commit_view.cursor = app.commit_view.message.len();
                    }
                    app.commit_view.focus = CommitFocus::Input;
                }
                (_, KeyCode::Esc) => app.commit_view.focus = CommitFocus::List,
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
        }
        CommitFocus::Input => match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => app.commit_view.focus = CommitFocus::TypeSelector,
            (_, KeyCode::Enter) => return Some(Action::CommitConfirm),
            (_, KeyCode::Backspace) => app.commit_backspace(),
            (_, KeyCode::Left) => app.commit_cursor_left(),
            (_, KeyCode::Right) => app.commit_cursor_right(),
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.commit_type_char(c)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        },
    }
    None
}

pub const COMMIT_TYPES: &[(&str, &str)] = &[
    ("feat", "new feature"),
    ("fix", "bug fix"),
    ("chore", "maintenance task"),
    ("docs", "documentation"),
    ("refactor", "code restructure"),
    ("test", "tests"),
    ("ci", "CI/CD changes"),
    ("perf", "performance improvement"),
];
