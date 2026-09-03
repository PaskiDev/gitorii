//! The ignore view: the `.toriignore` pair as rules you can read and change.
//!
//! Two panes parted by a rule. The left one lists every rule from both files,
//! grouped by kind; the right one says what the selected rule is and, above
//! all, **which file it lives in** — the difference between a rule everyone
//! with the repo can read and one that never leaves this machine.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::ignore_rules::{Kind, Origin};
use crate::tui::app::{App, IgnoreFocus};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    let divider = theme::divider_right();
    let list_pane = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = [panes[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    let [list_heading, list_body] = theme::heading_and_body(list_pane);
    let [detail_heading, detail_body] = theme::heading_and_body(panes[1]);

    render_list(f, app, list_heading, list_body, focused);
    render_detail(f, app, detail_heading, detail_body);

    match app.ignore_view.focus {
        IgnoreFocus::Input => render_input(f, app, area),
        IgnoreFocus::ConfirmDelete => render_confirm(f, app, area),
        IgnoreFocus::List => {}
    }
}

fn render_list(f: &mut Frame, app: &App, heading_row: Rect, body: Rect, focused: bool) {
    let iv = &app.ignore_view;
    let active = focused && iv.focus == IgnoreFocus::List;

    let public = iv
        .rules
        .iter()
        .filter(|r| r.origin == Origin::Public)
        .count();
    let local = iv.rules.len() - public;

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("rules", Some(iv.rules.len()), active));
    heading.push(Span::styled(
        format!("  {public} public  {local} local"),
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let items: Vec<ListItem> = if iv.rules.is_empty() {
        vec![ListItem::new(Span::styled(
            "no rules — press a to add one",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        let mut items = Vec::new();
        let mut last_kind: Option<Kind> = None;
        for (i, rule) in iv.rules.iter().enumerate() {
            // A group header whenever the kind changes, so paths and secret
            // patterns never read as one list.
            if last_kind != Some(rule.kind) {
                items.push(ListItem::new(Line::from(Span::styled(
                    rule.kind.label().to_string(),
                    Style::default()
                        .fg(theme::INK_FAINT)
                        .add_modifier(Modifier::BOLD),
                ))));
                last_kind = Some(rule.kind);
            }
            let is_sel = active && i == iv.idx;
            items.push(
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        truncate(&rule.pattern, 34),
                        Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                    ),
                    Span::styled(
                        format!("  {}", rule.origin.label()),
                        Style::default().fg(origin_colour(rule.origin)),
                    ),
                ]))
                .style(if is_sel {
                    Style::default()
                        .bg(theme::selection(app))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
            );
        }
        items
    };

    // The list carries group headers, so the selected row is not at `idx`.
    let mut state = ListState::default();
    if !iv.rules.is_empty() {
        state.select(Some(row_of(app, iv.idx)));
    }
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
}

/// Where rule `idx` sits once the group headers are counted in.
fn row_of(app: &App, idx: usize) -> usize {
    let mut row = 0;
    let mut last_kind = None;
    for (i, rule) in app.ignore_view.rules.iter().enumerate() {
        if last_kind != Some(rule.kind) {
            row += 1;
            last_kind = Some(rule.kind);
        }
        if i == idx {
            return row;
        }
        row += 1;
    }
    row
}

fn render_detail(f: &mut Frame, app: &App, heading_row: Rect, body: Rect) {
    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("detail", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let iv = &app.ignore_view;
    let mut lines: Vec<Line> = match app.ignore_selected() {
        Some(rule) => vec![
            field(
                "file",
                Span::styled(
                    rule.origin.file_name().to_string(),
                    Style::default()
                        .fg(origin_colour(rule.origin))
                        .add_modifier(Modifier::BOLD),
                ),
            ),
            field(
                "scope",
                Span::styled(
                    match rule.origin {
                        Origin::Public => "committed, public",
                        Origin::Local => "private, not committed",
                    },
                    Style::default().fg(theme::INK_FAINT),
                ),
            ),
            field(
                "kind",
                Span::styled(rule.kind.label(), Style::default().fg(theme::INK_DIM)),
            ),
            field(
                "line",
                Span::styled(
                    rule.line_no.to_string(),
                    Style::default().fg(theme::INK_FAINT),
                ),
            ),
            Line::from(""),
            field(
                "pattern",
                Span::styled(rule.pattern.clone(), Style::default().fg(theme::INK)),
            ),
        ],
        None => vec![Line::from(Span::styled(
            "no rule selected",
            Style::default().fg(theme::INK_FAINT),
        ))],
    };

    if let Some(rule) = app.ignore_selected() {
        if let Some(name) = &rule.name {
            lines.push(field(
                "name",
                Span::styled(name.clone(), Style::default().fg(theme::INK_DIM)),
            ));
        }
    }

    // What the next `a` would write, so the target is known before typing.
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("new rule ", Style::default().fg(theme::INK_FAINT)),
        Span::styled(
            iv.new_kind.label(),
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" → ", Style::default().fg(theme::INK_FAINT)),
        Span::styled(
            iv.new_origin.file_name(),
            Style::default().fg(origin_colour(iv.new_origin)),
        ),
    ]));

    if let Some(status) = &iv.status {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            status.clone(),
            Style::default().fg(theme::BAD),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let iv = &app.ignore_view;
    let w = 64u16.min(area.width.saturating_sub(4));
    let h = 5u16;
    let overlay = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, overlay);

    let msg = &iv.input;
    let cursor = iv.cursor.min(msg.len());
    let (before, after) = msg.split_at(cursor);

    let body = vec![
        Line::from(vec![
            Span::styled(
                format!(" new {} rule → ", iv.new_kind.label()),
                Style::default().fg(theme::INK_FAINT),
            ),
            Span::styled(
                iv.new_origin.file_name(),
                Style::default()
                    .fg(origin_colour(iv.new_origin))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(before.to_string(), Style::default().fg(theme::INK)),
            Span::styled("█", Style::default().fg(theme::accent(app))),
            Span::styled(after.to_string(), Style::default().fg(theme::INK)),
        ]),
        Line::from({
            let mut spans = vec![Span::raw(" ")];
            spans.extend(theme::key_hint(app, "Tab", "kind"));
            spans.extend(theme::key_hint(app, "^T", "target"));
            spans.extend(theme::key_hint(app, "Enter", "add"));
            spans.extend(theme::key_hint(app, "Esc", "cancel"));
            spans
        }),
    ];

    f.render_widget(Paragraph::new(body).block(popup(app)), overlay);
}

fn render_confirm(f: &mut Frame, app: &App, area: Rect) {
    let Some(rule) = app.ignore_selected() else {
        return;
    };
    let w = 62u16.min(area.width.saturating_sub(4));
    let h = 5u16;
    let overlay = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, overlay);

    let body = vec![
        Line::from(vec![
            Span::styled("  remove ", Style::default().fg(theme::INK)),
            Span::styled(
                truncate(&rule.pattern, 30),
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" from {}?", rule.origin.file_name()),
                Style::default().fg(theme::INK),
            ),
        ]),
        // Removing a rule loosens the scanner: say so, do not just ask.
        Line::from(Span::styled(
            match rule.kind {
                Kind::Secret => "  the scanner will stop allowing what it matched",
                _ => "  the path will be scanned and committed again",
            },
            Style::default().fg(theme::INK_FAINT),
        )),
        Line::from({
            let mut spans = vec![Span::raw(" ")];
            spans.extend(theme::key_hint(app, "y", "remove"));
            spans.extend(theme::key_hint(app, "n", "keep"));
            spans
        }),
    ];

    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::BAD)),
        ),
        overlay,
    );
}

/// A popup keeps its box: it is a window, not a column.
fn popup(app: &App) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE))
}

/// Private rules are the ones worth spotting at a glance.
fn origin_colour(origin: Origin) -> ratatui::style::Color {
    match origin {
        Origin::Public => theme::INK_FAINT,
        Origin::Local => theme::WARN,
    }
}

fn field(label: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<9}", label),
            Style::default().fg(theme::INK_FAINT),
        ),
        value,
    ])
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}
