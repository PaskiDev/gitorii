//! Remote + mirror view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_remote(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // confirm states (text input)
    match &app.remote_view.confirm {
        RemoteConfirm::AddName => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_name.clear();
                }
                (_, KeyCode::Enter) if !app.remote_view.new_name.is_empty() => {
                    app.remote_view.confirm = RemoteConfirm::AddUrl;
                }
                (_, KeyCode::Backspace) => {
                    app.remote_view.new_name.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.remote_view.new_name.push(c)
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::AddUrl => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_name.clear();
                    app.remote_view.new_url.clear();
                }
                (_, KeyCode::Enter) if !app.remote_view.new_url.is_empty() => {
                    return Some(Action::RemoteAdd);
                }
                (_, KeyCode::Backspace) => {
                    app.remote_view.new_url.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.remote_view.new_url.push(c)
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::Remove => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    return Some(Action::RemoteRemove);
                }
                _ => {
                    app.remote_view.confirm = RemoteConfirm::None;
                }
            }
            return None;
        }
        RemoteConfirm::Rename => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_name.clear();
                }
                (_, KeyCode::Enter) if !app.remote_view.new_name.is_empty() => {
                    return Some(Action::RemoteRename);
                }
                (_, KeyCode::Backspace) => {
                    app.remote_view.new_name.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.remote_view.new_name.push(c)
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::EditUrl => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_url.clear();
                }
                (_, KeyCode::Enter) if !app.remote_view.new_url.is_empty() => {
                    return Some(Action::RemoteEditUrl);
                }
                (_, KeyCode::Backspace) => {
                    app.remote_view.new_url.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.remote_view.new_url.push(c)
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorRename => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_name.clear();
                }
                (_, KeyCode::Enter) if !app.remote_view.new_name.is_empty() => {
                    return Some(Action::MirrorRename);
                }
                (_, KeyCode::Backspace) => {
                    app.remote_view.new_name.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.remote_view.new_name.push(c)
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorAddPlatform => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_mirror_platform.clear();
                }
                (_, KeyCode::Enter) if !app.remote_view.new_mirror_platform.is_empty() => {
                    app.remote_view.new_mirror_account.clear();
                    app.remote_view.confirm = RemoteConfirm::MirrorAddAccount;
                }
                (_, KeyCode::Backspace) => {
                    app.remote_view.new_mirror_platform.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.remote_view.new_mirror_platform.push(c)
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorAddAccount => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_mirror_platform.clear();
                    app.remote_view.new_mirror_account.clear();
                }
                (_, KeyCode::Enter) if !app.remote_view.new_mirror_account.is_empty() => {
                    app.remote_view.new_mirror_repo.clear();
                    app.remote_view.confirm = RemoteConfirm::MirrorAddRepo;
                }
                (_, KeyCode::Backspace) => {
                    app.remote_view.new_mirror_account.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.remote_view.new_mirror_account.push(c)
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorAddRepo => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_mirror_platform.clear();
                    app.remote_view.new_mirror_account.clear();
                    app.remote_view.new_mirror_repo.clear();
                }
                (_, KeyCode::Enter) if !app.remote_view.new_mirror_repo.is_empty() => {
                    app.remote_view.new_mirror_type = 0;
                    app.remote_view.confirm = RemoteConfirm::MirrorAddType;
                }
                (_, KeyCode::Backspace) => {
                    app.remote_view.new_mirror_repo.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.remote_view.new_mirror_repo.push(c)
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorAddType => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                }
                (_, KeyCode::Left) | (_, KeyCode::Char('h')) => {
                    app.remote_view.new_mirror_type = 0;
                }
                (_, KeyCode::Right) | (_, KeyCode::Char('l')) => {
                    app.remote_view.new_mirror_type = 1;
                }
                (_, KeyCode::Enter) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    return Some(Action::MirrorAdd);
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::None => {}
    }

    // ops dropdown
    if app.remote_view.ops_mode {
        let is_mirror = app.remote_view.selected_is_mirror();
        // mirror dropdown: 6 ops; git-remote dropdown: 7 ops (gained "add
        // mirror" in 0.7.12 so users can create the first mirror without
        // already having one selected).
        let ops_len = if is_mirror { 6 } else { 7 };
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.remote_view.ops_idx > 0 => {
                app.remote_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.remote_view.ops_idx < ops_len - 1 =>
            {
                app.remote_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.remote_view.ops_idx;
                app.remote_view.ops_mode = false;
                if is_mirror {
                    // mirror ops: sync all(0), force sync(1), add mirror(2), set primary(3), rename(4), remove(5)
                    return match idx {
                        0 => Some(Action::MirrorSync),
                        1 => Some(Action::MirrorSyncForce),
                        2 => {
                            app.remote_view.new_mirror_platform.clear();
                            app.remote_view.new_mirror_account.clear();
                            app.remote_view.new_mirror_repo.clear();
                            app.remote_view.new_mirror_type = 0;
                            app.remote_view.confirm = RemoteConfirm::MirrorAddPlatform;
                            None
                        }
                        3 => Some(Action::MirrorSetPrimary),
                        4 => {
                            app.remote_view.new_name.clear();
                            app.remote_view.confirm = RemoteConfirm::MirrorRename;
                            None
                        }
                        5 => Some(Action::MirrorRemove),
                        _ => None,
                    };
                } else {
                    // git remote ops: fetch(0), add remote(1), add mirror(2), rename(3), edit url(4), remove(5), open(6)
                    return match idx {
                        0 => Some(Action::RemoteFetch),
                        1 => {
                            app.remote_view.new_name.clear();
                            app.remote_view.confirm = RemoteConfirm::AddName;
                            None
                        }
                        2 => {
                            app.remote_view.new_mirror_platform.clear();
                            app.remote_view.new_mirror_account.clear();
                            app.remote_view.new_mirror_repo.clear();
                            app.remote_view.new_mirror_type = 0;
                            app.remote_view.confirm = RemoteConfirm::MirrorAddPlatform;
                            None
                        }
                        3 => {
                            app.remote_view.new_name.clear();
                            app.remote_view.confirm = RemoteConfirm::Rename;
                            None
                        }
                        4 => {
                            app.remote_view.new_url.clear();
                            app.remote_view.confirm = RemoteConfirm::EditUrl;
                            None
                        }
                        5 => {
                            app.remote_view.confirm = RemoteConfirm::Remove;
                            None
                        }
                        6 => Some(Action::RemoteOpenBrowser),
                        _ => None,
                    };
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.remote_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            app.remote_move_up();
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            app.remote_move_down();
        }
        (_, KeyCode::Char('o')) => {
            app.remote_view.ops_mode = true;
            app.remote_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}

pub(super) fn handle_mirror(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if app.mirror_view.ops_mode {
        const OPS_LEN: usize = 3;
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.mirror_view.ops_idx > 0 => {
                app.mirror_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.mirror_view.ops_idx < OPS_LEN - 1 =>
            {
                app.mirror_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.mirror_view.ops_idx;
                app.mirror_view.ops_mode = false;
                return match idx {
                    0 => Some(Action::MirrorSync),
                    1 => Some(Action::MirrorSyncForce),
                    2 => Some(Action::MirrorRemove),
                    _ => None,
                };
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.mirror_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.mirror_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.mirror_move_down(),
        (_, KeyCode::Char('o')) => {
            app.mirror_view.ops_mode = true;
            app.mirror_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}
