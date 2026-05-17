//! `worktree` TUI view — list every linked working copy with its branch
//! and dirty/clean state. Refreshed on entry via [`refresh`].
//!
//! No ops keybinds yet (0.7.2 ships informative-only); add/remove/lock
//! will land alongside the first interactive sweep of the TUI in 0.7.3.
//! `torii worktree …` from the shell remains the canonical write path.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::tui::app::{App, WorktreeEntry};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_YELLOW, C_CYAN, C_GREEN};

/// Read current worktree state from the on-disk repo and stuff it into
/// `app.worktree_view`. Cheap to call — done on view entry.
pub fn refresh(app: &mut App) {
    app.worktree_view.items.clear();
    app.worktree_view.status = None;

    let repo = match git2::Repository::open(".") {
        Ok(r) => r,
        Err(e) => {
            app.worktree_view.status = Some(format!("open: {}", e));
            return;
        }
    };

    // Main worktree first.
    if let Some(wd) = repo.workdir() {
        let path = wd.canonicalize().unwrap_or_else(|_| wd.to_path_buf());
        let (branch, state) = describe(&path);
        app.worktree_view.items.push(WorktreeEntry {
            name: "(main)".to_string(),
            path: path.display().to_string(),
            branch,
            state,
            is_main: true,
        });
    }

    // Linked worktrees.
    if let Ok(names) = repo.worktrees() {
        for i in 0..names.len() {
            let name = match names.get(i) {
                Some(n) => n,
                None => continue,
            };
            let wt = match repo.find_worktree(name) {
                Ok(w) => w,
                Err(_) => continue,
            };
            let path = wt
                .path()
                .canonicalize()
                .unwrap_or_else(|_| wt.path().to_path_buf());
            let (branch, mut state) = describe(&path);
            if let Ok(git2::WorktreeLockStatus::Locked(reason)) = wt.is_locked() {
                let suffix = reason.unwrap_or_else(|| "(no reason)".to_string());
                state = format!("locked: {suffix}");
            }
            app.worktree_view.items.push(WorktreeEntry {
                name: name.to_string(),
                path: path.display().to_string(),
                branch,
                state,
                is_main: false,
            });
        }
    }
    if app.worktree_view.idx >= app.worktree_view.items.len() {
        app.worktree_view.idx = app.worktree_view.items.len().saturating_sub(1);
    }
}

/// Best-effort branch + clean/dirty summary by opening the linked
/// worktree directory as its own repo. Failures degrade silently — the
/// view is informative, not load-bearing.
fn describe(path: &std::path::Path) -> (String, String) {
    let repo = match git2::Repository::open(path) {
        Ok(r) => r,
        Err(_) => return ("?".to_string(), "?".to_string()),
    };
    let branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "(detached)".to_string());
    let mut so = git2::StatusOptions::new();
    so.include_untracked(true).include_ignored(false);
    let dirty = repo
        .statuses(Some(&mut so))
        .ok()
        .map(|ss| {
            ss.iter()
                .filter(|s| !s.status().contains(git2::Status::IGNORED))
                .count()
        })
        .unwrap_or(0);
    let state = if dirty == 0 {
        "clean".to_string()
    } else {
        format!("{} change(s)", dirty)
    };
    (branch, state)
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // ── List ─────────────────────────────────────────────────────────────
    let items: Vec<ListItem> = if app.worktree_view.items.is_empty() {
        vec![ListItem::new(Span::styled(
            "  no worktrees",
            Style::default().fg(C_DIM),
        ))]
    } else {
        app.worktree_view
            .items
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let is_sel = i == app.worktree_view.idx;
                let style = if is_sel {
                    Style::default()
                        .bg(app.selected_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let icon = if w.is_main { "📍" } else { "🌳" };
                let state_color = if w.state == "clean" {
                    C_GREEN
                } else if w.state.starts_with("locked") {
                    C_YELLOW
                } else {
                    C_YELLOW
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {icon} "), Style::default().fg(bc)),
                    Span::styled(
                        format!("{:<22}", w.name),
                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" {:<18}", w.branch), Style::default().fg(C_CYAN)),
                    Span::styled(format!(" {}", w.state), Style::default().fg(state_color)),
                ]))
                .style(style)
            })
            .collect()
    };

    let mut state = ListState::default();
    if !app.worktree_view.items.is_empty() {
        state.select(Some(app.worktree_view.idx));
    }

    let list_block = Block::default()
        .title(Span::styled(
            format!(" worktrees — {} ", app.worktree_view.items.len()),
            if focused {
                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(bc)
            },
        ))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(if focused {
            Style::default().fg(C_WHITE)
        } else {
            Style::default().fg(bc)
        });
    f.render_stateful_widget(List::new(items).block(list_block), chunks[0], &mut state);

    // ── Info panel ───────────────────────────────────────────────────────
    let info_lines: Vec<Line> = if let Some(w) = app.worktree_view.items.get(app.worktree_view.idx)
    {
        vec![
            Line::from(vec![
                Span::styled("  name    ", Style::default().fg(C_SUBTLE)),
                Span::styled(&w.name, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  path    ", Style::default().fg(C_SUBTLE)),
                Span::styled(&w.path, Style::default().fg(C_WHITE)),
            ]),
            Line::from(vec![
                Span::styled("  branch  ", Style::default().fg(C_SUBTLE)),
                Span::styled(&w.branch, Style::default().fg(C_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  state   ", Style::default().fg(C_SUBTLE)),
                Span::styled(&w.state, Style::default().fg(C_WHITE)),
            ]),
            Line::from(vec![]),
            Line::from(vec![Span::styled(
                "  CLI:",
                Style::default().fg(C_SUBTLE).add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                "    torii worktree add -b <branch>",
                Style::default().fg(C_DIM),
            )]),
            Line::from(vec![Span::styled(
                "    torii worktree remove <path>",
                Style::default().fg(C_DIM),
            )]),
            Line::from(vec![Span::styled(
                "    torii worktree lock <path> -r <reason>",
                Style::default().fg(C_DIM),
            )]),
            Line::from(vec![Span::styled(
                "    torii worktree open <path>",
                Style::default().fg(C_DIM),
            )]),
        ]
    } else {
        vec![Line::from(Span::styled(
            "  no worktree selected",
            Style::default().fg(C_DIM),
        ))]
    };

    let info_block = Block::default()
        .title(Span::styled(" info ", Style::default().fg(bc)))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(Paragraph::new(info_lines).block(info_block), chunks[1]);
}
