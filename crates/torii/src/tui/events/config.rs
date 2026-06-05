//! Config + settings view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_config(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if app.config_view.editing {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.config_view.editing = false;
            }
            (_, KeyCode::Enter) => return Some(Action::ConfigSave),
            (_, KeyCode::Backspace) => app.config_backspace(),
            (_, KeyCode::Left) => app.config_cursor_left(),
            (_, KeyCode::Right) => app.config_cursor_right(),
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.config_type_char(c)
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
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.config_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.config_move_down(),
        (_, KeyCode::Enter) => {
            app.config_start_edit();
            return Some(Action::ConfigEdit);
        }
        (_, KeyCode::Tab) => return Some(Action::ConfigToggleScope),
        _ => {}
    }
    None
}

#[allow(dead_code)]
pub(super) fn handle_settings(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.settings_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.settings_move_down(),
        (_, KeyCode::Enter) => return Some(Action::SettingsToggle),
        (_, KeyCode::Char('s')) => return Some(Action::SettingsSave),
        _ => {}
    }
    None
}
