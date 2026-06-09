//! Tag view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_tag(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match app.tag_view.confirm.clone() {
        TagConfirm::Delete => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.tag_view.confirm = TagConfirm::None;
                    return Some(Action::TagDelete);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {
                    app.tag_view.confirm = TagConfirm::None;
                }
            }
            return None;
        }
        TagConfirm::CreateName => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) if !app.tag_view.new_name.trim().is_empty() => {
                    app.tag_view.confirm = TagConfirm::CreateMessage;
                }
                (_, KeyCode::Backspace) => {
                    app.tag_view.new_name.pop();
                }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.tag_view.new_name.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        TagConfirm::CreateMessage => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    app.tag_view.confirm = TagConfirm::None;
                    return Some(Action::TagCreate);
                }
                (_, KeyCode::Backspace) => {
                    app.tag_view.new_message.pop();
                }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.tag_view.new_message.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        TagConfirm::None => {}
    }
    if app.tag_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.tag_view.ops_idx > 0 => {
                app.tag_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.tag_view.ops_idx < 2 => {
                app.tag_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.tag_view.ops_idx;
                app.tag_view.ops_mode = false;
                match idx {
                    0 => return Some(Action::TagPush),
                    1 => {
                        app.tag_view.new_name.clear();
                        app.tag_view.new_message.clear();
                        app.tag_view.confirm = TagConfirm::CreateName;
                    }
                    2 if !app.tag_view.tags.is_empty() => {
                        app.tag_view.confirm = TagConfirm::Delete;
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.tag_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    if app.tag_view.search_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.tag_view.search_mode = false;
                app.tag_view.search_query.clear();
                app.tag_view.filtered.clear();
            }
            (_, KeyCode::Enter) => {
                app.tag_view.search_mode = false;
            }
            (_, KeyCode::Backspace) => {
                app.tag_view.search_query.pop();
                app.tag_update_filter();
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.tag_view.search_query.push(c);
                app.tag_update_filter();
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
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.tag_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.tag_move_down(),
        (_, KeyCode::Char('o')) => {
            app.tag_view.ops_mode = true;
            app.tag_view.ops_idx = 0;
        }
        (_, KeyCode::Char('/')) => {
            app.tag_view.search_mode = true;
            app.tag_view.search_query.clear();
            app.tag_view.filtered.clear();
        }
        _ => {}
    }
    None
}
