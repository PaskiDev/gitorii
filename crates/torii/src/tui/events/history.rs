//! History view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

#[allow(dead_code)]
pub(super) fn handle_history(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match app.history_view.confirm.clone() {
        HistoryConfirm::CherryPick => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryCherryPick);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {
                    app.history_view.confirm = HistoryConfirm::None;
                }
            }
            return None;
        }
        HistoryConfirm::Clean => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryClean);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {
                    app.history_view.confirm = HistoryConfirm::None;
                }
            }
            return None;
        }
        HistoryConfirm::RemoveFile => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) if !app.history_view.input.trim().is_empty() => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryRemoveFile);
                }
                (_, KeyCode::Backspace) => {
                    app.history_view.input.pop();
                }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::Rebase => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) if !app.history_view.input.trim().is_empty() => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryRebase);
                }
                (_, KeyCode::Backspace) => {
                    app.history_view.input.pop();
                }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::RewriteStart => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) if !app.history_view.input.trim().is_empty() => {
                    app.history_view.confirm = HistoryConfirm::RewriteEnd;
                }
                (_, KeyCode::Backspace) => {
                    app.history_view.input.pop();
                }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::RewriteEnd => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) if !app.history_view.input2.trim().is_empty() => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryRewrite);
                }
                (_, KeyCode::Backspace) => {
                    app.history_view.input2.pop();
                }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input2.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::Blame => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) if !app.history_view.input.trim().is_empty() => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryBlame);
                }
                (_, KeyCode::Backspace) => {
                    app.history_view.input.pop();
                }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::Scan => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('f')) => {
                    app.history_view.scan_full = !app.history_view.scan_full;
                }
                (_, KeyCode::Enter) => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryScan);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {
                    app.history_view.confirm = HistoryConfirm::None;
                }
            }
            return None;
        }
        HistoryConfirm::None => {}
    }
    if app.history_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.history_view.ops_idx > 0 => {
                app.history_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.history_view.ops_idx < 6 => {
                app.history_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.history_view.ops_idx;
                app.history_view.ops_mode = false;
                match idx {
                    0 => {
                        app.history_view.confirm = HistoryConfirm::CherryPick;
                    }
                    1 => {
                        app.history_view.input.clear();
                        app.history_view.confirm = HistoryConfirm::Rebase;
                    }
                    2 => {
                        app.history_view.confirm = HistoryConfirm::Scan;
                    }
                    3 => {
                        app.history_view.confirm = HistoryConfirm::Clean;
                    }
                    4 => {
                        app.history_view.input.clear();
                        app.history_view.confirm = HistoryConfirm::Blame;
                    }
                    5 => {
                        app.history_view.input.clear();
                        app.history_view.input2.clear();
                        app.history_view.confirm = HistoryConfirm::RewriteStart;
                    }
                    6 => {
                        app.history_view.input.clear();
                        app.history_view.confirm = HistoryConfirm::RemoveFile;
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.history_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.history_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.history_move_down(),
        (_, KeyCode::Char('o')) => {
            app.history_view.ops_mode = true;
            app.history_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}
