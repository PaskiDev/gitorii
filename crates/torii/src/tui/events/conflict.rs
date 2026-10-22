//! Conflict prompt key handling: the same keys as the CLI prompt and
//! selector, so a hand that learned one works in the other.

use crate::cli::conflict_picker::{picker_key, prompt_key, PickerAction, PickerState, PromptKey};
use crate::tui::app::App;
use crate::vcs::conflict::{Resolution, Side};
use crossterm::event;

pub(super) fn handle_conflict(key: event::KeyEvent, app: &mut App) {
    let Some(p) = app.conflict.as_mut() else {
        return;
    };
    if let Some(st) = p.picker.as_mut() {
        let Some(k) = picker_key(&key) else {
            return;
        };
        match st.handle(k) {
            PickerAction::Continue => {}
            PickerAction::Apply(r) => app.conflict_resolve(r),
            PickerAction::Cancel => app.conflict_resolve(Resolution::Manual),
        }
        return;
    }
    match prompt_key(&key) {
        Some(PromptKey::Local) => app.conflict_resolve(Resolution::All(Side::Local)),
        Some(PromptKey::Remote) => app.conflict_resolve(Resolution::All(Side::Remote)),
        Some(PromptKey::Manual) => app.conflict_resolve(Resolution::Manual),
        Some(PromptKey::ByFile) => p.picker = Some(PickerState::new(&p.files)),
        None => {}
    }
}
