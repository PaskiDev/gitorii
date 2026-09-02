//! The snapshot view: what has been saved aside, how to make another, and how
//! often one is taken automatically.
//!
//! Three sections parted by rules rather than three boxes. The ops dropdown
//! keeps its box: a popup is a window.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::tui::app::{App, AutoSnapshotInterval, SnapshotFocus};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;
    let in_list = focused && app.snapshot_view.focus == SnapshotFocus::List;
    let in_create = focused && app.snapshot_view.focus == SnapshotFocus::Create;
    let in_auto = focused && app.snapshot_view.focus == SnapshotFocus::AutoConfig;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(AutoSnapshotInterval::all().len() as u16 + 1),
        ])
        .split(area);

    render_list(f, app, rows[0], in_list);
    theme::hrule_content(f, rows[1], &[]);
    render_create(f, app, rows[2], in_create);
    theme::hrule_content(f, rows[3], &[]);
    render_auto(f, app, rows[4], in_auto);
}

fn render_list(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let display_indices: Vec<usize> =
        if app.snapshot_view.filtered.is_empty() && app.snapshot_view.search_query.is_empty() {
            (0..app.snapshot_view.snapshots.len()).collect()
        } else {
            app.snapshot_view.filtered.clone()
        };

    let searching = !app.snapshot_view.search_query.is_empty()
        && !app.snapshot_view.filtered.is_empty();

    let items: Vec<ListItem> = if app.snapshot_view.snapshots.is_empty() {
        vec![ListItem::new(Span::styled(
            "no snapshots — press n to create one",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else if display_indices.is_empty() {
        vec![ListItem::new(Span::styled(
            "no matches",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        display_indices
            .iter()
            .enumerate()
            .map(|(pos, &i)| {
                let s = &app.snapshot_view.snapshots[i];
                let is_sel = active && pos == app.snapshot_view.idx;
                let name_color = if searching {
                    theme::OK
                } else if is_sel {
                    theme::INK
                } else {
                    theme::INK_DIM
                };
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        &s.name,
                        Style::default().fg(name_color).add_modifier(if is_sel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                    ),
                    Span::styled(format!("  {}", s.id), Style::default().fg(theme::WARN)),
                    Span::styled(format!("  {}", s.time), Style::default().fg(theme::INK_FAINT)),
                ]))
                .style(if is_sel {
                    Style::default().bg(theme::selection(app))
                } else {
                    Style::default()
                })
            })
            .collect()
    };

    let mut state = ListState::default();
    if active && !display_indices.is_empty() {
        state.select(Some(app.snapshot_view.idx));
    }

    // While searching, the query is the one thing here worth the accent.
    let heading: Vec<Span> = if app.snapshot_view.search_mode {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(theme::panel_title("search", None, true));
        spans.push(Span::styled(
            format!("  {}", app.snapshot_view.search_query),
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("█", Style::default().fg(theme::accent(app))));
        spans
    } else if !app.snapshot_view.search_query.is_empty() {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(theme::panel_title(
            "snapshots",
            Some(display_indices.len()),
            active,
        ));
        spans.push(Span::styled(
            format!("  matching \"{}\"", app.snapshot_view.search_query),
            Style::default().fg(theme::INK_FAINT),
        ));
        spans
    } else {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(theme::panel_title(
            "snapshots",
            Some(app.snapshot_view.snapshots.len()),
            active,
        ));
        spans
    };
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );

    if app.snapshot_view.ops_mode && active {
        render_ops(f, app, body);
    }
}

/// The ops dropdown, anchored under the selected snapshot.
fn render_ops(f: &mut Frame, app: &App, body: Rect) {
    const OPS: &[(&str, bool)] = &[("restore", false), ("new", false), ("delete ⚠", true)];

    let dropdown_w = 16u16;
    let dropdown_h = OPS.len() as u16 + 2;
    let entry_y = body.y + app.snapshot_view.idx as u16 + 1;
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
            let is_sel = i == app.snapshot_view.ops_idx;
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
    state.select(Some(app.snapshot_view.ops_idx));

    // A popup keeps its box: it is a window, not a column.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE));

    f.render_widget(Clear, drop_area);
    f.render_stateful_widget(List::new(items).block(block), drop_area, &mut state);
}

fn render_create(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("new snapshot", None, active));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let name = &app.snapshot_view.create_name;
    let line = if active {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(name.as_str(), Style::default().fg(theme::INK)),
            Span::styled("█", Style::default().fg(theme::accent(app))),
        ])
    } else {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                if name.is_empty() {
                    "snapshot name..."
                } else {
                    name.as_str()
                },
                Style::default().fg(theme::INK_FAINT),
            ),
        ])
    };
    f.render_widget(Paragraph::new(line), body);
}

fn render_auto(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("auto-snapshot", None, active));
    heading.push(Span::styled(
        format!("  {}", app.snapshot_view.auto_interval.label()),
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let items: Vec<ListItem> = AutoSnapshotInterval::all()
        .iter()
        .enumerate()
        .map(|(i, interval)| {
            let is_sel = active && i == app.snapshot_view.auto_interval_idx;
            let is_current = *interval == app.snapshot_view.auto_interval;
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(
                    if is_current { "✓ " } else { "  " },
                    Style::default().fg(theme::OK),
                ),
                Span::styled(
                    interval.label(),
                    Style::default()
                        .fg(if is_sel { theme::INK } else { theme::INK_DIM })
                        .add_modifier(if is_sel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ]))
            .style(if is_sel {
                Style::default().bg(theme::selection(app))
            } else {
                Style::default()
            })
        })
        .collect();

    f.render_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}
