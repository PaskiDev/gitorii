//! Snapshot view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_snapshot(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{AutoSnapshotInterval, SnapshotFocus};

    // ops dropdown (only in List focus)
    if app.snapshot_view.focus == SnapshotFocus::List && app.snapshot_view.ops_mode {
        const OPS_LEN: usize = 4;
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.snapshot_view.ops_idx > 0 => {
                app.snapshot_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.snapshot_view.ops_idx < OPS_LEN - 1 =>
            {
                app.snapshot_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.snapshot_view.ops_idx;
                app.snapshot_view.ops_mode = false;
                return match idx {
                    0 => Some(Action::SnapshotRestore),
                    1 => Some(Action::SnapshotDiff),
                    2 => {
                        app.snapshot_view.create_name.clear();
                        app.snapshot_view.focus = SnapshotFocus::Create;
                        None
                    }
                    3 => Some(Action::SnapshotDelete),
                    _ => None,
                };
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.snapshot_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    match app.snapshot_view.focus {
        SnapshotFocus::List => {
            // search mode input
            if app.snapshot_view.search_mode {
                match (key.modifiers, key.code) {
                    (_, KeyCode::Esc) => {
                        app.snapshot_view.search_mode = false;
                        app.snapshot_view.search_query.clear();
                        app.snapshot_view.filtered.clear();
                        app.snapshot_view.idx = 0;
                    }
                    (_, KeyCode::Enter) => {
                        app.snapshot_view.search_mode = false;
                    }
                    (_, KeyCode::Backspace) => {
                        app.snapshot_view.search_query.pop();
                        snapshot_update_filter(app);
                    }
                    (_, KeyCode::Char(c))
                        if key.modifiers == KeyModifiers::NONE
                            || key.modifiers == KeyModifiers::SHIFT =>
                    {
                        app.snapshot_view.search_query.push(c);
                        snapshot_update_filter(app);
                    }
                    _ => {}
                }
                return None;
            }

            if let Some(a) = handle_global_nav(key, app) {
                return Some(a);
            }
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.snapshot_move_up(),
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.snapshot_move_down(),
                (_, KeyCode::Char('o')) => {
                    app.snapshot_view.ops_mode = true;
                    app.snapshot_view.ops_idx = 0;
                }
                (_, KeyCode::Char('d')) => return Some(Action::SnapshotDiff),
                (_, KeyCode::Char('/')) => {
                    app.snapshot_view.search_mode = true;
                    app.snapshot_view.search_query.clear();
                    app.snapshot_view.filtered.clear();
                    app.snapshot_view.idx = 0;
                }
                (_, KeyCode::Char('n')) => {
                    app.snapshot_view.create_name.clear();
                    app.snapshot_view.focus = SnapshotFocus::Create;
                }
                (_, KeyCode::Char('a')) => {
                    app.snapshot_view.auto_interval_idx =
                        AutoSnapshotInterval::picker_rows(&app.snapshot_view.auto_interval)
                            .iter()
                            .position(|i| i == &app.snapshot_view.auto_interval)
                            .unwrap_or(0);
                    app.snapshot_view.focus = SnapshotFocus::AutoConfig;
                }
                _ => {}
            }
        }
        SnapshotFocus::Create => match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => app.snapshot_view.focus = SnapshotFocus::List,
            (_, KeyCode::Enter) => return Some(Action::SnapshotCreate),
            (_, KeyCode::Backspace) => {
                app.snapshot_view.create_name.pop();
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.snapshot_view.create_name.push(c)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        },
        SnapshotFocus::AutoConfig => {
            let rows = AutoSnapshotInterval::picker_rows(&app.snapshot_view.auto_interval);
            let n = rows.len();
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k'))
                    if app.snapshot_view.auto_interval_idx > 0 =>
                {
                    app.snapshot_view.auto_interval_idx -= 1;
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                    if app.snapshot_view.auto_interval_idx < n - 1 =>
                {
                    app.snapshot_view.auto_interval_idx += 1;
                }
                (_, KeyCode::Enter) => {
                    let chosen = rows[app.snapshot_view.auto_interval_idx].clone();
                    if chosen.is_custom() {
                        app.snapshot_view.auto_custom_input = chosen
                            .minutes()
                            .map(AutoSnapshotInterval::input_hint)
                            .unwrap_or_default();
                        app.snapshot_view.auto_custom_error = None;
                        app.snapshot_view.focus = SnapshotFocus::AutoCustom;
                        return None;
                    }
                    app.snapshot_view.auto_interval = chosen;
                    app.snapshot_view.focus = SnapshotFocus::List;
                    return Some(Action::SnapshotSaveInterval);
                }
                (_, KeyCode::Esc) => app.snapshot_view.focus = SnapshotFocus::List,
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
        }
        SnapshotFocus::AutoCustom => match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.snapshot_view.auto_custom_input.clear();
                app.snapshot_view.auto_custom_error = None;
                app.snapshot_view.focus = SnapshotFocus::AutoConfig;
            }
            (_, KeyCode::Enter) => {
                let raw = app.snapshot_view.auto_custom_input.trim();
                if raw.is_empty() {
                    app.snapshot_view.auto_custom_error =
                        Some("type a duration, e.g. 30m or 2h".to_string());
                    return None;
                }
                match crate::duration::parse_duration(raw) {
                    Ok(mins) => {
                        app.snapshot_view.auto_interval = AutoSnapshotInterval::from_minutes(mins);
                        app.snapshot_view.auto_custom_input.clear();
                        app.snapshot_view.auto_custom_error = None;
                        app.snapshot_view.focus = SnapshotFocus::List;
                        return Some(Action::SnapshotSaveInterval);
                    }
                    Err(_) => {
                        app.snapshot_view.auto_custom_error =
                            Some("use 30m, 1h, 2h, or a number of minutes".to_string());
                    }
                }
            }
            (_, KeyCode::Backspace) => {
                app.snapshot_view.auto_custom_input.pop();
                app.snapshot_view.auto_custom_error = None;
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.snapshot_view.auto_custom_input.push(c);
                app.snapshot_view.auto_custom_error = None;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        },
    }
    None
}

pub(super) fn snapshot_update_filter(app: &mut App) {
    let q = app.snapshot_view.search_query.to_lowercase();
    if q.is_empty() {
        app.snapshot_view.filtered.clear();
    } else {
        app.snapshot_view.filtered = app
            .snapshot_view
            .snapshots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.name.to_lowercase().contains(&q) || s.id.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
    }
    app.snapshot_view.idx = 0;
}
