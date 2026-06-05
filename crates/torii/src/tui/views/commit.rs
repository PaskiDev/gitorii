use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::tui::app::{App, CommitFocus};
use crate::tui::events::COMMIT_TYPES;
use super::super::ui::{C_WHITE, C_SUBTLE, C_GREEN, C_DIM};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let in_list     = app.commit_view.focus == CommitFocus::List;
    let in_selector = app.commit_view.focus == CommitFocus::TypeSelector;
    let in_input    = app.commit_view.focus == CommitFocus::Input;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(COMMIT_TYPES.len() as u16 + 2),
            Constraint::Length(3),
        ])
        .split(area);

    let bc = app.brand_color();

    // ── Staged files ──────────────────────────────────────────────────────────
    let staged_items: Vec<ListItem> = if app.staged.is_empty() {
        vec![ListItem::new(Span::styled(
            "  no staged files — use [space] on files view to stage",
            Style::default().fg(C_DIM),
        ))]
    } else {
        app.staged.iter().map(|e| {
            ListItem::new(Line::from(vec![
                Span::styled("  + ", Style::default().fg(C_GREEN)),
                Span::styled(&e.path, Style::default().fg(C_WHITE)),
            ]))
        }).collect()
    };

    let staged_block = Block::default()
        .title(Span::styled(
            format!(" staged ({}) ", app.staged.len()),
            if in_list { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else       { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if in_list { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_widget(List::new(staged_items).block(staged_block), chunks[0]);

    // ── Type selector ─────────────────────────────────────────────────────────
    let type_items: Vec<ListItem> = COMMIT_TYPES.iter().enumerate().map(|(i, (prefix, desc))| {
        let is_sel = in_selector && i == app.commit_view.type_idx;
        let prefix_color = if is_sel { bc } else { C_SUBTLE };
        let desc_color   = if is_sel { C_WHITE } else { C_DIM };
        let indicator    = if is_sel { "▶ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(indicator, Style::default().fg(bc)),
            Span::styled(format!("{:<10}", prefix), Style::default().fg(prefix_color).add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() })),
            Span::styled(*desc, Style::default().fg(desc_color)),
        ])).style(if is_sel { Style::default().bg(app.selected_bg()) } else { Style::default() })
    }).collect();

    let type_block = Block::default()
        .title(Span::styled(
            " type  [i] skip ",
            if in_selector { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else           { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if in_selector { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_widget(List::new(type_items).block(type_block), chunks[1]);

    // ── Message input ─────────────────────────────────────────────────────────
    let msg    = &app.commit_view.message;
    let cursor = app.commit_view.cursor;
    let (before, after) = msg.split_at(cursor.min(msg.len()));
    let cursor_char = after.chars().next().unwrap_or(' ');
    let after_cursor = if after.is_empty() { "" } else { &after[cursor_char.len_utf8()..] };

    let input_line = if in_input {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(before, Style::default().fg(C_WHITE)),
            Span::styled(cursor_char.to_string(), Style::default().bg(app.selected_bg()).fg(C_WHITE).add_modifier(Modifier::BOLD)),
            Span::styled(after_cursor, Style::default().fg(C_WHITE)),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(msg.as_str(), Style::default().fg(C_DIM)),
        ])
    };

    let msg_block = Block::default()
        .title(Span::styled(
            " message ",
            if in_input { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else        { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if in_input { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_widget(Paragraph::new(input_line).block(msg_block), chunks[2]);
}
