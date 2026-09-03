//! Config + settings view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_config(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // The tab keys work from either side.
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('1')) if !app.config_view.editing => {
            app.config_view.tab = ConfigTab::Values;
            return None;
        }
        (KeyModifiers::NONE, KeyCode::Char('2')) if !app.config_view.editing => {
            app.config_view.tab = ConfigTab::Keys;
            return None;
        }
        _ => {}
    }
    if app.config_view.tab == ConfigTab::Keys {
        return handle_keys_tab(key, app);
    }
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

/// Keys tab: pick an action, then record a binding for it.
///
/// Recording is a separate mode handled in `events::mod` — while it is on,
/// every key belongs to the binding, so an action can be put on `q` or `Tab`
/// without those doing their usual job first.
fn handle_keys_tab(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    let last = crate::tui::keys::ACTIONS.len().saturating_sub(1);
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            app.config_view.keys_idx = app.config_view.keys_idx.saturating_sub(1);
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            app.config_view.keys_idx = (app.config_view.keys_idx + 1).min(last);
        }
        (_, KeyCode::Enter) => {
            if let Some(action) = crate::tui::keys::ACTIONS.get(app.config_view.keys_idx) {
                app.config_view.capturing = Some(action.id.to_string());
                app.config_view.captured.clear();
                app.config_view.status = None;
            }
        }
        (_, KeyCode::Char('d')) => {
            if let Some(action) = crate::tui::keys::ACTIONS.get(app.config_view.keys_idx) {
                app.keymap.unbind(action.id);
                match app.keymap.save() {
                    Ok(()) => {
                        app.config_view.status = Some(format!("{} unbound", action.id));
                    }
                    Err(e) => {
                        app.config_view.status = Some(format!("could not write keys.toml: {e}"));
                    }
                }
            }
        }
        _ => {}
    }
    None
}
