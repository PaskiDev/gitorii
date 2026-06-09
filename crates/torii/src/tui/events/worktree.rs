//! Worktree view key handling.

use super::Action;
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

// 0.7.39 — Worktree view: contextual ops via `o` dropdown. Each op
// routes through `crate::cmd::worktree`. The view's own input/confirm
// overlays handle the parameters (branch name, lock reason, new path).
pub(super) fn handle_worktree(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{WorktreeFocus, WorktreePendingOp};
    use std::path::Path;

    match app.worktree_view.focus {
        WorktreeFocus::List => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.worktree_view.idx > 0 => {
                    app.worktree_view.idx -= 1;
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                    if app.worktree_view.idx + 1 < app.worktree_view.items.len() =>
                {
                    app.worktree_view.idx += 1;
                }
                (KeyModifiers::NONE, KeyCode::Char('o')) => {
                    app.worktree_view.dropdown_idx = 0;
                    app.worktree_view.focus = WorktreeFocus::OpsDropdown;
                }
                _ => {}
            }
            None
        }
        WorktreeFocus::OpsDropdown => {
            let ops = crate::tui::views::worktree::ops_for(&app.worktree_view);
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k'))
                    if app.worktree_view.dropdown_idx > 0 =>
                {
                    app.worktree_view.dropdown_idx -= 1;
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                    if app.worktree_view.dropdown_idx + 1 < ops.len() =>
                {
                    app.worktree_view.dropdown_idx += 1;
                }
                (_, KeyCode::Enter) => return dispatch_worktree_op(app),
                _ => {}
            }
            None
        }
        WorktreeFocus::InputArgs => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    let buf = std::mem::take(&mut app.worktree_view.input_buffer);
                    let op = app.worktree_view.pending_op.clone();
                    app.worktree_view.focus = WorktreeFocus::List;
                    app.worktree_view.pending_op = WorktreePendingOp::None;
                    let trimmed = buf.trim().to_string();
                    if trimmed.is_empty() {
                        app.set_status("✗ empty input, aborted");
                        return None;
                    }
                    let repo_path = std::path::PathBuf::from(".");
                    match op {
                        WorktreePendingOp::AddBranch => {
                            let spec = crate::cmd::worktree::BranchSpec::New(trimmed.clone());
                            let res = crate::cmd::worktree::add(
                                &repo_path,
                                spec,
                                &crate::cmd::worktree::AddOpts {
                                    explicit_path: None,
                                },
                            );
                            match res {
                                Ok(_) => {
                                    app.set_status(format!("✓ worktree added for `{}`", trimmed))
                                }
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                        }
                        WorktreePendingOp::LockReason => {
                            let target = app
                                .worktree_view
                                .items
                                .get(app.worktree_view.idx)
                                .map(|w| std::path::PathBuf::from(&w.path));
                            if let Some(t) = target {
                                match crate::cmd::worktree::lock(&repo_path, &t, Some(&trimmed)) {
                                    Ok(_) => app.set_status(format!("✓ locked `{}`", t.display())),
                                    Err(e) => app.set_status(format!("✗ {}", e)),
                                }
                            }
                        }
                        WorktreePendingOp::MoveNewPath => {
                            let cur = app
                                .worktree_view
                                .items
                                .get(app.worktree_view.idx)
                                .map(|w| std::path::PathBuf::from(&w.path));
                            if let Some(c) = cur {
                                let new = std::path::PathBuf::from(trimmed);
                                match crate::cmd::worktree::move_wt(&repo_path, &c, &new) {
                                    Ok(_) => {
                                        app.set_status(format!("✓ moved to `{}`", new.display()))
                                    }
                                    Err(e) => app.set_status(format!("✗ {}", e)),
                                }
                            }
                        }
                        WorktreePendingOp::None => {}
                    }
                    crate::tui::views::worktree::refresh(app);
                }
                (_, KeyCode::Backspace) => {
                    app.worktree_view.input_buffer.pop();
                }
                (_, KeyCode::Char(c)) if key.modifiers != KeyModifiers::CONTROL => {
                    app.worktree_view.input_buffer.push(c);
                }
                _ => {}
            }
            None
        }
        WorktreeFocus::ConfirmRemove => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    let target = app
                        .worktree_view
                        .items
                        .get(app.worktree_view.idx)
                        .map(|w| std::path::PathBuf::from(&w.path));
                    if let Some(t) = target {
                        let res = crate::cmd::worktree::remove(
                            Path::new("."),
                            &t,
                            &crate::cmd::worktree::RemoveOpts {
                                force: false,
                                no_snapshot: false,
                            },
                        );
                        match res {
                            Ok(_) => app.set_status(format!("✓ removed `{}`", t.display())),
                            Err(e) => app.set_status(format!("✗ {}", e)),
                        }
                    }
                    app.worktree_view.focus = WorktreeFocus::List;
                    crate::tui::views::worktree::refresh(app);
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    app.worktree_view.focus = WorktreeFocus::List;
                }
                _ => {}
            }
            None
        }
        WorktreeFocus::ConfirmPrune => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    match crate::cmd::worktree::prune(Path::new(".")) {
                        Ok(_) => app.set_status("✓ pruned"),
                        Err(e) => app.set_status(format!("✗ {}", e)),
                    }
                    app.worktree_view.focus = WorktreeFocus::List;
                    crate::tui::views::worktree::refresh(app);
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    app.worktree_view.focus = WorktreeFocus::List;
                }
                _ => {}
            }
            None
        }
    }
}

pub(super) fn dispatch_worktree_op(app: &mut App) -> Option<Action> {
    use crate::tui::app::{WorktreeFocus, WorktreePendingOp};
    use std::path::Path;
    let ops = crate::tui::views::worktree::ops_for(&app.worktree_view);
    let label = ops
        .get(app.worktree_view.dropdown_idx)
        .map(|o| o.0)
        .unwrap_or("");
    let repo = Path::new(".");
    match label {
        "Add new worktree" => {
            app.worktree_view.focus = WorktreeFocus::InputArgs;
            app.worktree_view.input_buffer.clear();
            app.worktree_view.input_prompt = "branch name to create (path auto)".to_string();
            app.worktree_view.pending_op = WorktreePendingOp::AddBranch;
        }
        "Open in $SHELL" => {
            // Doesn't make sense to leave the TUI mid-flight here —
            // open() spawns the shell against the worktree path; we
            // surface a hint pointing at the CLI for now since
            // suspending + restoring around an interactive shell is
            // a separate task.
            let path = app
                .worktree_view
                .items
                .get(app.worktree_view.idx)
                .map(|w| w.path.clone())
                .unwrap_or_default();
            app.set_status(format!("run from shell: torii worktree open {}", path));
            app.worktree_view.focus = WorktreeFocus::List;
        }
        "Lock" => {
            app.worktree_view.focus = WorktreeFocus::InputArgs;
            app.worktree_view.input_buffer.clear();
            app.worktree_view.input_prompt = "lock reason".to_string();
            app.worktree_view.pending_op = WorktreePendingOp::LockReason;
        }
        "Unlock" => {
            let target = app
                .worktree_view
                .items
                .get(app.worktree_view.idx)
                .map(|w| std::path::PathBuf::from(&w.path));
            if let Some(t) = target {
                match crate::cmd::worktree::unlock(repo, &t) {
                    Ok(_) => app.set_status(format!("✓ unlocked `{}`", t.display())),
                    Err(e) => app.set_status(format!("✗ {}", e)),
                }
            }
            app.worktree_view.focus = WorktreeFocus::List;
            crate::tui::views::worktree::refresh(app);
        }
        "Move" => {
            app.worktree_view.focus = WorktreeFocus::InputArgs;
            app.worktree_view.input_buffer.clear();
            app.worktree_view.input_prompt = "new path".to_string();
            app.worktree_view.pending_op = WorktreePendingOp::MoveNewPath;
        }
        "Remove" => {
            app.worktree_view.focus = WorktreeFocus::ConfirmRemove;
        }
        "Prune" => {
            app.worktree_view.focus = WorktreeFocus::ConfirmPrune;
        }
        "Repair" => {
            match crate::cmd::worktree::repair(repo) {
                Ok(_) => app.set_status("✓ repaired worktree links"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.worktree_view.focus = WorktreeFocus::List;
            crate::tui::views::worktree::refresh(app);
        }
        _ => {
            app.worktree_view.focus = WorktreeFocus::List;
        }
    }
    None
}
