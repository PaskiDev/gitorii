//! Keys for the ignore view.
//!
//! Reading is the default; writing takes a deliberate key. Adding opens an
//! overlay that says which file the rule will land in before a character is
//! typed, and removing asks first — a rule is what keeps the scanner quiet,
//! so dropping one loosens it.

use crossterm::event::{self, KeyCode, KeyModifiers};

use super::Action;
use crate::tui::app::{App, IgnoreFocus, SafetyTab};

pub(super) fn handle_safety(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match app.ignore_view.focus.clone() {
        IgnoreFocus::List => handle_list(key, app),
        IgnoreFocus::Input => handle_input(key, app),
        IgnoreFocus::ConfirmDelete => handle_confirm(key, app),
    }
}

fn handle_list(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // The tabs answer from either side.
    match key.code {
        KeyCode::Char('1') => {
            app.ignore_view.tab = SafetyTab::Rules;
            return None;
        }
        KeyCode::Char('2') => {
            app.ignore_view.tab = SafetyTab::Scanner;
            return None;
        }
        KeyCode::Tab => {
            app.safety_toggle_tab();
            return None;
        }
        _ => {}
    }
    if app.ignore_view.tab == SafetyTab::Scanner {
        // The scanner tab is a report: it scrolls, and sends you to the rules
        // tab for anything that can be changed.
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                app.ignore_view.scanner_idx = app.ignore_view.scanner_idx.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.ignore_view.scanner_idx = app.ignore_view.scanner_idx.saturating_sub(1);
            }
            KeyCode::Char('r') => app.load_safety(),
            _ => {}
        }
        return None;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.ignore_move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.ignore_move_up(),
        KeyCode::Char('a') => {
            app.ignore_view.status = None;
            app.ignore_view.input.clear();
            app.ignore_view.cursor = 0;
            app.ignore_view.focus = IgnoreFocus::Input;
        }
        KeyCode::Char('d') => {
            if app.ignore_selected().is_some() {
                app.ignore_view.focus = IgnoreFocus::ConfirmDelete;
            }
        }
        KeyCode::Char('t') => app.ignore_toggle_kind(),
        KeyCode::Char('r') => {
            app.ignore_view.status = None;
            app.load_safety();
        }
        _ => {}
    }
    None
}

fn handle_input(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match key.code {
        KeyCode::Esc => {
            app.ignore_view.input.clear();
            app.ignore_view.cursor = 0;
            app.ignore_view.status = None;
            app.ignore_view.focus = IgnoreFocus::List;
        }
        KeyCode::Enter => {
            if !app.ignore_view.input.trim().is_empty() {
                app.ignore_commit_input();
            }
        }
        KeyCode::Tab => app.ignore_toggle_kind(),
        // Ctrl-T sends the rule to the other file — the one deliberate way to
        // put a secret pattern in the committed file.
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.ignore_toggle_origin()
        }
        KeyCode::Char(c) => {
            let idx = app.ignore_view.cursor.min(app.ignore_view.input.len());
            app.ignore_view.input.insert(idx, c);
            app.ignore_view.cursor = idx + c.len_utf8();
        }
        KeyCode::Backspace => {
            let cursor = app.ignore_view.cursor.min(app.ignore_view.input.len());
            if cursor > 0 {
                // Step back one char, not one byte: a pattern can hold any of
                // them.
                let prev = app.ignore_view.input[..cursor]
                    .chars()
                    .next_back()
                    .map(char::len_utf8)
                    .unwrap_or(1);
                app.ignore_view.input.remove(cursor - prev);
                app.ignore_view.cursor = cursor - prev;
            }
        }
        KeyCode::Left => {
            let cursor = app.ignore_view.cursor.min(app.ignore_view.input.len());
            let prev = app.ignore_view.input[..cursor]
                .chars()
                .next_back()
                .map(char::len_utf8)
                .unwrap_or(0);
            app.ignore_view.cursor = cursor.saturating_sub(prev);
        }
        KeyCode::Right => {
            let cursor = app.ignore_view.cursor.min(app.ignore_view.input.len());
            let next = app.ignore_view.input[cursor..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(0);
            app.ignore_view.cursor = (cursor + next).min(app.ignore_view.input.len());
        }
        _ => {}
    }
    None
}

fn handle_confirm(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match key.code {
        KeyCode::Char('y') => app.ignore_delete_selected(),
        _ => app.ignore_view.focus = IgnoreFocus::List,
    }
    None
}
