//! Issue view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_issue(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // text input states
    match &app.issue_view.confirm {
        IssueConfirm::CreateTitle => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.issue_view.confirm = IssueConfirm::None;
                    app.issue_view.create_title.clear();
                }
                (_, KeyCode::Enter) if !app.issue_view.create_title.is_empty() => {
                    app.issue_view.create_desc.clear();
                    app.issue_view.confirm = IssueConfirm::CreateDesc;
                }
                (_, KeyCode::Backspace) => {
                    app.issue_view.create_title.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.issue_view.create_title.push(c)
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        IssueConfirm::CreateDesc => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.issue_view.confirm = IssueConfirm::None;
                    app.issue_view.create_title.clear();
                    app.issue_view.create_desc.clear();
                }
                (_, KeyCode::Enter) => {
                    return Some(Action::IssueCreate);
                }
                (_, KeyCode::Backspace) => {
                    app.issue_view.create_desc.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.issue_view.create_desc.push(c)
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        IssueConfirm::Comment => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.issue_view.confirm = IssueConfirm::None;
                    app.issue_view.comment_input.clear();
                }
                (_, KeyCode::Enter) if !app.issue_view.comment_input.is_empty() => {
                    return Some(Action::IssueComment);
                }
                (_, KeyCode::Backspace) => {
                    app.issue_view.comment_input.pop();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.issue_view.comment_input.push(c)
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        IssueConfirm::Close => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.issue_view.confirm = IssueConfirm::None;
                    return Some(Action::IssueClose);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {
                    app.issue_view.confirm = IssueConfirm::None;
                }
            }
            return None;
        }
        IssueConfirm::None => {}
    }

    // ops dropdown
    if app.issue_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.issue_view.ops_idx > 0 => {
                app.issue_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.issue_view.ops_idx < 3 => {
                app.issue_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.issue_view.ops_idx;
                app.issue_view.ops_mode = false;
                match idx {
                    0 => {
                        app.issue_view.create_title.clear();
                        app.issue_view.confirm = IssueConfirm::CreateTitle;
                    }
                    1 => {
                        app.issue_view.comment_input.clear();
                        app.issue_view.confirm = IssueConfirm::Comment;
                    }
                    2 => return Some(Action::IssueOpenBrowser),
                    3 => {
                        app.issue_view.confirm = IssueConfirm::Close;
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.issue_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('r') {
        return Some(Action::IssueRefresh);
    }
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.issue_view.idx > 0 => {
            app.issue_view.idx -= 1;
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j'))
            if app.issue_view.idx + 1 < app.issue_view.issues.len() =>
        {
            app.issue_view.idx += 1;
        }
        (_, KeyCode::Char('o')) => {
            app.issue_view.ops_mode = true;
            app.issue_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}
