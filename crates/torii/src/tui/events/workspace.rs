//! Workspace view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_workspace(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // confirm states
    match &app.workspace_view.confirm {
        WorkspaceConfirm::DeleteWorkspace => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    return Some(Action::WorkspaceDelete);
                }
                _ => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                }
            }
            return None;
        }
        WorkspaceConfirm::RemoveRepo => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    return Some(Action::WorkspaceRemoveRepo);
                }
                _ => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                }
            }
            return None;
        }
        WorkspaceConfirm::SaveMessage => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    app.workspace_view.input.clear();
                }
                (_, KeyCode::Enter) if !app.workspace_view.input.trim().is_empty() => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    return Some(Action::WorkspaceSave);
                }
                (_, KeyCode::Backspace) => {
                    app.workspace_view.input.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.workspace_view.input.push(c)
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        WorkspaceConfirm::AddRepoPath => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    app.workspace_view.input.clear();
                }
                (_, KeyCode::Enter) if !app.workspace_view.input.trim().is_empty() => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    return Some(Action::WorkspaceAddRepo);
                }
                (_, KeyCode::Backspace) => {
                    app.workspace_view.input.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.workspace_view.input.push(c)
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        WorkspaceConfirm::RenameWorkspace => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    app.workspace_view.input.clear();
                }
                (_, KeyCode::Enter) if !app.workspace_view.input.trim().is_empty() => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    return Some(Action::WorkspaceRename);
                }
                (_, KeyCode::Backspace) => {
                    app.workspace_view.input.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.workspace_view.input.push(c)
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        WorkspaceConfirm::None => {}
    }

    // ops dropdown
    if app.workspace_view.ops_mode {
        let is_repos = app.workspace_view.focus == WorkspaceFocus::Repos;
        let ops_len = if is_repos { 4 } else { 5 };
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.workspace_view.ops_idx > 0 => {
                app.workspace_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.workspace_view.ops_idx < ops_len - 1 =>
            {
                app.workspace_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.workspace_view.ops_idx;
                app.workspace_view.ops_mode = false;
                if is_repos {
                    // open(0), sync repo(1), sync workspace(2), remove from workspace(3)
                    return match idx {
                        0 => Some(Action::WorkspaceOpenRepo),
                        1 => Some(Action::WorkspaceSyncOne),
                        2 => Some(Action::WorkspaceSync),
                        3 => {
                            app.workspace_view.confirm = WorkspaceConfirm::RemoveRepo;
                            None
                        }
                        _ => None,
                    };
                } else {
                    // sync all(0), save all(1), rename(2), add repo(3), delete workspace(4)
                    return match idx {
                        0 => Some(Action::WorkspaceSync),
                        1 => {
                            app.workspace_view.input.clear();
                            app.workspace_view.confirm = WorkspaceConfirm::SaveMessage;
                            None
                        }
                        2 => {
                            app.workspace_view.input.clear();
                            app.workspace_view.confirm = WorkspaceConfirm::RenameWorkspace;
                            None
                        }
                        3 => {
                            app.workspace_view.input.clear();
                            app.workspace_view.confirm = WorkspaceConfirm::AddRepoPath;
                            None
                        }
                        4 => {
                            app.workspace_view.confirm = WorkspaceConfirm::DeleteWorkspace;
                            None
                        }
                        _ => None,
                    };
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.workspace_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match app.workspace_view.focus {
        WorkspaceFocus::Workspaces => match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.workspace_move_up(),
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.workspace_move_down(),
            (_, KeyCode::Right) | (_, KeyCode::Char('l')) => app.workspace_focus_repos(),
            (_, KeyCode::Enter) => app.workspace_focus_repos(),
            (_, KeyCode::Char('o')) => {
                app.workspace_view.ops_mode = true;
                app.workspace_view.ops_idx = 0;
            }
            _ => {}
        },
        WorkspaceFocus::Repos => match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.workspace_move_up(),
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.workspace_move_down(),
            (_, KeyCode::Left) | (_, KeyCode::Char('h')) => app.workspace_focus_workspaces(),
            (_, KeyCode::Enter) => return Some(Action::WorkspaceOpenRepo),
            (_, KeyCode::Char('o')) => {
                app.workspace_view.ops_mode = true;
                app.workspace_view.ops_idx = 0;
            }
            _ => {}
        },
    }
    None
}
