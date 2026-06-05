//! `worktree` TUI view — list every linked working copy with its branch
//! and dirty/clean state, and drive add/remove/lock/unlock/move/prune/
//! repair/open from the same ops dropdown pattern the Auth, Bisect and
//! Platform views use.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::super::ui::{C_DIM, C_GREEN, C_RED, C_SUBTLE, C_WHITE, C_YELLOW};
use crate::tui::app::{App, WorktreeEntry, WorktreeFocus, WorktreeState};

pub fn refresh(app: &mut App) {
    let prev_focus = app.worktree_view.focus.clone();
    let prev_input = std::mem::take(&mut app.worktree_view.input_buffer);
    let prev_prompt = std::mem::take(&mut app.worktree_view.input_prompt);
    let prev_op = app.worktree_view.pending_op.clone();
    let prev_idx = app.worktree_view.dropdown_idx;

    app.worktree_view.items.clear();
    app.worktree_view.status = None;
    app.worktree_view.focus = prev_focus;
    app.worktree_view.input_buffer = prev_input;
    app.worktree_view.input_prompt = prev_prompt;
    app.worktree_view.pending_op = prev_op;
    app.worktree_view.dropdown_idx = prev_idx;

    let repo = match git2::Repository::open(".") {
        Ok(r) => r,
        Err(e) => {
            app.worktree_view.status = Some(format!("open: {}", e));
            return;
        }
    };

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

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    render_list(f, app, cols[0]);
    render_detail(f, app, cols[1]);
    let _ = (bc, focused);

    match app.worktree_view.focus {
        WorktreeFocus::OpsDropdown => render_ops_dropdown(f, app, area),
        WorktreeFocus::InputArgs => render_input_overlay(f, app, area),
        WorktreeFocus::ConfirmRemove => {
            render_confirm(f, app, area, "Remove the selected worktree?")
        }
        WorktreeFocus::ConfirmPrune => {
            render_confirm(f, app, area, "Prune stale worktree admin dirs?")
        }
        WorktreeFocus::List => {}
    }
}

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;
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
                let marker = if w.is_main { "●" } else { "○" };
                let state_color = if w.state == "clean" {
                    C_GREEN
                } else if w.state.starts_with("locked") {
                    C_DIM
                } else {
                    C_YELLOW
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {} ", marker), Style::default().fg(bc)),
                    Span::styled(
                        format!("{:<22}", w.name),
                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" {:<18}", w.branch), Style::default().fg(bc)),
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
    let title_color = if focused && app.worktree_view.focus == WorktreeFocus::List {
        C_WHITE
    } else {
        bc
    };
    let block = Block::default()
        .title(Span::styled(
            format!(" worktrees — {} ", app.worktree_view.items.len()),
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(title_color));
    f.render_stateful_widget(List::new(items).block(block), area, &mut state);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let body: Vec<Line> = if let Some(w) = app.worktree_view.items.get(app.worktree_view.idx) {
        vec![
            kv("name", &w.name, C_WHITE),
            kv("path", &w.path, C_DIM),
            kv("branch", &w.branch, bc),
            kv(
                "state",
                &w.state,
                if w.state == "clean" {
                    C_GREEN
                } else {
                    C_YELLOW
                },
            ),
            Line::from(""),
            Line::from(Span::styled(
                "  [o] open ops menu",
                Style::default().fg(C_DIM),
            )),
        ]
    } else {
        vec![Line::from(Span::styled(
            "  no selection",
            Style::default().fg(C_DIM),
        ))]
    };
    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(Span::styled(
                    " detail ",
                    Style::default().fg(bc).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(bc)),
        ),
        area,
    );
}

fn kv<'a>(k: &'a str, v: &str, vc: ratatui::style::Color) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {:<8} ", k), Style::default().fg(C_SUBTLE)),
        Span::styled(v.to_string(), Style::default().fg(vc)),
    ])
}

/// Contextual ops; shrinks/widens based on the selected entry. The main
/// worktree refuses Remove/Lock/Move (those don't make sense on it).
pub fn ops_for(state: &WorktreeState) -> Vec<(&'static str, &'static str)> {
    let is_main = state
        .items
        .get(state.idx)
        .map(|w| w.is_main)
        .unwrap_or(false);
    let locked = state
        .items
        .get(state.idx)
        .map(|w| w.state.starts_with("locked"))
        .unwrap_or(false);

    let mut ops: Vec<(&'static str, &'static str)> = Vec::new();
    ops.push((
        "Add new worktree",
        "create + check out a branch in a sibling dir",
    ));
    ops.push(("Open in $SHELL", "cd into the worktree, suspend the TUI"));
    if !is_main {
        if locked {
            ops.push(("Unlock", "drop the lock so prune/remove can act on it"));
        } else {
            ops.push(("Lock", "mark as locked; prune skips it"));
        }
        ops.push(("Move", "rename the worktree directory"));
        ops.push(("Remove", "delete the worktree (+ its branch ref) ⚠"));
    }
    ops.push(("Prune", "drop admin entries for missing worktrees"));
    ops.push(("Repair", "fix broken back-pointers (after moves)"));
    ops
}

fn render_ops_dropdown(f: &mut Frame, app: &App, area: Rect) {
    let ops = ops_for(&app.worktree_view);
    if ops.is_empty() {
        return;
    }
    let bc = app.brand_color();

    let w: u16 = 54;
    let h: u16 = ops.len() as u16 + 2;
    let popup = Rect {
        x: area.x + 4,
        y: area.y + 4,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = ops
        .iter()
        .enumerate()
        .map(|(i, (label, desc))| {
            let is_sel = i == app.worktree_view.dropdown_idx;
            let danger = label.starts_with("Remove");
            let label_color = if danger {
                C_RED
            } else if is_sel {
                C_WHITE
            } else {
                C_SUBTLE
            };
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(format!("{:<20}", label), Style::default().fg(label_color)),
                Span::styled(*desc, Style::default().fg(C_DIM)),
            ]))
            .style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.worktree_view.dropdown_idx));
    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    " ops — Enter run · Esc close ",
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
        &mut state,
    );
}

fn render_input_overlay(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let w: u16 = 70.min(area.width.saturating_sub(4));
    let h: u16 = 5;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let body = vec![
        Line::from(Span::styled(
            format!(" {}", app.worktree_view.input_prompt),
            Style::default().fg(C_WHITE),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                &app.worktree_view.input_buffer,
                Style::default().fg(C_WHITE),
            ),
            Span::styled("█", Style::default().fg(bc)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " input · Enter run · Esc cancel ",
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
    );
}

fn render_confirm(f: &mut Frame, app: &App, area: Rect, prompt: &str) {
    let w: u16 = 60;
    let h: u16 = 5;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);
    let body = vec![
        Line::from(Span::styled(
            format!("  {}", prompt),
            Style::default().fg(C_WHITE),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  [y] yes   [n] no",
            Style::default().fg(C_DIM),
        )),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " confirm ",
                    Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_RED)),
        ),
        popup,
    );
}
