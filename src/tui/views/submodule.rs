//! `submodule` TUI view — list every registered submodule with its
//! HEAD vs working OID and the libgit2 state string. Operate via the
//! ops dropdown (`o`); each entry routes to `crate::cmd::submodule::*`.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::tui::app::{App, SubmoduleEntry, SubmoduleFocus, SubmoduleState};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_GREEN, C_RED, C_YELLOW};

pub fn refresh(app: &mut App) {
    let prev_focus = app.submodule_view.focus.clone();
    let prev_input = std::mem::take(&mut app.submodule_view.input_buffer);
    let prev_prompt = std::mem::take(&mut app.submodule_view.input_prompt);
    let prev_op = app.submodule_view.pending_op.clone();
    let prev_idx = app.submodule_view.dropdown_idx;
    let prev_url = std::mem::take(&mut app.submodule_view.pending_url);

    app.submodule_view.items.clear();
    app.submodule_view.status = None;
    app.submodule_view.focus = prev_focus;
    app.submodule_view.input_buffer = prev_input;
    app.submodule_view.input_prompt = prev_prompt;
    app.submodule_view.pending_op = prev_op;
    app.submodule_view.dropdown_idx = prev_idx;
    app.submodule_view.pending_url = prev_url;

    let repo = match git2::Repository::open(".") {
        Ok(r) => r,
        Err(e) => {
            app.submodule_view.status = Some(format!("open: {}", e));
            return;
        }
    };
    let subs = match repo.submodules() {
        Ok(s) => s,
        Err(e) => {
            app.submodule_view.status = Some(format!("submodules(): {}", e));
            return;
        }
    };
    for sm in &subs {
        let name = sm.name().unwrap_or("?").to_string();
        let state = describe_state(&repo, &name);
        app.submodule_view.items.push(SubmoduleEntry {
            name: name.clone(),
            path: sm.path().display().to_string(),
            url: sm.url().unwrap_or("(no url)").to_string(),
            head_oid: sm.head_id().map(|o| o.to_string()[..7].to_string()).unwrap_or_else(|| "—".to_string()),
            workdir_oid: sm.workdir_id().map(|o| o.to_string()[..7].to_string()).unwrap_or_else(|| "(not cloned)".to_string()),
            state,
        });
    }
    if app.submodule_view.idx >= app.submodule_view.items.len() {
        app.submodule_view.idx = app.submodule_view.items.len().saturating_sub(1);
    }
}

fn describe_state(repo: &git2::Repository, name: &str) -> String {
    let status = match repo.submodule_status(name, git2::SubmoduleIgnore::None) {
        Ok(s) => s, Err(_) => return "?".to_string(),
    };
    let mut parts = Vec::new();
    if !status.contains(git2::SubmoduleStatus::IN_WD) { parts.push("not initialised"); }
    if status.contains(git2::SubmoduleStatus::WD_MODIFIED) { parts.push("modified"); }
    if status.contains(git2::SubmoduleStatus::WD_INDEX_MODIFIED) { parts.push("staged"); }
    if status.contains(git2::SubmoduleStatus::WD_UNTRACKED) { parts.push("untracked"); }
    if parts.is_empty() { "clean".to_string() } else { parts.join(", ") }
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

    match app.submodule_view.focus {
        SubmoduleFocus::OpsDropdown   => render_ops_dropdown(f, app, area),
        SubmoduleFocus::InputArgs     => render_input_overlay(f, app, area),
        SubmoduleFocus::ConfirmRemove => render_confirm(f, app, area),
        SubmoduleFocus::List          => {}
    }
}

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;
    let items: Vec<ListItem> = if app.submodule_view.items.is_empty() {
        vec![ListItem::new(Span::styled("  no submodules", Style::default().fg(C_DIM)))]
    } else {
        app.submodule_view.items.iter().enumerate().map(|(i, s)| {
            let is_sel = i == app.submodule_view.idx;
            let style = if is_sel {
                Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
            } else { Style::default() };
            let color = if s.state == "clean" { C_GREEN } else { C_YELLOW };
            let marker = if s.workdir_oid.starts_with('(') { "○" } else { "●" };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", marker), Style::default().fg(bc)),
                Span::styled(format!("{:<22}", s.name), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {:<10}", s.workdir_oid), Style::default().fg(bc)),
                Span::styled(format!(" {}", s.state), Style::default().fg(color)),
            ])).style(style)
        }).collect()
    };
    let mut state = ListState::default();
    if !app.submodule_view.items.is_empty() { state.select(Some(app.submodule_view.idx)); }
    let title_color = if focused && app.submodule_view.focus == SubmoduleFocus::List { C_WHITE } else { bc };
    let block = Block::default()
        .title(Span::styled(
            format!(" submodules — {} ", app.submodule_view.items.len()),
            Style::default().fg(title_color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(title_color));
    f.render_stateful_widget(List::new(items).block(block), area, &mut state);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let body: Vec<Line> = if let Some(s) = app.submodule_view.items.get(app.submodule_view.idx) {
        vec![
            kv("name",    &s.name,        C_WHITE),
            kv("path",    &s.path,        C_DIM),
            kv("url",     &s.url,         C_DIM),
            kv("head",    &s.head_oid,    bc),
            kv("working", &s.workdir_oid, bc),
            kv("state",   &s.state,       if s.state == "clean" { C_GREEN } else { C_YELLOW }),
            Line::from(""),
            Line::from(Span::styled("  [o] open ops menu", Style::default().fg(C_DIM))),
        ]
    } else {
        vec![Line::from(Span::styled("  no selection", Style::default().fg(C_DIM)))]
    };
    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(Span::styled(" detail ", Style::default().fg(bc).add_modifier(Modifier::BOLD)))
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

/// Contextual ops; Remove disabled when there's nothing selected,
/// Init/Update always available, Add gates open the two-step input
/// flow (URL → path).
pub fn ops_for(state: &SubmoduleState) -> Vec<(&'static str, &'static str)> {
    let mut ops: Vec<(&'static str, &'static str)> = Vec::new();
    ops.push(("Add new submodule",   "clone a repo into a subdirectory"));
    ops.push(("Update",              "fetch + checkout to recorded commit"));
    ops.push(("Update + init",       "init missing, then update"));
    ops.push(("Init",                "register submodules without cloning"));
    ops.push(("Sync URLs",           "rewrite .git/config from .gitmodules"));
    ops.push(("Foreach <cmd>",       "run a shell command in each submodule"));
    if state.items.get(state.idx).is_some() {
        ops.push(("Remove",          "deregister + delete the working tree ⚠"));
    }
    ops
}

fn render_ops_dropdown(f: &mut Frame, app: &App, area: Rect) {
    let ops = ops_for(&app.submodule_view);
    if ops.is_empty() { return; }
    let bc = app.brand_color();

    let w: u16 = 54;
    let h: u16 = ops.len() as u16 + 2;
    let popup = Rect {
        x: area.x + 4, y: area.y + 4,
        width: w.min(area.width), height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = ops.iter().enumerate().map(|(i, (label, desc))| {
        let is_sel = i == app.submodule_view.dropdown_idx;
        let danger = label.starts_with("Remove");
        let label_color = if danger { C_RED } else if is_sel { C_WHITE } else { C_SUBTLE };
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        let prefix = if is_sel { "▶ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(bc)),
            Span::styled(format!("{:<22}", label), Style::default().fg(label_color)),
            Span::styled(*desc, Style::default().fg(C_DIM)),
        ])).style(style)
    }).collect();

    let mut state = ListState::default();
    state.select(Some(app.submodule_view.dropdown_idx));
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
        popup, &mut state,
    );
}

fn render_input_overlay(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let w: u16 = 70.min(area.width.saturating_sub(4));
    let h: u16 = 5;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w, height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let body = vec![
        Line::from(Span::styled(format!(" {}", app.submodule_view.input_prompt), Style::default().fg(C_WHITE))),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(&app.submodule_view.input_buffer, Style::default().fg(C_WHITE)),
            Span::styled("█", Style::default().fg(bc)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " input · Enter next/run · Esc cancel ",
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
    );
}

fn render_confirm(f: &mut Frame, app: &App, area: Rect) {
    let name = app.submodule_view.items.get(app.submodule_view.idx)
        .map(|s| s.name.clone()).unwrap_or_default();
    let w: u16 = 60;
    let h: u16 = 5;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w.min(area.width), height: h.min(area.height),
    };
    f.render_widget(Clear, popup);
    let body = vec![
        Line::from(Span::styled(format!("  Remove submodule `{}`?", name), Style::default().fg(C_WHITE))),
        Line::from(""),
        Line::from(Span::styled("  [y] yes   [n] no", Style::default().fg(C_DIM))),
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
