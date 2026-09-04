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
        // Two prompts, same shape as the rewrite pair — but both sides
        // want the full "Name <email>" form here, where the CLI lets
        // `--old` match loosely on a bare name or a bare email.
        //
        // The looseness is right for a flag and wrong for this menu. A
        // long history usually carries the same person under several
        // identities — one name over two or three emails, one email
        // under two spellings of the name — and a bare half of that pair
        // sweeps up every one of them. On the CLI you bound the damage
        // with `--since` and read the dry-run first; the menu offers
        // neither, so the pair has to be exact on both ends.
        //
        // Refusing on Enter — rather than passing it on and letting the
        // rewrite match the wrong set, or dropping the keystroke with no
        // word about why — is what `identity_error` carries to `ui.rs`.
        HistoryConfirm::ReauthorOld => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) if !app.history_view.input.trim().is_empty() => {
                    let parsed =
                        crate::history_reauthor::Identity::parse_full(&app.history_view.input);
                    match parsed {
                        Ok(_) => {
                            app.history_view.identity_error = None;
                            app.history_view.confirm = HistoryConfirm::ReauthorNew;
                        }
                        Err(why) => app.history_view.identity_error = Some(why.to_string()),
                    }
                }
                (_, KeyCode::Backspace) => {
                    app.history_view.input.pop();
                    app.history_view.identity_error = None;
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                // Unlike a path or a ref, an identity is full of capitals
                // and `<>`, which arrive with SHIFT held. The older
                // prompts only accept NONE and would silently drop every
                // one of them.
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.history_view.input.push(c);
                    app.history_view.identity_error = None;
                }
                _ => {}
            }
            return None;
        }
        HistoryConfirm::ReauthorNew => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) if !app.history_view.input2.trim().is_empty() => {
                    let parsed =
                        crate::history_reauthor::Identity::parse_full(&app.history_view.input2);
                    match parsed {
                        Ok(_) => {
                            app.history_view.identity_error = None;
                            app.history_view.confirm = HistoryConfirm::None;
                            return Some(Action::HistoryReauthor);
                        }
                        Err(why) => app.history_view.identity_error = Some(why.to_string()),
                    }
                }
                (_, KeyCode::Backspace) => {
                    app.history_view.input2.pop();
                    app.history_view.identity_error = None;
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.history_view.input2.push(c);
                    app.history_view.identity_error = None;
                }
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
