//! Log view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_log(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if app.log.search_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.log.search_mode = false;
                app.log.search_query.clear();
                app.log.filtered.clear();
            }
            (_, KeyCode::Enter) => {
                app.log.search_mode = false;
            }
            (_, KeyCode::Backspace) => {
                app.log.search_query.pop();
                app.log_update_filter();
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.log.search_query.push(c);
                app.log_update_filter();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }
    if app.log.ops_mode {
        let ops_len = crate::tui::views::log::log_ops().len();
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.log.ops_idx > 0 => {
                app.log.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.log.ops_idx + 1 < ops_len => {
                app.log.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.log.ops_idx;
                app.log.ops_mode = false;
                // 0.8.3 — extended ops menu. Indices map onto the
                // `log_ops()` list; the renderer reads the same list
                // so the order can't drift. Ops that already have a
                // dispatcher (cherry-pick / blame / scan / clean,
                // wired in 0.7.2 when history fused into log) come
                // back as Action variants and run from the main
                // loop's handler. Ops that need an interactive prompt
                // (remove-file, rewrite) flip the same `confirm`
                // state the old History view used, which the global
                // dispatcher already routes correctly. Show-signature
                // opens the armor overlay directly because the worker
                // is already a method on App.
                match idx {
                    0 => return Some(Action::OpenDiffFromLog),
                    1 => return Some(Action::LogCopyHash),
                    2 => {
                        app.log.search_mode = true;
                        app.log.search_query.clear();
                        app.log.filtered.clear();
                    }
                    3 => return Some(Action::HistoryCherryPick),
                    4 => return Some(Action::HistoryRebase),
                    5 => {
                        if let Some(c) = app.commits.get(app.log.idx) {
                            let oid = c.full_hash.clone();
                            app.start_signature_overlay(oid);
                        }
                    }
                    6 => return Some(Action::HistoryBlame),
                    7 => return Some(Action::HistoryRemoveFile),
                    8 => return Some(Action::HistoryRewrite),
                    9 => return Some(Action::HistoryScan),
                    10 => return Some(Action::HistoryClean),
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.log.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    // 0.7.36 — armor overlay grabs every key so the user can dismiss
    // it without accidentally firing log-navigation keybinds.
    if app.log.signature_overlay.is_some() {
        // Esc / any non-modifier key closes the modal.
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {
                app.log.signature_overlay = None;
                app.log_signature_rx = None;
            }
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            app.log_move_up();
            app.log.ops_mode = false;
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            app.log_move_down();
            app.log.ops_mode = false;
        }
        (_, KeyCode::Char('o')) => {
            app.log.ops_mode = true;
            app.log.ops_idx = 0;
        }
        (_, KeyCode::Char('/')) => {
            app.log.search_mode = true;
            app.log.search_query.clear();
            app.log.filtered.clear();
        }
        // 0.7.36 — toggle the GPG signature column. Populates the
        // verdict cache the first time it turns on (cheap because we
        // only run gpg --verify on the commits already loaded into
        // the log slice, typically ≤ page_size).
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (_, KeyCode::Char('G')) => {
            app.log.show_signatures = !app.log.show_signatures;
            if app.log.show_signatures {
                app.refresh_signature_cache();
            }
        }
        // 0.7.36 — open the armor overlay for the currently selected
        // commit. Spawns a worker so the modal can render `loading…`
        // while gpg runs.
        (KeyModifiers::SHIFT, KeyCode::Char('S')) | (_, KeyCode::Char('S')) => {
            if let Some(c) = app.commits.get(app.log.idx) {
                let oid = c.full_hash.clone();
                app.start_signature_overlay(oid);
            }
        }
        _ => {}
    }
    None
}
