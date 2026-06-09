//! Branch view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_branch(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match app.branch_view.confirm.clone() {
        BranchConfirm::Delete => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.branch_view.confirm = BranchConfirm::None;
                    return Some(Action::BranchDelete);
                }
                _ => {
                    app.branch_view.confirm = BranchConfirm::None;
                    app.branch_view.status = Some("cancelled".to_string());
                }
            }
            return None;
        }
        BranchConfirm::NewBranch => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.branch_view.confirm = BranchConfirm::None;
                    app.branch_view.new_name.clear();
                }
                (_, KeyCode::Enter) => {
                    app.branch_view.confirm = BranchConfirm::None;
                    return Some(Action::BranchCreate);
                }
                (_, KeyCode::Backspace) => {
                    app.branch_view.new_name.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.branch_view.new_name.push(c)
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        BranchConfirm::None => {}
    }
    if app.branch_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.branch_view.ops_idx > 0 => {
                app.branch_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.branch_view.ops_idx < 3 => {
                app.branch_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.branch_view.ops_idx;
                app.branch_view.ops_mode = false;
                match idx {
                    0 => return Some(Action::BranchCheckout),
                    1 => {
                        app.branch_view.new_name.clear();
                        app.branch_view.confirm = BranchConfirm::NewBranch;
                    }
                    2 if !app.branch_view.current_has_upstream => {
                        return Some(Action::BranchPush);
                    }
                    3 => {
                        if let Some(b) = app.branch_view.branches.get(app.branch_view.idx) {
                            if !b.is_current && !b.is_remote {
                                app.branch_view.confirm = BranchConfirm::Delete;
                            } else if b.is_remote {
                                app.branch_view.status =
                                    Some("cannot delete remote branch".to_string());
                            } else {
                                app.branch_view.status =
                                    Some("cannot delete current branch".to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.branch_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    if app.branch_view.search_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.branch_view.search_mode = false;
                app.branch_view.search_query.clear();
                app.branch_view.filtered.clear();
            }
            (_, KeyCode::Enter) => {
                app.branch_view.search_mode = false;
            }
            (_, KeyCode::Backspace) => {
                app.branch_view.search_query.pop();
                app.branch_update_filter();
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.branch_view.search_query.push(c);
                app.branch_update_filter();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.branch_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.branch_move_down(),
        (_, KeyCode::Char('o')) => {
            app.branch_view.ops_mode = true;
            app.branch_view.ops_idx = 0;
        }
        (_, KeyCode::Char('/')) => {
            app.branch_view.search_mode = true;
            app.branch_view.search_query.clear();
            app.branch_view.filtered.clear();
        }
        _ => {}
    }
    None
}
