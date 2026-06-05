#![allow(dead_code)]

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::tui::app::App;
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_CYAN, C_YELLOW, C_GREEN, C_RED};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // ── Mirror list ───────────────────────────────────────────────────────────
    let items: Vec<ListItem> = if app.mirror_view.mirrors.is_empty() {
        vec![ListItem::new(Span::styled(
            "  no mirrors configured",
            Style::default().fg(C_DIM),
        ))]
    } else {
        app.mirror_view.mirrors.iter().enumerate().map(|(i, m)| {
            let is_sel = i == app.mirror_view.idx;
            let style = if is_sel {
                Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            let kind_color = if m.kind == "primary" { C_YELLOW } else { C_SUBTLE };
            let platform_color = platform_color(&m.platform);
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(format!("{:<10} ", &m.name), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<10}", &m.kind), Style::default().fg(kind_color)),
                Span::styled(format!("{:<10}", &m.platform), Style::default().fg(platform_color)),
                Span::styled(truncate(&m.url, 30), Style::default().fg(C_DIM)),
            ])).style(style)
        }).collect()
    };

    let mut state = ListState::default();
    if !app.mirror_view.mirrors.is_empty() { state.select(Some(app.mirror_view.idx)); }

    let list_block = Block::default()
        .title(Span::styled(
            format!(" mirrors — {} ", app.mirror_view.mirrors.len()),
            if focused { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if focused { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_stateful_widget(List::new(items).block(list_block), chunks[0], &mut state);

    // ── Info panel ────────────────────────────────────────────────────────────
    let info_lines: Vec<Line> = if let Some(m) = app.mirror_view.mirrors.get(app.mirror_view.idx) {
        let https_url = ssh_to_https(&m.url);
        vec![
            Line::from(vec![
                Span::styled("  name      ", Style::default().fg(C_SUBTLE)),
                Span::styled(&m.name, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  kind      ", Style::default().fg(C_SUBTLE)),
                Span::styled(&m.kind, Style::default().fg(
                    if m.kind == "primary" { C_YELLOW } else { C_SUBTLE }
                ).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  platform  ", Style::default().fg(C_SUBTLE)),
                Span::styled(&m.platform, Style::default().fg(platform_color(&m.platform))),
            ]),
            Line::from(vec![
                Span::styled("  account   ", Style::default().fg(C_SUBTLE)),
                Span::styled(&m.account, Style::default().fg(C_WHITE)),
            ]),
            Line::from(vec![
                Span::styled("  repo      ", Style::default().fg(C_SUBTLE)),
                Span::styled(&m.repo, Style::default().fg(C_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  url       ", Style::default().fg(C_SUBTLE)),
                Span::styled(&m.url, Style::default().fg(C_DIM)),
            ]),
            Line::from(vec![
                Span::styled("  https     ", Style::default().fg(C_SUBTLE)),
                Span::styled(https_url, Style::default().fg(C_DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    if let Some(s) = &app.mirror_view.status {
                        format!("  {}", s)
                    } else {
                        "  ready".to_string()
                    },
                    Style::default().fg(if app.mirror_view.status.is_some() { C_GREEN } else { C_DIM }),
                ),
            ]),
        ]
    } else {
        vec![
            Line::from(Span::styled("  no mirror selected", Style::default().fg(C_DIM))),
            Line::from(""),
            Line::from(vec![
                Span::styled("  add with: ", Style::default().fg(C_DIM)),
                Span::styled("torii mirror add", Style::default().fg(C_SUBTLE)),
            ]),
        ]
    };

    let info_block = Block::default()
        .title(Span::styled(" info ", Style::default().fg(bc)))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(Paragraph::new(info_lines).block(info_block), chunks[1]);

    // ── Ops dropdown overlay ──────────────────────────────────────────────────
    if app.mirror_view.ops_mode {
        const OPS: &[(&str, bool)] = &[
            ("sync all",    false),
            ("force sync",  false),
            ("remove ⚠",   true),
        ];
        let dropdown_w = 16u16;
        let dropdown_h = OPS.len() as u16 + 2;
        let entry_y = chunks[0].y + 1 + app.mirror_view.idx as u16 + 1;
        let drop_y = if entry_y + dropdown_h < chunks[0].y + chunks[0].height {
            entry_y
        } else {
            chunks[0].y + chunks[0].height - dropdown_h
        };
        let drop_area = Rect::new(chunks[0].x + 3, drop_y, dropdown_w, dropdown_h);

        let drop_items: Vec<ListItem> = OPS.iter().enumerate().map(|(i, (label, danger))| {
            let is_sel = i == app.mirror_view.ops_idx;
            let no_mirror = app.mirror_view.mirrors.is_empty();
            let dimmed = no_mirror;
            let color = if dimmed { C_DIM }
                else if *danger { C_RED }
                else if is_sel { C_WHITE }
                else { C_SUBTLE };
            let prefix = if is_sel { "▶ " } else { "  " };
            let style = if is_sel && !dimmed {
                Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(*label, Style::default().fg(color)),
            ])).style(style)
        }).collect();

        let mut drop_state = ListState::default();
        drop_state.select(Some(app.mirror_view.ops_idx));

        let drop_block = Block::default()
            .borders(Borders::ALL).border_type(app.border_type())
            .border_style(Style::default().fg(bc));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(List::new(drop_items).block(drop_block), drop_area, &mut drop_state);
    }
}

fn platform_color(platform: &str) -> ratatui::style::Color {
    match platform.to_lowercase().as_str() {
        "github"    => C_WHITE,
        "gitlab"    => C_YELLOW,
        "bitbucket" => C_CYAN,
        "codeberg"  => C_GREEN,
        _           => C_DIM,
    }
}

fn ssh_to_https(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("git@") {
        let s = rest.replacen(':', "/", 1);
        let s = s.strip_suffix(".git").unwrap_or(&s);
        return format!("https://{}", s);
    }
    url.strip_suffix(".git").unwrap_or(url).to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { return s.to_string(); }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut)
}
