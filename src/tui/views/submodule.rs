//! `submodule` TUI view — list every registered submodule with its
//! HEAD vs working OID and the state string libgit2 reports. Read-only
//! in 0.7.2; add/update/remove via `torii submodule …` for now.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::tui::app::{App, SubmoduleEntry};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_YELLOW, C_CYAN, C_GREEN};

pub fn refresh(app: &mut App) {
    app.submodule_view.items.clear();
    app.submodule_view.status = None;

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
            head_oid: sm
                .head_id()
                .map(|o| o.to_string()[..7].to_string())
                .unwrap_or_else(|| "—".to_string()),
            workdir_oid: sm
                .workdir_id()
                .map(|o| o.to_string()[..7].to_string())
                .unwrap_or_else(|| "(not cloned)".to_string()),
            state,
        });
    }
    if app.submodule_view.idx >= app.submodule_view.items.len() {
        app.submodule_view.idx = app.submodule_view.items.len().saturating_sub(1);
    }
}

fn describe_state(repo: &git2::Repository, name: &str) -> String {
    let status = match repo.submodule_status(name, git2::SubmoduleIgnore::None) {
        Ok(s) => s,
        Err(_) => return "?".to_string(),
    };
    let mut parts = Vec::new();
    if !status.contains(git2::SubmoduleStatus::IN_WD) {
        parts.push("not initialised");
    }
    if status.contains(git2::SubmoduleStatus::WD_MODIFIED) {
        parts.push("modified");
    }
    if status.contains(git2::SubmoduleStatus::WD_INDEX_MODIFIED) {
        parts.push("staged");
    }
    if status.contains(git2::SubmoduleStatus::WD_UNTRACKED) {
        parts.push("untracked");
    }
    if parts.is_empty() {
        "clean".to_string()
    } else {
        parts.join(", ")
    }
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let items: Vec<ListItem> = if app.submodule_view.items.is_empty() {
        vec![ListItem::new(Span::styled(
            "  no submodules",
            Style::default().fg(C_DIM),
        ))]
    } else {
        app.submodule_view
            .items
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let is_sel = i == app.submodule_view.idx;
                let style = if is_sel {
                    Style::default()
                        .bg(app.selected_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let color = if s.state == "clean" { C_GREEN } else { C_YELLOW };
                ListItem::new(Line::from(vec![
                    Span::styled(" 📦 ", Style::default().fg(bc)),
                    Span::styled(
                        format!("{:<22}", s.name),
                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" {:<10}", s.workdir_oid), Style::default().fg(C_CYAN)),
                    Span::styled(format!(" {}", s.state), Style::default().fg(color)),
                ]))
                .style(style)
            })
            .collect()
    };

    let mut state = ListState::default();
    if !app.submodule_view.items.is_empty() {
        state.select(Some(app.submodule_view.idx));
    }

    let list_block = Block::default()
        .title(Span::styled(
            format!(" submodules — {} ", app.submodule_view.items.len()),
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

    let info_lines: Vec<Line> =
        if let Some(s) = app.submodule_view.items.get(app.submodule_view.idx) {
            vec![
                Line::from(vec![
                    Span::styled("  name     ", Style::default().fg(C_SUBTLE)),
                    Span::styled(
                        &s.name,
                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  path     ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&s.path, Style::default().fg(C_WHITE)),
                ]),
                Line::from(vec![
                    Span::styled("  url      ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&s.url, Style::default().fg(C_WHITE)),
                ]),
                Line::from(vec![
                    Span::styled("  head     ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&s.head_oid, Style::default().fg(C_CYAN)),
                ]),
                Line::from(vec![
                    Span::styled("  working  ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&s.workdir_oid, Style::default().fg(C_CYAN)),
                ]),
                Line::from(vec![
                    Span::styled("  state    ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&s.state, Style::default().fg(C_WHITE)),
                ]),
                Line::from(vec![]),
                Line::from(vec![Span::styled(
                    "  CLI:",
                    Style::default().fg(C_SUBTLE).add_modifier(Modifier::BOLD),
                )]),
                Line::from(vec![Span::styled(
                    "    torii submodule add <url> <path>",
                    Style::default().fg(C_DIM),
                )]),
                Line::from(vec![Span::styled(
                    "    torii submodule update --init [--recursive]",
                    Style::default().fg(C_DIM),
                )]),
                Line::from(vec![Span::styled(
                    "    torii submodule foreach '<cmd>'",
                    Style::default().fg(C_DIM),
                )]),
                Line::from(vec![Span::styled(
                    "    torii submodule remove <path>",
                    Style::default().fg(C_DIM),
                )]),
            ]
        } else {
            vec![Line::from(Span::styled(
                "  no submodule selected",
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
