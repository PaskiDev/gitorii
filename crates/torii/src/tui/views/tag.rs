//! The tag view: the tags, and what the selected one points at.
//!
//! Two panes parted by a rule rather than two boxes, the way the log view is
//! laid out. The ops dropdown keeps its box: a popup is a window.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::tui::app::App;
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // The list carries the rule; the info pane sits the other side of it, and
    // the rule reaches into the chrome above and below.
    let divider = theme::divider_right();
    let list_pane = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = [panes[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    let [list_heading, list_body] = theme::heading_and_body(list_pane);
    let [info_heading, info_body] = theme::heading_and_body(panes[1]);

    // ── Tag list ──────────────────────────────────────────────────────────────
    let items: Vec<ListItem> = if app.tag_view.tags.is_empty() {
        vec![ListItem::new(Span::styled(
            "no tags",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        app.tag_view
            .tags
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let is_sel = focused && i == app.tag_view.idx;
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        format!("{:<20}", &t.name),
                        Style::default()
                            .fg(theme::WARN)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {}", &t.time),
                        Style::default().fg(theme::INK_FAINT),
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
    if !app.tag_view.tags.is_empty() {
        state.select(Some(app.tag_view.idx));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "tags",
        Some(app.tag_view.tags.len()),
        focused,
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), list_heading);

    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        list_body,
        &mut state,
    );

    // ── Info pane ─────────────────────────────────────────────────────────────
    let mut info_title = vec![Span::raw(" ")];
    info_title.extend(theme::panel_title("info", None, false));
    f.render_widget(Paragraph::new(Line::from(info_title)), info_heading);

    let field = |label: &str, value: Span<'static>| {
        Line::from(vec![
            Span::styled(
                format!("  {:<9}", label),
                Style::default().fg(theme::INK_FAINT),
            ),
            value,
        ])
    };
    let info_lines: Vec<Line> = if let Some(t) = app.tag_view.tags.get(app.tag_view.idx) {
        vec![
            field(
                "name",
                Span::styled(
                    t.name.clone(),
                    Style::default()
                        .fg(theme::WARN)
                        .add_modifier(Modifier::BOLD),
                ),
            ),
            field(
                "commit",
                Span::styled(t.hash.clone(), Style::default().fg(theme::INK_DIM)),
            ),
            field(
                "message",
                Span::styled(t.message.clone(), Style::default().fg(theme::INK)),
            ),
            field(
                "age",
                Span::styled(t.time.clone(), Style::default().fg(theme::INK_FAINT)),
            ),
        ]
    } else {
        vec![Line::from(Span::styled(
            "  no tag selected",
            Style::default().fg(theme::INK_FAINT),
        ))]
    };
    f.render_widget(Paragraph::new(info_lines), info_body);

    if app.tag_view.ops_mode {
        render_ops(f, app, list_body);
    }
}

/// The ops dropdown, anchored under the selected tag.
fn render_ops(f: &mut Frame, app: &App, body: Rect) {
    const OPS: &[(&str, bool)] = &[("push", false), ("new tag", false), ("delete ⚠", true)];

    let dropdown_w = 16u16;
    let dropdown_h = OPS.len() as u16 + 2;
    let entry_y = body.y + app.tag_view.idx as u16 + 1;
    let drop_y = if entry_y + dropdown_h < body.y + body.height {
        entry_y
    } else {
        body.y + body.height.saturating_sub(dropdown_h)
    };
    let drop_area = Rect::new(body.x + 3, drop_y, dropdown_w, dropdown_h);

    let items: Vec<ListItem> = OPS
        .iter()
        .enumerate()
        .map(|(i, (label, danger))| {
            let is_sel = i == app.tag_view.ops_idx;
            let color = if *danger {
                theme::BAD
            } else if is_sel {
                theme::INK
            } else {
                theme::INK_DIM
            };
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(*label, Style::default().fg(color)),
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
    state.select(Some(app.tag_view.ops_idx));

    // A popup keeps its box: it is a window, not a column.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE));

    f.render_widget(Clear, drop_area);
    f.render_stateful_widget(List::new(items).block(block), drop_area, &mut state);
}
