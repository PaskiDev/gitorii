use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

use crate::tui::app::App;
use super::super::ui::{C_WHITE, C_SUBTLE, C_GREEN, C_RED};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1)])
        .split(area);

    // ── Branch list ───────────────────────────────────────────────────────────
    let locals: Vec<(usize, &crate::tui::app::BranchEntry)> = app.branch_view.branches
        .iter().enumerate().filter(|(_, b)| !b.is_remote).collect();
    let remotes: Vec<(usize, &crate::tui::app::BranchEntry)> = app.branch_view.branches
        .iter().enumerate().filter(|(_, b)| b.is_remote).collect();

    let mut items: Vec<ListItem> = vec![];

    if !locals.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(" local ", Style::default().fg(C_SUBTLE).add_modifier(Modifier::BOLD)),
        ])));
        for (i, b) in &locals {
            items.push(branch_item(app, *i, b, bc));
        }
    }

    if !remotes.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::raw(" "),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::styled(" remote ", Style::default().fg(C_SUBTLE).add_modifier(Modifier::BOLD)),
        ])));
        for (i, b) in &remotes {
            items.push(branch_item(app, *i, b, bc));
        }
    }

    // Map logical idx to list position (account for header rows)
    let sel_list_pos = {
        let idx = app.branch_view.idx;
        let is_remote = app.branch_view.branches.get(idx).map(|b| b.is_remote).unwrap_or(false);
        if !is_remote {
            let pos_in_locals = locals.iter().position(|(i, _)| *i == idx).unwrap_or(0);
            1 + pos_in_locals // +1 for "local" header
        } else {
            let pos_in_remotes = remotes.iter().position(|(i, _)| *i == idx).unwrap_or(0);
            locals.len() + 3 + pos_in_remotes // locals header + locals + blank + remotes header
        }
    };

    let local_count = locals.len();
    let remote_count = remotes.len();
    let title = format!(" branches — {} local  {} remote ", local_count, remote_count);

    let mut state = ListState::default();
    if !app.branch_view.branches.is_empty() { state.select(Some(sel_list_pos)); }

    let list_block = Block::default()
        .title(Span::styled(title,
            if focused { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(bc) }
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if focused { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_stateful_widget(List::new(items).block(list_block), chunks[0], &mut state);

    // ── Ops dropdown overlay ──────────────────────────────────────────────────
    if app.branch_view.ops_mode {
        let push_disabled = app.branch_view.current_has_upstream;
        let selected = app.branch_view.branches.get(app.branch_view.idx);
        let can_delete = selected.map(|b| !b.is_current && !b.is_remote).unwrap_or(false);

        let ops: &[(&str, bool)] = &[
            ("checkout",    false),
            ("new branch",  false),
            ("push",        false),
            ("delete ⚠",    true),
        ];

        let dropdown_w = 18u16;
        let dropdown_h = ops.len() as u16 + 2;
        let entry_y = chunks[0].y + 1 + sel_list_pos as u16 + 1;
        let drop_y = if entry_y + dropdown_h < chunks[0].y + chunks[0].height {
            entry_y
        } else {
            chunks[0].y + chunks[0].height - dropdown_h
        };
        let drop_area = Rect::new(chunks[0].x + 3, drop_y, dropdown_w, dropdown_h);

        let items: Vec<ListItem> = ops.iter().enumerate().map(|(i, (label, danger))| {
            let is_sel = i == app.branch_view.ops_idx;
            let dimmed = (i == 2 && push_disabled) || (i == 3 && !can_delete);
            let color = if dimmed { super::super::ui::C_DIM }
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
        drop_state.select(Some(app.branch_view.ops_idx));

        let drop_block = Block::default()
            .borders(Borders::ALL).border_type(app.border_type())
            .border_style(Style::default().fg(bc));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(List::new(items).block(drop_block), drop_area, &mut drop_state);
    }
}

fn branch_item<'a>(
    app: &App,
    idx: usize,
    b: &'a crate::tui::app::BranchEntry,
    bc: ratatui::style::Color,
) -> ListItem<'a> {
    let is_sel = idx == app.branch_view.idx;
    let style = if is_sel {
        Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let prefix = if is_sel { "█ " } else { "  " };
    let current = if b.is_current { "* " } else { "  " };
    let name_color = if b.is_current { C_GREEN } else if is_sel { C_WHITE } else { C_SUBTLE };

    ListItem::new(Line::from(vec![
        Span::styled(prefix, Style::default().fg(bc)),
        Span::styled(current, Style::default().fg(C_GREEN)),
        Span::styled(b.name.clone(), Style::default().fg(name_color)),
    ])).style(style)
}
