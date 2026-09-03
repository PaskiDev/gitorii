//! `worktree` TUI view — list every linked working copy with its branch
//! and dirty/clean state, and drive add/remove/lock/unlock/move/prune/
//! repair/open from the same ops dropdown pattern the Auth, Bisect and
//! Platform views use.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{App, WorktreeEntry, WorktreeFocus, WorktreeState};
use crate::tui::theme;

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
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // The list carries the rule; the detail pane sits the other side of it.
    let divider = theme::divider_right();
    let list_pane = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = [panes[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    render_list(f, app, list_pane);
    render_detail(f, app, panes[1]);

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
    let [heading_row, body] = theme::heading_and_body(area);
    let active = !app.sidebar_focused && app.worktree_view.focus == WorktreeFocus::List;

    let items: Vec<ListItem> = if app.worktree_view.items.is_empty() {
        vec![ListItem::new(Span::styled(
            "no worktrees",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        app.worktree_view
            .items
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let is_sel = active && i == app.worktree_view.idx;
                // The main working copy is the filled marker; the linked ones
                // are hollow.
                let marker = if w.is_main { "●" } else { "○" };
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        format!("{} ", marker),
                        Style::default().fg(theme::INK_FAINT),
                    ),
                    Span::styled(
                        format!("{:<22}", w.name),
                        Style::default()
                            .fg(if is_sel { theme::INK } else { theme::INK_DIM })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {:<18}", w.branch),
                        Style::default().fg(theme::INK_DIM),
                    ),
                    Span::styled(
                        format!(" {}", w.state),
                        Style::default().fg(state_color(&w.state)),
                    ),
                ]))
                .style(if is_sel {
                    Style::default()
                        .bg(theme::selection(app))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                })
            })
            .collect()
    };

    let mut state = ListState::default();
    if !app.worktree_view.items.is_empty() {
        state.select(Some(app.worktree_view.idx));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "worktrees",
        Some(app.worktree_view.items.len()),
        active,
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body_area] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("detail", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let body: Vec<Line> = match app.worktree_view.items.get(app.worktree_view.idx) {
        Some(w) => vec![
            kv("name", &w.name, theme::INK),
            kv("path", &w.path, theme::INK_FAINT),
            kv("branch", &w.branch, theme::INK_DIM),
            kv("state", &w.state, state_color(&w.state)),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("o", Style::default().fg(theme::accent(app))),
                Span::styled("  open ops menu", Style::default().fg(theme::INK_FAINT)),
            ]),
        ],
        None => vec![Line::from(Span::styled(
            "  no selection",
            Style::default().fg(theme::INK_FAINT),
        ))],
    };
    f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), body_area);
}

/// Clean is settled, locked is parked, anything else is work in progress.
fn state_color(state: &str) -> ratatui::style::Color {
    if state == "clean" {
        theme::OK
    } else if state.starts_with("locked") {
        theme::INK_FAINT
    } else {
        theme::WARN
    }
}

fn kv<'a>(k: &'a str, v: &str, vc: ratatui::style::Color) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {:<8} ", k),
            Style::default().fg(theme::INK_FAINT),
        ),
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
            let label_color = if label.starts_with("Remove") {
                theme::BAD
            } else if is_sel {
                theme::INK
            } else {
                theme::INK_DIM
            };
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(format!("{:<20}", label), Style::default().fg(label_color)),
                Span::styled(*desc, Style::default().fg(theme::INK_FAINT)),
            ]))
            .style(if is_sel {
                Style::default()
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.worktree_view.dropdown_idx));
    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    " ops — Enter run · Esc close ",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::RULE)),
        ),
        popup,
        &mut state,
    );
}

fn render_input_overlay(f: &mut Frame, app: &App, area: Rect) {
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
            Style::default().fg(theme::INK),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                &app.worktree_view.input_buffer,
                Style::default().fg(theme::INK),
            ),
            Span::styled("█", Style::default().fg(theme::accent(app))),
        ]),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " input · Enter run · Esc cancel ",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::RULE)),
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
            Style::default().fg(theme::INK),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("y", Style::default().fg(theme::accent(app))),
            Span::styled("  yes   ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("n", Style::default().fg(theme::accent(app))),
            Span::styled("  no", Style::default().fg(theme::INK_FAINT)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " confirm ",
                    Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::BAD)),
        ),
        popup,
    );
}
