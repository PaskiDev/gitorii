use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::tui::app::App;
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_YELLOW, C_CYAN, C_RED};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // ── Tag list ──────────────────────────────────────────────────────────────
    let items: Vec<ListItem> = if app.tag_view.tags.is_empty() {
        vec![ListItem::new(Span::styled(
            "  no tags",
            Style::default().fg(C_DIM),
        ))]
    } else {
        app.tag_view.tags.iter().enumerate().map(|(i, t)| {
            let is_sel = i == app.tag_view.idx;
            let style = if is_sel {
                Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(format!("{:<20}", &t.name), Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {}", &t.time), Style::default().fg(C_DIM)),
            ])).style(style)
        }).collect()
    };

    let mut state = ListState::default();
    if !app.tag_view.tags.is_empty() { state.select(Some(app.tag_view.idx)); }

    let list_block = Block::default()
        .title(Span::styled(
            format!(" tags — {} ", app.tag_view.tags.len()),
            if focused { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if focused { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_stateful_widget(List::new(items).block(list_block), chunks[0], &mut state);

    // ── Info panel ────────────────────────────────────────────────────────────
    let info_lines: Vec<Line> = if let Some(t) = app.tag_view.tags.get(app.tag_view.idx) {
        vec![
            Line::from(vec![
                Span::styled("  name     ", Style::default().fg(C_SUBTLE)),
                Span::styled(&t.name, Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  commit   ", Style::default().fg(C_SUBTLE)),
                Span::styled(&t.hash, Style::default().fg(C_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  message  ", Style::default().fg(C_SUBTLE)),
                Span::styled(&t.message, Style::default().fg(C_WHITE)),
            ]),
            Line::from(vec![
                Span::styled("  age      ", Style::default().fg(C_SUBTLE)),
                Span::styled(&t.time, Style::default().fg(C_DIM)),
            ]),
        ]
    } else {
        vec![Line::from(Span::styled("  no tag selected", Style::default().fg(C_DIM)))]
    };

    let info_block = Block::default()
        .title(Span::styled(" info ", Style::default().fg(bc)))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(
        Paragraph::new(info_lines).block(info_block),
        chunks[1],
    );

    // ── Ops dropdown overlay ──────────────────────────────────────────────────
    if app.tag_view.ops_mode {
        const OPS: &[(&str, bool)] = &[
            ("push",       false),
            ("new tag",    false),
            ("delete ⚠",  true),
        ];
        let dropdown_w = 16u16;
        let dropdown_h = OPS.len() as u16 + 2;
        let entry_y = chunks[0].y + 1 + app.tag_view.idx as u16 + 1;
        let drop_y = if entry_y + dropdown_h < chunks[0].y + chunks[0].height {
            entry_y
        } else {
            chunks[0].y + chunks[0].height - dropdown_h
        };
        let drop_area = Rect::new(chunks[0].x + 3, drop_y, dropdown_w, dropdown_h);

        let items: Vec<ListItem> = OPS.iter().enumerate().map(|(i, (label, danger))| {
            let is_sel = i == app.tag_view.ops_idx;
            let color = if *danger { C_RED } else if is_sel { C_WHITE } else { C_SUBTLE };
            let prefix = if is_sel { "▶ " } else { "  " };
            let style = if is_sel {
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
        drop_state.select(Some(app.tag_view.ops_idx));

        let drop_block = Block::default()
            .borders(Borders::ALL).border_type(app.border_type())
            .border_style(Style::default().fg(bc));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(List::new(items).block(drop_block), drop_area, &mut drop_state);
    }
}
