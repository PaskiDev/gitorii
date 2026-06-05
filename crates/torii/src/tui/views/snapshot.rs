use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::tui::app::{App, AutoSnapshotInterval, SnapshotFocus};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_RED, C_YELLOW, C_GREEN};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let in_list   = app.snapshot_view.focus == SnapshotFocus::List;
    let in_create = app.snapshot_view.focus == SnapshotFocus::Create;
    let in_auto   = app.snapshot_view.focus == SnapshotFocus::AutoConfig;

    let bc = app.brand_color();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(AutoSnapshotInterval::all().len() as u16 + 2),
        ])
        .split(area);

    // ── Snapshot list ─────────────────────────────────────────────────────────
    let display_indices: Vec<usize> = if app.snapshot_view.filtered.is_empty() && app.snapshot_view.search_query.is_empty() {
        (0..app.snapshot_view.snapshots.len()).collect()
    } else {
        app.snapshot_view.filtered.clone()
    };

    let items: Vec<ListItem> = if app.snapshot_view.snapshots.is_empty() {
        vec![ListItem::new(Span::styled(
            "  no snapshots — press [n] to create one",
            Style::default().fg(C_DIM),
        ))]
    } else if display_indices.is_empty() {
        vec![ListItem::new(Span::styled(
            "  no matches",
            Style::default().fg(C_DIM),
        ))]
    } else {
        display_indices.iter().map(|&i| {
            let s = &app.snapshot_view.snapshots[i];
            let is_sel = in_list && app.snapshot_view.idx == display_indices.iter().position(|&x| x == i).unwrap_or(usize::MAX);
            let name_color = if !app.snapshot_view.search_query.is_empty() && !app.snapshot_view.filtered.is_empty() {
                super::super::ui::C_GREEN
            } else if is_sel { C_WHITE } else { C_SUBTLE };
            ListItem::new(Line::from(vec![
                Span::styled(if is_sel { "▶ " } else { "  " }, Style::default().fg(bc)),
                Span::styled(&s.name, Style::default().fg(name_color).add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled(format!("  {}", s.id), Style::default().fg(C_YELLOW)),
                Span::styled(format!("  {}", s.time), Style::default().fg(C_DIM)),
            ])).style(if is_sel { Style::default().bg(app.selected_bg()) } else { Style::default() })
        }).collect()
    };

    let mut state = ListState::default();
    if in_list && !display_indices.is_empty() { state.select(Some(app.snapshot_view.idx)); }

    let title = if app.snapshot_view.search_mode {
        format!(" snapshots — search: {}█ ", app.snapshot_view.search_query)
    } else if !app.snapshot_view.search_query.is_empty() {
        format!(" snapshots — \"{}\"  {} matches ", app.snapshot_view.search_query, display_indices.len())
    } else {
        format!(" snapshots ({}) ", app.snapshot_view.snapshots.len())
    };
    let title_color = if app.snapshot_view.search_mode { C_YELLOW } else if in_list { C_WHITE } else { bc };

    let list_block = Block::default()
        .title(Span::styled(title, Style::default().fg(title_color).add_modifier(
            if in_list || app.snapshot_view.search_mode { Modifier::BOLD } else { Modifier::empty() }
        )))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if in_list { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_stateful_widget(List::new(items).block(list_block), chunks[0], &mut state);

    // ── Ops dropdown overlay ──────────────────────────────────────────────────
    if app.snapshot_view.ops_mode && in_list {
        const OPS: &[(&str, bool)] = &[
            ("restore",    false),
            ("new",        false),
            ("delete ⚠",  true),
        ];
        let dropdown_w = 16u16;
        let dropdown_h = OPS.len() as u16 + 2;
        let entry_y = chunks[0].y + 1 + app.snapshot_view.idx as u16 + 1;
        let drop_y = if entry_y + dropdown_h < chunks[0].y + chunks[0].height {
            entry_y
        } else {
            chunks[0].y + chunks[0].height - dropdown_h
        };
        let drop_area = Rect::new(chunks[0].x + 3, drop_y, dropdown_w, dropdown_h);

        let drop_items: Vec<ListItem> = OPS.iter().enumerate().map(|(i, (label, danger))| {
            let is_sel = i == app.snapshot_view.ops_idx;
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
        drop_state.select(Some(app.snapshot_view.ops_idx));

        let drop_block = Block::default()
            .borders(Borders::ALL).border_type(app.border_type())
            .border_style(Style::default().fg(bc));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(List::new(drop_items).block(drop_block), drop_area, &mut drop_state);
    }

    // ── Create input ──────────────────────────────────────────────────────────
    let name = &app.snapshot_view.create_name;
    let input_line = if in_create {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(name.as_str(), Style::default().fg(C_WHITE)),
            Span::styled("█", Style::default().fg(bc)),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(if name.is_empty() { "snapshot name..." } else { name.as_str() },
                Style::default().fg(C_DIM)),
        ])
    };
    let create_block = Block::default()
        .title(Span::styled(
            " new snapshot ",
            if in_create { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) } else { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if in_create { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_widget(Paragraph::new(input_line).block(create_block), chunks[1]);

    // ── Auto-snapshot config ──────────────────────────────────────────────────
    let intervals = AutoSnapshotInterval::all();
    let auto_items: Vec<ListItem> = intervals.iter().enumerate().map(|(i, interval)| {
        let is_sel     = in_auto && i == app.snapshot_view.auto_interval_idx;
        let is_current = *interval == app.snapshot_view.auto_interval;
        let indicator  = if is_sel { "▶ " } else { "  " };
        let check      = if is_current { "✓ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(indicator, Style::default().fg(bc)),
            Span::styled(check, Style::default().fg(C_GREEN)),
            Span::styled(interval.label(), Style::default()
                .fg(if is_sel { C_WHITE } else { C_SUBTLE })
                .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() })),
        ])).style(if is_sel { Style::default().bg(app.selected_bg()) } else { Style::default() })
    }).collect();

    let auto_block = Block::default()
        .title(Span::styled(
            format!(" auto-snapshot  [current: {}] ", app.snapshot_view.auto_interval.label()),
            if in_auto { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) } else { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if in_auto { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_widget(List::new(auto_items).block(auto_block), chunks[2]);
}
