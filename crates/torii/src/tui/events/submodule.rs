//! Submodule view key handling.

use super::Action;
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

// ── Platform view (0.7.12) ────────────────────────────────────────────────────
//
// Sub-tabs: 1/2/3/4 → pipelines/jobs/releases/packages.
// 'r' opens the remote-selector popup (centred over the view); inside the
// popup ↑/↓ navigates, Enter selects, Esc closes.
// On the Pipelines list, Enter drills into the selected pipeline's Jobs.
// On the Jobs list (drill-down), Enter fetches the job log into a
// scrollable panel; Esc walks the drill-down back (handled outside).
// Ctrl-R re-runs the loader for whatever sub-tab is active.
// 0.7.40 — Submodule view: contextual ops via `o` dropdown. Each op
// routes through `crate::cmd::submodule`. Add is a two-step input
// (URL → path); Remove gates behind a y/n confirm; rest dispatch
// directly.
pub(super) fn handle_submodule(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{SubmoduleFocus, SubmodulePendingOp};
    use std::path::Path;

    match app.submodule_view.focus {
        SubmoduleFocus::List => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                    if app.submodule_view.idx > 0 {
                        app.submodule_view.idx -= 1;
                    }
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                    if app.submodule_view.idx + 1 < app.submodule_view.items.len() {
                        app.submodule_view.idx += 1;
                    }
                }
                (KeyModifiers::NONE, KeyCode::Char('o')) => {
                    app.submodule_view.dropdown_idx = 0;
                    app.submodule_view.focus = SubmoduleFocus::OpsDropdown;
                }
                _ => {}
            }
            None
        }
        SubmoduleFocus::OpsDropdown => {
            let ops = crate::tui::views::submodule::ops_for(&app.submodule_view);
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                    if app.submodule_view.dropdown_idx > 0 {
                        app.submodule_view.dropdown_idx -= 1;
                    }
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                    if app.submodule_view.dropdown_idx + 1 < ops.len() {
                        app.submodule_view.dropdown_idx += 1;
                    }
                }
                (_, KeyCode::Enter) => return dispatch_submodule_op(app),
                _ => {}
            }
            None
        }
        SubmoduleFocus::InputArgs => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    let buf = std::mem::take(&mut app.submodule_view.input_buffer);
                    let op = app.submodule_view.pending_op.clone();
                    let trimmed = buf.trim().to_string();
                    if trimmed.is_empty() {
                        app.submodule_view.focus = SubmoduleFocus::List;
                        app.submodule_view.pending_op = SubmodulePendingOp::None;
                        app.set_status("✗ empty input, aborted");
                        return None;
                    }
                    let repo_path = std::path::PathBuf::from(".");
                    match op {
                        SubmodulePendingOp::AddUrl => {
                            // Stash URL, prompt for path.
                            app.submodule_view.pending_url = trimmed;
                            app.submodule_view.input_prompt =
                                "path (relative to repo root)".to_string();
                            app.submodule_view.pending_op = SubmodulePendingOp::AddPath;
                            // focus stays on InputArgs
                        }
                        SubmodulePendingOp::AddPath => {
                            let url = std::mem::take(&mut app.submodule_view.pending_url);
                            let path = std::path::PathBuf::from(trimmed);
                            let opts = crate::cmd::submodule::AddOpts {
                                branch: None,
                                name: None,
                                recursive: false,
                            };
                            match crate::cmd::submodule::add(&repo_path, &url, &path, &opts) {
                                Ok(_) => app.set_status(format!(
                                    "✓ submodule added at `{}`",
                                    path.display()
                                )),
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                            app.submodule_view.focus = SubmoduleFocus::List;
                            app.submodule_view.pending_op = SubmodulePendingOp::None;
                            crate::tui::views::submodule::refresh(app);
                        }
                        SubmodulePendingOp::Foreach => {
                            match crate::cmd::submodule::foreach(&repo_path, &trimmed) {
                                Ok(_) => app.set_status("✓ foreach completed"),
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                            app.submodule_view.focus = SubmoduleFocus::List;
                            app.submodule_view.pending_op = SubmodulePendingOp::None;
                            crate::tui::views::submodule::refresh(app);
                        }
                        SubmodulePendingOp::None => {
                            app.submodule_view.focus = SubmoduleFocus::List;
                        }
                    }
                }
                (_, KeyCode::Backspace) => {
                    app.submodule_view.input_buffer.pop();
                }
                (_, KeyCode::Char(c)) if key.modifiers != KeyModifiers::CONTROL => {
                    app.submodule_view.input_buffer.push(c);
                }
                _ => {}
            }
            None
        }
        SubmoduleFocus::ConfirmRemove => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    let path = app
                        .submodule_view
                        .items
                        .get(app.submodule_view.idx)
                        .map(|s| std::path::PathBuf::from(&s.path));
                    if let Some(p) = path {
                        match crate::cmd::submodule::remove(Path::new("."), &p) {
                            Ok(_) => app.set_status(format!("✓ removed `{}`", p.display())),
                            Err(e) => app.set_status(format!("✗ {}", e)),
                        }
                    }
                    app.submodule_view.focus = SubmoduleFocus::List;
                    crate::tui::views::submodule::refresh(app);
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    app.submodule_view.focus = SubmoduleFocus::List;
                }
                _ => {}
            }
            None
        }
    }
}

pub(super) fn dispatch_submodule_op(app: &mut App) -> Option<Action> {
    use crate::tui::app::{SubmoduleFocus, SubmodulePendingOp};
    use std::path::Path;
    let ops = crate::tui::views::submodule::ops_for(&app.submodule_view);
    let label = ops
        .get(app.submodule_view.dropdown_idx)
        .map(|o| o.0)
        .unwrap_or("");
    let repo = Path::new(".");
    match label {
        "Add new submodule" => {
            app.submodule_view.focus = SubmoduleFocus::InputArgs;
            app.submodule_view.input_buffer.clear();
            app.submodule_view.input_prompt = "URL (e.g. https://github.com/u/r.git)".to_string();
            app.submodule_view.pending_op = SubmodulePendingOp::AddUrl;
        }
        "Update" => {
            let opts = crate::cmd::submodule::UpdateOpts {
                init: false,
                recursive: false,
            };
            match crate::cmd::submodule::update(repo, &opts) {
                Ok(_) => app.set_status("✓ submodules updated"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.submodule_view.focus = SubmoduleFocus::List;
            crate::tui::views::submodule::refresh(app);
        }
        "Update + init" => {
            let opts = crate::cmd::submodule::UpdateOpts {
                init: true,
                recursive: false,
            };
            match crate::cmd::submodule::update(repo, &opts) {
                Ok(_) => app.set_status("✓ submodules updated (with init)"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.submodule_view.focus = SubmoduleFocus::List;
            crate::tui::views::submodule::refresh(app);
        }
        "Init" => {
            match crate::cmd::submodule::init(repo, false) {
                Ok(_) => app.set_status("✓ submodules initialised"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.submodule_view.focus = SubmoduleFocus::List;
            crate::tui::views::submodule::refresh(app);
        }
        "Sync URLs" => {
            match crate::cmd::submodule::sync(repo) {
                Ok(_) => app.set_status("✓ submodule URLs synced"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.submodule_view.focus = SubmoduleFocus::List;
            crate::tui::views::submodule::refresh(app);
        }
        "Foreach <cmd>" => {
            app.submodule_view.focus = SubmoduleFocus::InputArgs;
            app.submodule_view.input_buffer.clear();
            app.submodule_view.input_prompt = "command (e.g. git status)".to_string();
            app.submodule_view.pending_op = SubmodulePendingOp::Foreach;
        }
        "Remove" => {
            app.submodule_view.focus = SubmoduleFocus::ConfirmRemove;
        }
        _ => {
            app.submodule_view.focus = SubmoduleFocus::List;
        }
    }
    None
}
