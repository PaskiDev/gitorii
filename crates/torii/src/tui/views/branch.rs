//! The branch view: one list, grouped into local and remote.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::tui::app::App;
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;

    let [heading_row, body] = theme::heading_and_body(area);

    // ── Branch list ───────────────────────────────────────────────────────────
    let locals: Vec<(usize, &crate::tui::app::BranchEntry)> = app
        .branch_view
        .branches
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.is_remote)
        .collect();
    let remotes: Vec<(usize, &crate::tui::app::BranchEntry)> = app
        .branch_view
        .branches
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_remote)
        .collect();

    let mut items: Vec<ListItem> = vec![];

    if !locals.is_empty() {
        items.push(group_header("local"));
        for (i, b) in &locals {
            items.push(branch_item(app, *i, b));
        }
    }

    if !remotes.is_empty() {
        items.push(ListItem::new(Line::from(vec![Span::raw(" ")])));
        items.push(group_header("remote"));
        for (i, b) in &remotes {
            items.push(branch_item(app, *i, b));
        }
    }

    // Map logical idx to list position (account for header rows)
    let sel_list_pos = {
        let idx = app.branch_view.idx;
        let is_remote = app
            .branch_view
            .branches
            .get(idx)
            .map(|b| b.is_remote)
            .unwrap_or(false);
        if !is_remote {
            let pos_in_locals = locals.iter().position(|(i, _)| *i == idx).unwrap_or(0);
            1 + pos_in_locals // +1 for "local" header
        } else {
            let pos_in_remotes = remotes.iter().position(|(i, _)| *i == idx).unwrap_or(0);
            locals.len() + 3 + pos_in_remotes // locals header + locals + blank + remotes header
        }
    };

    let mut state = ListState::default();
    if !app.branch_view.branches.is_empty() {
        state.select(Some(sel_list_pos));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "branches",
        Some(app.branch_view.branches.len()),
        focused,
    ));
    heading.push(Span::styled(
        format!("  {} local  {} remote", locals.len(), remotes.len()),
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );

    // ── Ops dropdown overlay ──────────────────────────────────────────────────
    if app.branch_view.ops_mode {
        let push_disabled = app.branch_view.current_has_upstream;
        let selected = app.branch_view.branches.get(app.branch_view.idx);
        let can_delete = selected
            .map(|b| !b.is_current && !b.is_remote)
            .unwrap_or(false);

        let ops: &[(&str, bool)] = &[
            ("checkout", false),
            ("new branch", false),
            ("push", false),
            ("delete ⚠", true),
        ];

        let dropdown_w = 18u16;
        let dropdown_h = ops.len() as u16 + 2;
        let entry_y = body.y + sel_list_pos as u16 + 1;
        let drop_y = if entry_y + dropdown_h < body.y + body.height {
            entry_y
        } else {
            body.y + body.height.saturating_sub(dropdown_h)
        };
        let drop_area = Rect::new(body.x + 3, drop_y, dropdown_w, dropdown_h);

        let items: Vec<ListItem> = ops
            .iter()
            .enumerate()
            .map(|(i, (label, danger))| {
                let is_sel = i == app.branch_view.ops_idx;
                let dimmed = (i == 2 && push_disabled) || (i == 3 && !can_delete);
                let color = if dimmed {
                    theme::INK_FAINT
                } else if *danger {
                    theme::BAD
                } else if is_sel {
                    theme::INK
                } else {
                    theme::INK_DIM
                };
                let style = if is_sel && !dimmed {
                    Style::default()
                        .bg(theme::selection(app))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel && !dimmed),
                    Span::styled(*label, Style::default().fg(color)),
                ]))
                .style(style)
            })
            .collect();

        let mut drop_state = ListState::default();
        drop_state.select(Some(app.branch_view.ops_idx));

        // A popup keeps its box: it is a window, not a column.
        let drop_block = Block::default()
            .borders(Borders::ALL)
            .border_type(app.border_type())
            .border_style(Style::default().fg(theme::RULE));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(items).block(drop_block),
            drop_area,
            &mut drop_state,
        );
    }
}

/// The `local` / `remote` divider inside the list.
fn group_header(label: &str) -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled(
        label.to_string(),
        Style::default()
            .fg(theme::INK_FAINT)
            .add_modifier(Modifier::BOLD),
    )))
}

fn branch_item<'a>(app: &App, idx: usize, b: &'a crate::tui::app::BranchEntry) -> ListItem<'a> {
    let is_sel = idx == app.branch_view.idx;
    let style = if is_sel {
        Style::default()
            .bg(theme::selection(app))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let name_color = if b.is_current {
        theme::OK
    } else if is_sel {
        theme::INK
    } else {
        theme::INK_DIM
    };

    ListItem::new(Line::from(vec![
        theme::caret(app, is_sel),
        Span::styled(
            if b.is_current { "* " } else { "  " },
            Style::default().fg(theme::OK),
        ),
        Span::styled(b.name.clone(), Style::default().fg(name_color)),
    ]))
    .style(style)
}
