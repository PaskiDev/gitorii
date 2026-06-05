use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::tui::app::{App, SyncOp, SyncStatus};
use super::super::ui::{C_WHITE, C_DIM, C_GREEN, C_RED, C_YELLOW};

const OPS: &[SyncOp] = &[
    SyncOp::PullPush,
    SyncOp::PullOnly,
    SyncOp::PushOnly,
    SyncOp::ForcePush,
    SyncOp::Fetch,
];

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

    render_ops(f, app, chunks[0]);
    render_status(f, app, chunks[1]);
}

fn render_ops(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let items: Vec<ListItem> = OPS.iter().map(|op| {
        let is_sel = *op == app.sync_view.selected_op;
        let (label, desc) = op_label(op);
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let prefix = if is_sel { "█ " } else { "  " };
        let label_color = if is_sel { bc } else { C_WHITE };
        let line = Line::from(vec![
            Span::styled(prefix, Style::default().fg(bc)),
            Span::styled(format!("{:<14}", label), Style::default().fg(label_color)),
            Span::styled(desc, Style::default().fg(C_DIM)),
        ]);
        ListItem::new(line).style(style)
    }).collect();

    let block = Block::default()
        .title(Span::styled(" operation ", Style::default().fg(bc)))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(List::new(items).block(block), area);
}

fn progress_bar(tick: usize) -> String {
    const TOTAL: usize = 10;
    const CYCLE: usize = (TOTAL - 1) * 2;
    let pos = tick % CYCLE;
    let ball = if pos < TOTAL { pos } else { CYCLE - pos };
    (0..TOTAL).map(|i| if i == ball { '▰' } else { '▱' }).collect()
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();

    let line = match &app.sync_view.status {
        SyncStatus::Idle => Line::from(vec![
            Span::raw(" "),
            Span::styled("ready", Style::default().fg(C_DIM)),
        ]),
        SyncStatus::Running => {
            let bar = progress_bar(app.tick / 2);
            Line::from(vec![
                Span::raw(" "),
                Span::styled(bar, Style::default().fg(C_YELLOW)),
                Span::styled("  syncing...", Style::default().fg(C_YELLOW)),
            ])
        }
        SyncStatus::Done(msg) => Line::from(vec![
            Span::raw(" "),
            Span::styled("▰▰▰▰▰▰▰▰▰▰", Style::default().fg(C_GREEN)),
            Span::styled(format!("  ✓  {}", msg.lines().next().unwrap_or("")), Style::default().fg(C_GREEN)),
        ]),
        SyncStatus::Error(msg) => Line::from(vec![
            Span::raw(" "),
            Span::styled("▰▰▰▰▰▰▰▰▰▰", Style::default().fg(C_RED)),
            Span::styled(format!("  ✗  {}", msg.lines().next().unwrap_or("")), Style::default().fg(C_RED)),
        ]),
    };

    let block = Block::default()
        .title(Span::styled(" status ", Style::default().fg(bc)))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn op_label(op: &SyncOp) -> (&'static str, &'static str) {
    match op {
        SyncOp::PullPush  => ("pull + push",  "fetch remote changes then push local commits"),
        SyncOp::PullOnly  => ("pull",         "fetch and merge remote changes only"),
        SyncOp::PushOnly  => ("push",         "push local commits to remote"),
        SyncOp::ForcePush => ("force push",   "overwrite remote history (use with care)"),
        SyncOp::Fetch     => ("fetch",        "update remote refs without merging"),
    }
}
