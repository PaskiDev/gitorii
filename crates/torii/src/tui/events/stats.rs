//! Keys for the stats view: pick which numbers, and ask for them again.

use crossterm::event::{self, KeyCode};

use super::{handle_global_nav, Action};
use crate::tui::app::{App, StatsMode};

pub(super) fn handle_stats(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match key.code {
        KeyCode::Char('1') => app.stats_view.mode = StatsMode::Repo,
        KeyCode::Char('2') => app.stats_view.mode = StatsMode::Workspace,
        KeyCode::Char('3') => app.stats_view.mode = StatsMode::People,
        KeyCode::Up | KeyCode::Char('k') => app.stats_move(-1),
        KeyCode::Down | KeyCode::Char('j') => app.stats_move(1),
        KeyCode::Tab => app.stats_toggle_mode(),
        // The numbers are a snapshot; `r` takes another one.
        KeyCode::Char('r') => app.load_stats(),
        _ => {}
    }
    None
}
