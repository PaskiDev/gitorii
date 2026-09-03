//! The issue view: the open issues, and the selected one in full.
//!
//! Two panes parted by a rule rather than two boxes. The ops dropdown and the
//! input overlays keep their boxes: a popup is a window.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{App, IssueConfirm};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;
    let iv = &app.issue_view;

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // The list carries the rule; the detail sits the other side of it.
    let divider = theme::divider_right();
    let list_pane = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = [panes[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    let [list_heading, list_body] = theme::heading_and_body(list_pane);
    let [detail_heading, detail_body] = theme::heading_and_body(panes[1]);

    // ── Issue list ────────────────────────────────────────────────────────────
    let items: Vec<ListItem> = if let Some(err) = &iv.error {
        err.chars()
            .collect::<Vec<_>>()
            .chunks(list_body.width.saturating_sub(4).max(1) as usize)
            .enumerate()
            .map(|(i, chunk)| {
                let text = chunk.iter().collect::<String>();
                if i == 0 {
                    ListItem::new(Line::from(vec![
                        Span::styled("authentication required: ", Style::default().fg(theme::BAD)),
                        Span::styled(text, Style::default().fg(theme::INK_FAINT)),
                    ]))
                } else {
                    ListItem::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(text, Style::default().fg(theme::INK_FAINT)),
                    ]))
                }
            })
            .collect()
    } else if iv.issues.is_empty() && !iv.loading {
        vec![ListItem::new(Span::styled(
            "no open issues",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        iv.issues
            .iter()
            .enumerate()
            .map(|(i, issue)| {
                let is_sel = focused && i == iv.idx;
                let labels = if issue.labels.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", issue.labels.join(", "))
                };
                let comments = if issue.comments > 0 {
                    format!(" 💬{}", issue.comments)
                } else {
                    String::new()
                };
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        format!("#{} ", issue.number),
                        Style::default().fg(theme::WARN),
                    ),
                    Span::styled(
                        issue.title.clone(),
                        Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                    ),
                    Span::styled(labels, Style::default().fg(theme::INK_FAINT)),
                    Span::styled(comments, Style::default().fg(theme::INK_FAINT)),
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

    let mut list_state = ListState::default();
    if !iv.issues.is_empty() {
        list_state.select(Some(iv.idx));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "issues",
        if iv.loading {
            None
        } else {
            Some(iv.issues.len())
        },
        focused,
    ));
    if iv.loading {
        heading.push(Span::styled(
            "  loading…",
            Style::default().fg(theme::INK_FAINT),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(heading)), list_heading);
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        list_body,
        &mut list_state,
    );
    // Rows the pointer can address, mapped from where the list actually
    // scrolled to.
    app.hits.borrow_mut().rows(
        list_body,
        "issue",
        list_state.offset(),
        iv.issues.len().saturating_sub(list_state.offset()),
    );

    // ── Detail pane ───────────────────────────────────────────────────────────
    let mut detail_title = vec![Span::raw(" ")];
    detail_title.extend(theme::panel_title("detail", None, false));
    f.render_widget(Paragraph::new(Line::from(detail_title)), detail_heading);

    let detail_lines: Vec<Line> = match iv.issues.get(iv.idx) {
        Some(issue) => {
            let open = issue.state == "open" || issue.state == "opened";
            let mut lines = vec![
                field(
                    "number",
                    Span::styled(
                        format!("#{}", issue.number),
                        Style::default()
                            .fg(theme::WARN)
                            .add_modifier(Modifier::BOLD),
                    ),
                ),
                field(
                    "state",
                    Span::styled(
                        issue.state.clone(),
                        Style::default().fg(if open { theme::OK } else { theme::INK_FAINT }),
                    ),
                ),
                field(
                    "author",
                    Span::styled(issue.author.clone(), Style::default().fg(theme::INK_DIM)),
                ),
            ];
            if !issue.labels.is_empty() {
                lines.push(field(
                    "labels",
                    Span::styled(issue.labels.join(", "), Style::default().fg(theme::INK_DIM)),
                ));
            }
            lines.push(Line::from(""));
            match &issue.body {
                Some(body) => {
                    for l in body.lines().take(6) {
                        lines.push(Line::from(Span::styled(
                            l.to_string(),
                            Style::default().fg(theme::INK_DIM),
                        )));
                    }
                }
                None => lines.push(Line::from(Span::styled(
                    "no description",
                    Style::default().fg(theme::INK_FAINT),
                ))),
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "no issue selected",
            Style::default().fg(theme::INK_FAINT),
        ))],
    };
    f.render_widget(
        Paragraph::new(detail_lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        detail_body,
    );

    // ── Overlays ──────────────────────────────────────────────────────────────
    if iv.confirm == IssueConfirm::Close {
        render_close_confirm(f, app, list_body);
    }
    match iv.confirm {
        IssueConfirm::CreateTitle => render_input_overlay(f, app, area, "title", &iv.create_title),
        IssueConfirm::CreateDesc => {
            render_input_overlay(f, app, area, "description (optional)", &iv.create_desc)
        }
        IssueConfirm::Comment => render_input_overlay(f, app, area, "comment", &iv.comment_input),
        _ => {}
    }

    if iv.ops_mode {
        render_ops(f, app, list_body);
    }
}

fn field(label: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<8}", label),
            Style::default().fg(theme::INK_FAINT),
        ),
        value,
    ])
}

/// The close prompt, anchored on the issue it would close.
fn render_close_confirm(f: &mut Frame, app: &App, body: Rect) {
    let row = app
        .issue_view
        .idx
        .min(body.height.saturating_sub(3) as usize) as u16;
    let overlay = Rect::new(body.x + 2, body.y + row, 32, 3);
    f.render_widget(Clear, overlay);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  close issue? ", Style::default().fg(theme::BAD)),
            Span::styled(
                "y",
                Style::default()
                    .fg(theme::accent(app))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  any other key cancels",
                Style::default().fg(theme::INK_FAINT),
            ),
        ]))
        // A destructive prompt keeps the red border: here red is status, not
        // the brand.
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::BAD)),
        ),
        overlay,
    );
}

/// The ops dropdown, anchored under the selected issue.
fn render_ops(f: &mut Frame, app: &App, body: Rect) {
    const OPS: &[(&str, bool)] = &[
        ("create", false),
        ("comment", false),
        ("open browser", false),
        ("close ⚠", true),
    ];

    let dropdown_w = 16u16;
    let dropdown_h = OPS.len() as u16 + 2;
    let entry_y = body.y + app.issue_view.idx as u16 + 1;
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
            let is_sel = i == app.issue_view.ops_idx;
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
    state.select(Some(app.issue_view.ops_idx));

    f.render_widget(Clear, drop_area);
    f.render_stateful_widget(List::new(items).block(popup(app)), drop_area, &mut state);
}

fn render_input_overlay(f: &mut Frame, app: &App, area: Rect, label: &str, value: &str) {
    let ow = 56u16;
    let oh = 3u16;
    let ox = area.x + area.width.saturating_sub(ow) / 2;
    let oy = area.y + area.height.saturating_sub(oh) / 2;
    let overlay = Rect::new(ox, oy, ow, oh);

    f.render_widget(Clear, overlay);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("  {}: ", label),
                Style::default().fg(theme::INK_FAINT),
            ),
            Span::styled(value.to_string(), Style::default().fg(theme::INK)),
            Span::styled("█", Style::default().fg(theme::accent(app))),
        ]))
        .block(popup(app)),
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
