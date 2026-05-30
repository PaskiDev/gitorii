//! `bisect` TUI view — detect + drive an active `git bisect` session.
//!
//! Detects an active session by looking for `.git/BISECT_START` and the
//! `.git/BISECT_TERMS` / `BISECT_LOG` siblings that `git bisect` writes.
//! Operations (start, good, bad, skip, run, reset) are dispatched
//! through an ops dropdown — same chrome family as the Auth and
//! Platform views.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::tui::app::{App, BisectFocus, BisectState};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_GREEN, C_RED, C_YELLOW};

pub fn refresh(app: &mut App) {
    let prev_focus = app.bisect_view.focus.clone();
    let prev_input = std::mem::take(&mut app.bisect_view.input_buffer);
    let prev_prompt = std::mem::take(&mut app.bisect_view.input_prompt);
    let prev_op = app.bisect_view.pending_op.clone();
    let prev_idx = app.bisect_view.dropdown_idx;

    app.bisect_view = Default::default();
    app.bisect_view.focus = prev_focus;
    app.bisect_view.input_buffer = prev_input;
    app.bisect_view.input_prompt = prev_prompt;
    app.bisect_view.pending_op = prev_op;
    app.bisect_view.dropdown_idx = prev_idx;

    let repo = match git2::Repository::open(".") {
        Ok(r) => r,
        Err(e) => {
            app.bisect_view.status = Some(format!("open: {}", e));
            return;
        }
    };
    let gitdir = repo.path();
    let started = gitdir.join("BISECT_START");
    if !started.exists() {
        return;
    }
    app.bisect_view.in_progress = true;

    if let Ok(head) = repo.head() {
        if let Some(oid) = head.target() {
            app.bisect_view.current_hash = Some(format!("{}", &oid.to_string()[..8]));
        }
    }

    if let Ok(log) = std::fs::read_to_string(gitdir.join("BISECT_LOG")) {
        for line in log.lines() {
            if let Some(rest) = line.strip_prefix("# good: ") {
                app.bisect_view.good_refs.push(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("# bad: ") {
                app.bisect_view.bad_refs.push(rest.trim().to_string());
            }
        }
    }
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;
    let pv = &app.bisect_view;

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    render_status(f, app, cols[0]);
    render_detail(f, app, cols[1]);
    let _ = (bc, focused, pv);

    // Overlays — drawn last so they sit on top.
    match app.bisect_view.focus {
        BisectFocus::OpsDropdown  => render_ops_dropdown(f, app, area),
        BisectFocus::InputArgs    => render_input_overlay(f, app, area),
        BisectFocus::ConfirmReset => render_confirm_reset(f, app, area),
        BisectFocus::List         => {}
    }
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;
    let pv = &app.bisect_view;

    let mut lines: Vec<Line> = Vec::new();
    if pv.in_progress {
        lines.push(Line::from(vec![Span::styled(
            "  ● bisecting",
            Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));
        if let Some(h) = &pv.current_hash {
            lines.push(Line::from(vec![
                Span::styled("  testing   ", Style::default().fg(C_SUBTLE)),
                Span::styled(h.clone(), Style::default().fg(bc).add_modifier(Modifier::BOLD)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("  good      ", Style::default().fg(C_SUBTLE)),
            Span::styled(
                format!("{}", pv.good_refs.len()),
                Style::default().fg(C_GREEN),
            ),
            Span::styled(" ref(s)", Style::default().fg(C_DIM)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  bad       ", Style::default().fg(C_SUBTLE)),
            Span::styled(
                format!("{}", pv.bad_refs.len()),
                Style::default().fg(C_RED),
            ),
            Span::styled(" ref(s)", Style::default().fg(C_DIM)),
        ]));
        lines.push(Line::from(""));
        for r in pv.good_refs.iter().take(8) {
            lines.push(Line::from(vec![
                Span::styled("    ✓ ", Style::default().fg(C_GREEN)),
                Span::styled(short(r), Style::default().fg(C_DIM)),
            ]));
        }
        for r in pv.bad_refs.iter().take(8) {
            lines.push(Line::from(vec![
                Span::styled("    ✗ ", Style::default().fg(C_RED)),
                Span::styled(short(r), Style::default().fg(C_DIM)),
            ]));
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            "  no bisect in progress",
            Style::default().fg(C_DIM),
        )]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "  press [o] → Start to begin",
            Style::default().fg(C_SUBTLE),
        )]));
    }

    let title_color = if focused { C_WHITE } else { bc };
    let border_color = if focused { C_WHITE } else { bc };
    let block = Block::default()
        .title(Span::styled(
            " bisect ",
            Style::default().fg(title_color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(border_color));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let pv = &app.bisect_view;

    let mut body: Vec<Line> = Vec::new();
    if pv.in_progress {
        body.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("ops cheatsheet", Style::default().fg(C_DIM)),
        ]));
        body.push(Line::from(""));
        for (label, desc) in ops_for(pv) {
            body.push(Line::from(vec![
                Span::styled(format!("  {:<16}", label), Style::default().fg(bc)),
                Span::styled(desc, Style::default().fg(C_DIM)),
            ]));
        }
    } else {
        body.push(Line::from(Span::styled(
            "  Mark a known-bad and one or more known-good",
            Style::default().fg(C_WHITE),
        )));
        body.push(Line::from(Span::styled(
            "  commits. torii (via libgit2) bisects between",
            Style::default().fg(C_WHITE),
        )));
        body.push(Line::from(Span::styled(
            "  them, checks out a candidate, and asks you to",
            Style::default().fg(C_WHITE),
        )));
        body.push(Line::from(Span::styled(
            "  mark it good / bad / skip.",
            Style::default().fg(C_WHITE),
        )));
        body.push(Line::from(""));
        body.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("[o]", Style::default().fg(bc)),
            Span::styled(" → Start", Style::default().fg(C_DIM)),
        ]));
    }

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

/// Contextual ops for the current bisect state. List shrinks to the
/// only sensible action (Start) when no session is active.
pub fn ops_for(state: &BisectState) -> Vec<(&'static str, &'static str)> {
    if !state.in_progress {
        return vec![("Start", "begin a new bisect session")];
    }
    vec![
        ("Mark HEAD good",   "current commit is known-good"),
        ("Mark HEAD bad",    "current commit is known-bad"),
        ("Skip HEAD",        "untestable; pick another candidate"),
        ("Run command",      "auto-bisect via exit code (0=good, ≠0=bad, 125=skip)"),
        ("Reset",            "finish bisect + restore original HEAD ⚠"),
    ]
}

fn render_ops_dropdown(f: &mut Frame, app: &App, area: Rect) {
    let ops = ops_for(&app.bisect_view);
    if ops.is_empty() { return; }
    let bc = app.brand_color();

    let w: u16 = 50;
    let h: u16 = ops.len() as u16 + 2;
    let popup = Rect {
        x: area.x + 4,
        y: area.y + 4,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = ops.iter().enumerate().map(|(i, (label, desc))| {
        let is_sel = i == app.bisect_view.dropdown_idx;
        let danger = label.starts_with("Reset");
        let label_color = if danger { C_RED }
                          else if is_sel { C_WHITE }
                          else { C_SUBTLE };
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let prefix = if is_sel { "▶ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(bc)),
            Span::styled(format!("{:<18}", label), Style::default().fg(label_color)),
            Span::styled(*desc, Style::default().fg(C_DIM)),
        ])).style(style)
    }).collect();

    let mut state = ListState::default();
    state.select(Some(app.bisect_view.dropdown_idx));
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
            format!(" {}", app.bisect_view.input_prompt),
            Style::default().fg(C_WHITE),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(&app.bisect_view.input_buffer, Style::default().fg(C_WHITE)),
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

fn render_confirm_reset(f: &mut Frame, app: &App, area: Rect) {
    let w: u16 = 56;
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
            "  Reset bisect and restore original HEAD?",
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

fn short(s: &str) -> String {
    s.chars().take(40).collect()
}
