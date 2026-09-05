//! Clone view key handling.
//!
//! Four things can have the keyboard here: the repository list, the
//! host dropdown, one of the three text fields, and — over all of them
//! — a clone that is running. Esc is not handled here: it walks the
//! same chain in `events/mod.rs` that every other view's overlays use,
//! because Esc is intercepted before a view handler ever sees it.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_clone(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // A clone in flight owns the keyboard: the list under the modal is
    // not what the keys are about, and there is nothing to press until
    // it ends. When it has ended, one key puts the modal away.
    if app.clone_view.run.is_some() {
        if app.clone_run_in_flight() {
            return None;
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ')) {
            app.clone_view.run = None;
        }
        return None;
    }

    match app.clone_view.focus {
        CloneFocus::List => handle_list(key, app),
        CloneFocus::Platforms => handle_platforms(key, app),
        CloneFocus::Filter => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => app.clone_view.focus = CloneFocus::List,
                (_, KeyCode::Backspace) => {
                    app.clone_view.filter.pop();
                    app.clone_refilter();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.clone_view.filter.push(c);
                    app.clone_refilter();
                }
                // The list keeps moving under an open filter box, so a
                // match can be picked without closing it first.
                (_, KeyCode::Up) => app.clone_move_up(),
                (_, KeyCode::Down) => app.clone_move_down(),
                _ => {}
            }
            None
        }
        CloneFocus::Url => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    // A URL typed and confirmed is a clone asked for.
                    app.clone_view.focus = CloneFocus::List;
                    app.clone_start();
                }
                (_, KeyCode::Backspace) => {
                    app.clone_view.url_input.pop();
                    app.clone_sync_dest();
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.clone_view.url_input.push(c);
                    app.clone_sync_dest();
                }
                _ => {}
            }
            None
        }
        CloneFocus::Dest => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => app.clone_view.focus = CloneFocus::List,
                (_, KeyCode::Backspace) => {
                    app.clone_view.dest_input.pop();
                    app.clone_view.dest_edited = true;
                }
                (_, KeyCode::Char(c))
                    if key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT =>
                {
                    app.clone_view.dest_input.push(c);
                    app.clone_view.dest_edited = true;
                }
                _ => {}
            }
            None
        }
    }
}

fn handle_list(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // `q` and `?` before the view's own letters, the way every other
    // list view takes them.
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.clone_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.clone_move_down(),
        (_, KeyCode::Char('p')) => {
            app.clone_view.platform_idx = CLONE_PLATFORMS
                .iter()
                .position(|p| *p == app.clone_view.platform)
                .unwrap_or(0);
            app.clone_view.focus = CloneFocus::Platforms;
        }
        (_, KeyCode::Char('/')) => app.clone_view.focus = CloneFocus::Filter,
        (_, KeyCode::Char('u')) => app.clone_view.focus = CloneFocus::Url,
        (_, KeyCode::Char('d')) => app.clone_view.focus = CloneFocus::Dest,
        (_, KeyCode::Char('t')) => {
            app.clone_view.ssh = !app.clone_view.ssh;
        }
        (_, KeyCode::Char('r')) => app.load_clone_repos(),
        (_, KeyCode::Enter) => app.clone_start(),
        _ => {}
    }
    None
}

fn handle_platforms(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    let len = CLONE_PLATFORMS.len();
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.clone_view.platform_idx > 0 => {
            app.clone_view.platform_idx -= 1;
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.clone_view.platform_idx + 1 < len => {
            app.clone_view.platform_idx += 1;
        }
        (_, KeyCode::Enter) => {
            let picked = CLONE_PLATFORMS[app.clone_view.platform_idx.min(len - 1)];
            app.clone_view.focus = CloneFocus::List;
            // Re-asking the same host is a refresh, not a no-op: it is
            // the only way to see a repository created since the view
            // was opened without leaving it.
            app.clone_view.platform = picked.to_string();
            app.load_clone_repos();
        }
        _ => {}
    }
    None
}
