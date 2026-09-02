//! The sync view: which way the commits travel, and how the last run went.
//!
//! Two sections parted by a rule rather than two boxes; the operation being
//! selected is marked by the caret, not by a coloured border.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Padding, Paragraph},
    Frame,
};

use crate::tui::app::{App, SyncOp, SyncStatus};
use crate::tui::theme;

const OPS: &[SyncOp] = &[
    SyncOp::PullPush,
    SyncOp::PullOnly,
    SyncOp::PushOnly,
    SyncOp::ForcePush,
    SyncOp::Fetch,
];

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(area);

    render_ops(f, app, rows[0], focused);
    theme::hrule_content(f, rows[1], &[]);
    render_status(f, app, rows[2]);
}

fn render_ops(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("operation", None, active));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let items: Vec<ListItem> = OPS
        .iter()
        .map(|op| {
            let is_sel = active && *op == app.sync_view.selected_op;
            let (label, desc) = op_label(op);
            let label_color = if *op == SyncOp::ForcePush {
                // The one entry that rewrites someone else's history says so
                // in its colour, the way the branch view marks a delete.
                theme::BAD
            } else if is_sel {
                theme::INK
            } else {
                theme::INK_DIM
            };
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(format!("{:<14}", label), Style::default().fg(label_color)),
                Span::styled(desc, Style::default().fg(theme::INK_FAINT)),
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

    f.render_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

fn progress_bar(tick: usize) -> String {
    const TOTAL: usize = 10;
    const CYCLE: usize = (TOTAL - 1) * 2;
    let pos = tick % CYCLE;
    let ball = if pos < TOTAL { pos } else { CYCLE - pos };
    (0..TOTAL)
        .map(|i| if i == ball { '▰' } else { '▱' })
        .collect()
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("status", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let line = match &app.sync_view.status {
        SyncStatus::Idle => Line::from(vec![
            Span::raw("  "),
            Span::styled("ready", Style::default().fg(theme::INK_FAINT)),
        ]),
        SyncStatus::Running => {
            let bar = progress_bar(app.tick / 2);
            Line::from(vec![
                Span::raw("  "),
                Span::styled(bar, Style::default().fg(theme::WARN)),
                Span::styled("  syncing...", Style::default().fg(theme::WARN)),
            ])
        }
        SyncStatus::Done(msg) => Line::from(vec![
            Span::raw("  "),
            Span::styled("▰▰▰▰▰▰▰▰▰▰", Style::default().fg(theme::OK)),
            Span::styled(
                format!("  ✓  {}", msg.lines().next().unwrap_or("")),
                Style::default().fg(theme::OK),
            ),
        ]),
        SyncStatus::Error(msg) => Line::from(vec![
            Span::raw("  "),
            Span::styled("▰▰▰▰▰▰▰▰▰▰", Style::default().fg(theme::BAD)),
            Span::styled(
                format!("  ✗  {}", msg.lines().next().unwrap_or("")),
                Style::default().fg(theme::BAD),
            ),
        ]),
    };

    f.render_widget(Paragraph::new(line), body);
}

fn op_label(op: &SyncOp) -> (&'static str, &'static str) {
    match op {
        SyncOp::PullPush => (
            "pull + push",
            "fetch remote changes then push local commits",
        ),
        SyncOp::PullOnly => ("pull", "fetch and merge remote changes only"),
        SyncOp::PushOnly => ("push", "push local commits to remote"),
        SyncOp::ForcePush => ("force push", "overwrite remote history (use with care)"),
        SyncOp::Fetch => ("fetch", "update remote refs without merging"),
    }
}
