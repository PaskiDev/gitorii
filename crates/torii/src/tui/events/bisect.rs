//! Bisect view + ref-picker key handling.

use super::Action;
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

// 0.7.33 — Bisect view: contextual ops via `o` dropdown. start/good/
// bad/skip/run/reset routed through `crate::bisect`. Same dropdown +
// input + confirm chrome family as the Auth view.
pub(super) fn handle_bisect(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{BisectFocus, BisectPendingOp};

    match app.bisect_view.focus {
        BisectFocus::List => {
            if (key.modifiers, key.code) == (KeyModifiers::NONE, KeyCode::Char('o')) {
                app.bisect_view.dropdown_idx = 0;
                app.bisect_view.focus = BisectFocus::OpsDropdown;
            }
            None
        }
        BisectFocus::OpsDropdown => {
            let ops = crate::tui::views::bisect::ops_for(&app.bisect_view);
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.bisect_view.dropdown_idx > 0 => {
                    app.bisect_view.dropdown_idx -= 1;
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                    if app.bisect_view.dropdown_idx + 1 < ops.len() =>
                {
                    app.bisect_view.dropdown_idx += 1;
                }
                (_, KeyCode::Enter) => return dispatch_bisect_op(app),
                _ => {}
            }
            None
        }
        BisectFocus::RefPicker => handle_bisect_picker(key, app),
        BisectFocus::InputArgs => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    let buf = std::mem::take(&mut app.bisect_view.input_buffer);
                    let op = app.bisect_view.pending_op.clone();
                    app.bisect_view.focus = BisectFocus::List;
                    app.bisect_view.pending_op = BisectPendingOp::None;
                    let trimmed = buf.trim();
                    if trimmed.is_empty() {
                        app.set_status("✗ empty input, aborted");
                        return None;
                    }
                    match op {
                        BisectPendingOp::Run => {
                            let cmd: Vec<String> =
                                trimmed.split_whitespace().map(String::from).collect();
                            match crate::bisect::run(std::path::Path::new("."), &cmd) {
                                Ok(_) => app.set_status("✓ bisect run finished"),
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                        }
                        BisectPendingOp::None => {}
                    }
                    crate::tui::views::bisect::refresh(app);
                }
                (_, KeyCode::Backspace) => {
                    app.bisect_view.input_buffer.pop();
                }
                (_, KeyCode::Char(c)) if key.modifiers != KeyModifiers::CONTROL => {
                    app.bisect_view.input_buffer.push(c);
                }
                _ => {}
            }
            None
        }
        BisectFocus::ConfirmReset => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    match crate::bisect::reset(std::path::Path::new(".")) {
                        Ok(_) => app.set_status("✓ bisect reset"),
                        Err(e) => app.set_status(format!("✗ {}", e)),
                    }
                    app.bisect_view.focus = BisectFocus::List;
                    crate::tui::views::bisect::refresh(app);
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    app.bisect_view.focus = BisectFocus::List;
                }
                _ => {}
            }
            None
        }
    }
}

pub(super) fn dispatch_bisect_op(app: &mut App) -> Option<Action> {
    use crate::tui::app::{
        BisectFocus, BisectPendingOp, RefPickerOp, RefPickerState, RefPickerTab,
    };
    let ops = crate::tui::views::bisect::ops_for(&app.bisect_view);
    let idx = app.bisect_view.dropdown_idx;
    let label = ops.get(idx).map(|o| o.0).unwrap_or("");
    let path = std::path::Path::new(".");

    let open_picker = |app: &mut App, op: RefPickerOp| {
        app.bisect_view.picker = RefPickerState {
            op,
            tab: RefPickerTab::Bad,
            all: crate::tui::views::bisect::load_refs(),
            idx: 0,
            filter: String::new(),
            bad_pick: None,
            good_picks: Vec::new(),
        };
        app.bisect_view.focus = BisectFocus::RefPicker;
    };

    match label {
        "Start" => open_picker(app, RefPickerOp::Start),
        "Mark good <ref>…" => open_picker(app, RefPickerOp::MarkGood),
        "Mark bad <ref>…" => open_picker(app, RefPickerOp::MarkBad),
        "Skip <ref>…" => open_picker(app, RefPickerOp::Skip),

        "Mark HEAD good" => {
            match crate::bisect::good(path, None) {
                Ok(_) => app.set_status("✓ marked good"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            crate::tui::views::bisect::refresh(app);
        }
        "Mark HEAD bad" => {
            match crate::bisect::bad(path, None) {
                Ok(_) => app.set_status("✓ marked bad"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            crate::tui::views::bisect::refresh(app);
        }
        "Skip HEAD" => {
            match crate::bisect::skip(path, None) {
                Ok(_) => app.set_status("✓ skipped"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            crate::tui::views::bisect::refresh(app);
        }
        "Run command" => {
            app.bisect_view.focus = BisectFocus::InputArgs;
            app.bisect_view.input_buffer.clear();
            app.bisect_view.input_prompt = "command (e.g. cargo test -q)".to_string();
            app.bisect_view.pending_op = BisectPendingOp::Run;
        }
        "Reset" => {
            app.bisect_view.focus = BisectFocus::ConfirmReset;
        }
        _ => {
            app.bisect_view.focus = BisectFocus::List;
        }
    }
    None
}

// Guard-collapsing the nav/Space arms would make a falsified condition
// fall through to the generic `Char(c)` filter-input arm (typing j/k/␣
// into the filter), so the nested-if form is load-bearing here.
#[allow(clippy::collapsible_match)]
pub(super) fn handle_bisect_picker(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{RefPickerOp, RefPickerTab};

    // Resolve the filtered slice + currently highlighted entry every
    // call so any filter-buffer mutation immediately reflects in
    // navigation.
    let filtered = crate::tui::views::bisect::filter_indexes(
        &app.bisect_view.picker.all,
        &app.bisect_view.picker.filter,
    );
    let total = filtered.len();
    let cur_idx = app.bisect_view.picker.idx.min(total.saturating_sub(1));

    match (key.modifiers, key.code) {
        // Navigation. `j`/`k` only when the filter is empty so they
        // don't collide with typing into the filter.
        (_, KeyCode::Up) if cur_idx > 0 => {
            app.bisect_view.picker.idx = cur_idx - 1;
        }
        (_, KeyCode::Down) if cur_idx + 1 < total => {
            app.bisect_view.picker.idx = cur_idx + 1;
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) if app.bisect_view.picker.filter.is_empty() => {
            if cur_idx > 0 {
                app.bisect_view.picker.idx = cur_idx - 1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('j')) if app.bisect_view.picker.filter.is_empty() => {
            if cur_idx + 1 < total {
                app.bisect_view.picker.idx = cur_idx + 1;
            }
        }

        // Tab switches Bad ↔ Good only for the Start op.
        (_, KeyCode::Tab) | (_, KeyCode::BackTab)
            if matches!(app.bisect_view.picker.op, RefPickerOp::Start) =>
        {
            app.bisect_view.picker.tab = match app.bisect_view.picker.tab {
                RefPickerTab::Bad => RefPickerTab::Good,
                RefPickerTab::Good => RefPickerTab::Bad,
            };
            app.bisect_view.picker.idx = 0;
            app.bisect_view.picker.filter.clear();
        }

        // Space: in Start/Good tab, toggle the current entry in the
        // good_picks set (multi-select). Ignored elsewhere.
        (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            if matches!(app.bisect_view.picker.op, RefPickerOp::Start)
                && app.bisect_view.picker.tab == RefPickerTab::Good
                && total > 0
            {
                if let Some(&orig) = filtered.get(cur_idx) {
                    let entry = app.bisect_view.picker.all[orig].clone();
                    let pos = app
                        .bisect_view
                        .picker
                        .good_picks
                        .iter()
                        .position(|g| g.target == entry.target);
                    match pos {
                        Some(p) => {
                            app.bisect_view.picker.good_picks.remove(p);
                        }
                        None => {
                            app.bisect_view.picker.good_picks.push(entry);
                        }
                    }
                }
            }
        }

        // Enter: commit the picker depending on op.
        (_, KeyCode::Enter) => {
            return commit_bisect_picker(app, filtered, cur_idx);
        }

        // Filter: backspace removes one char.
        (_, KeyCode::Backspace) => {
            app.bisect_view.picker.filter.pop();
            app.bisect_view.picker.idx = 0;
        }

        // Filter: any printable char that isn't a navigation key.
        (mods, KeyCode::Char(c)) if mods == KeyModifiers::NONE || mods == KeyModifiers::SHIFT => {
            app.bisect_view.picker.filter.push(c);
            app.bisect_view.picker.idx = 0;
        }

        _ => {}
    }

    None
}

pub(super) fn commit_bisect_picker(
    app: &mut App,
    filtered: Vec<usize>,
    cur_idx: usize,
) -> Option<Action> {
    use crate::tui::app::{BisectFocus, RefPickerOp, RefPickerTab};
    let path = std::path::Path::new(".");

    let &orig = filtered.get(cur_idx)?;
    let entry = app.bisect_view.picker.all[orig].clone();

    match app.bisect_view.picker.op.clone() {
        RefPickerOp::Start => {
            match app.bisect_view.picker.tab {
                RefPickerTab::Bad => {
                    // Pick this as bad and advance to the Good tab.
                    app.bisect_view.picker.bad_pick = Some(entry);
                    app.bisect_view.picker.tab = RefPickerTab::Good;
                    app.bisect_view.picker.idx = 0;
                    app.bisect_view.picker.filter.clear();
                }
                RefPickerTab::Good => {
                    // If the user hit Enter with nothing toggled, take
                    // the highlighted row as the (single) good. Either
                    // way we need at least one bad + one good.
                    if app.bisect_view.picker.good_picks.is_empty() {
                        app.bisect_view.picker.good_picks.push(entry);
                    }
                    let bad = app.bisect_view.picker.bad_pick.clone();
                    let goods: Vec<String> = app
                        .bisect_view
                        .picker
                        .good_picks
                        .iter()
                        .map(|e| e.target.clone())
                        .collect();
                    let Some(bad) = bad else {
                        app.set_status("✗ no bad ref selected");
                        return None;
                    };
                    let res = crate::bisect::start(path, Some(&bad.target), &goods);
                    match res {
                        Ok(_) => app.set_status(format!(
                            "✓ bisect started (bad: {} · {} good ref(s))",
                            bad.display,
                            goods.len()
                        )),
                        Err(e) => app.set_status(format!("✗ {}", e)),
                    }
                    app.bisect_view.focus = BisectFocus::List;
                    app.bisect_view.picker = Default::default();
                    crate::tui::views::bisect::refresh(app);
                }
            }
        }
        RefPickerOp::MarkGood => {
            let res = crate::bisect::good(path, Some(&entry.target));
            match res {
                Ok(_) => app.set_status(format!("✓ marked good: {}", entry.display)),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            app.bisect_view.picker = Default::default();
            crate::tui::views::bisect::refresh(app);
        }
        RefPickerOp::MarkBad => {
            let res = crate::bisect::bad(path, Some(&entry.target));
            match res {
                Ok(_) => app.set_status(format!("✓ marked bad: {}", entry.display)),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            app.bisect_view.picker = Default::default();
            crate::tui::views::bisect::refresh(app);
        }
        RefPickerOp::Skip => {
            let res = crate::bisect::skip(path, Some(&entry.target));
            match res {
                Ok(_) => app.set_status(format!("✓ skipped: {}", entry.display)),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            app.bisect_view.picker = Default::default();
            crate::tui::views::bisect::refresh(app);
        }
    }
    None
}
