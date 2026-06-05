use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::tui::app::{App, IssueConfirm};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_GREEN, C_RED, C_YELLOW, C_CYAN};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;
    let iv = &app.issue_view;

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // ── Issue list ────────────────────────────────────────────────────────────
    let title = if iv.loading {
        " issues — loading… ".to_string()
    } else {
        format!(" issues — {} ", iv.issues.len())
    };

    let items: Vec<ListItem> = if let Some(err) = &iv.error {
        err.chars().collect::<Vec<_>>().chunks(cols[0].width.saturating_sub(4) as usize)
            .enumerate()
            .map(|(i, chunk)| {
                let text = chunk.iter().collect::<String>();
                if i == 0 {
                    ListItem::new(Line::from(vec![
                        Span::styled("  authentication required: ", Style::default().fg(C_RED)),
                        Span::styled(text, Style::default().fg(C_DIM)),
                    ]))
                } else {
                    ListItem::new(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(text, Style::default().fg(C_DIM)),
                    ]))
                }
            }).collect()
    } else if iv.issues.is_empty() && !iv.loading {
        vec![ListItem::new(Line::from(vec![
            Span::styled("  no open issues", Style::default().fg(C_DIM)),
        ]))]
    } else {
        iv.issues.iter().enumerate().map(|(i, issue)| {
            let is_sel = i == iv.idx;
            let style = if is_sel {
                Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
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
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(format!("#{} ", issue.number), Style::default().fg(C_YELLOW)),
                Span::styled(issue.title.clone(), Style::default().fg(if is_sel { C_WHITE } else { C_SUBTLE })),
                Span::styled(labels, Style::default().fg(C_CYAN)),
                Span::styled(comments, Style::default().fg(C_DIM)),
            ])).style(style)
        }).collect()
    };

    let mut list_state = ListState::default();
    if !iv.issues.is_empty() { list_state.select(Some(iv.idx)); }

    let list_block = Block::default()
        .title(Span::styled(title,
            if focused { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(bc) }
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if focused { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_stateful_widget(List::new(items).block(list_block), cols[0], &mut list_state);

    // ── Detail panel ──────────────────────────────────────────────────────────
    let detail_lines = if let Some(issue) = iv.issues.get(iv.idx) {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("  number  ", Style::default().fg(C_SUBTLE)),
                Span::styled(format!("#{}", issue.number), Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  state   ", Style::default().fg(C_SUBTLE)),
                Span::styled(issue.state.clone(), Style::default().fg(
                    if issue.state == "open" || issue.state == "opened" { C_GREEN } else { C_RED }
                )),
            ]),
            Line::from(vec![
                Span::styled("  author  ", Style::default().fg(C_SUBTLE)),
                Span::styled(issue.author.clone(), Style::default().fg(C_CYAN)),
            ]),
        ];
        if !issue.labels.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  labels  ", Style::default().fg(C_SUBTLE)),
                Span::styled(issue.labels.join(", "), Style::default().fg(C_CYAN)),
            ]));
        }
        lines.push(Line::from(""));
        if let Some(body) = &issue.body {
            let preview: String = body.lines().take(6).collect::<Vec<_>>().join("\n");
            for l in preview.lines() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(l.to_string(), Style::default().fg(C_SUBTLE)),
                ]));
            }
        } else {
            lines.push(Line::from(vec![Span::styled("  no description", Style::default().fg(C_DIM))]));
        }
        lines
    } else {
        vec![Line::from(vec![Span::styled("  no issue selected", Style::default().fg(C_DIM))])]
    };

    let detail_block = Block::default()
        .title(Span::styled(" detail ", Style::default().fg(bc)))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(
        Paragraph::new(detail_lines).block(detail_block).wrap(Wrap { trim: true }),
        cols[1],
    );

    // ── Close confirm ─────────────────────────────────────────────────────────
    if iv.confirm == IssueConfirm::Close {
        let overlay = Rect::new(cols[0].x + 2, cols[0].y + 2 + iv.idx.min(cols[0].height as usize - 5) as u16, 32, 3);
        f.render_widget(Clear, overlay);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  close issue? ", Style::default().fg(C_RED)),
                Span::styled("[y]", Style::default().fg(C_RED).add_modifier(Modifier::BOLD)),
                Span::styled(" / any", Style::default().fg(C_SUBTLE)),
            ])).block(Block::default().borders(Borders::ALL)
                .border_style(Style::default().fg(C_RED))
                .border_type(app.border_type())),
            overlay,
        );
    }

    // ── Create title input ────────────────────────────────────────────────────
    if iv.confirm == IssueConfirm::CreateTitle {
        render_input_overlay(f, app, area, "title", &iv.create_title, bc);
    }

    // ── Create desc input ─────────────────────────────────────────────────────
    if iv.confirm == IssueConfirm::CreateDesc {
        render_input_overlay(f, app, area, "description (optional)", &iv.create_desc, bc);
    }

    // ── Comment input ─────────────────────────────────────────────────────────
    if iv.confirm == IssueConfirm::Comment {
        render_input_overlay(f, app, area, "comment", &iv.comment_input, bc);
    }

    // ── Ops dropdown ─────────────────────────────────────────────────────────
    if iv.ops_mode {
        let ops: &[(&str, bool)] = &[
            ("create",       false),
            ("comment",      false),
            ("open browser", false),
            ("close ⚠",     true),
        ];
        let dw = 16u16;
        let dh = ops.len() as u16 + 2;
        let ey = cols[0].y + 1 + iv.idx as u16 + 1;
        let dy = if ey + dh < cols[0].y + cols[0].height { ey } else { cols[0].y + cols[0].height - dh };
        let drop_area = Rect::new(cols[0].x + 3, dy, dw, dh);

        let drop_items: Vec<ListItem> = ops.iter().enumerate().map(|(i, (label, danger))| {
            let is_sel = i == iv.ops_idx;
            let color = if *danger { C_RED } else if is_sel { C_WHITE } else { C_SUBTLE };
            let prefix = if is_sel { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(*label, Style::default().fg(color)),
            ])).style(if is_sel { Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD) } else { Style::default() })
        }).collect();

        let mut drop_state = ListState::default();
        drop_state.select(Some(iv.ops_idx));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(drop_items).block(Block::default().borders(Borders::ALL).border_type(app.border_type()).border_style(Style::default().fg(bc))),
            drop_area,
            &mut drop_state,
        );
    }
}

fn render_input_overlay(f: &mut Frame, app: &App, area: Rect, label: &str, value: &str, bc: ratatui::style::Color) {
    let ow = 56u16;
    let oh = 3u16;
    let ox = area.x + area.width.saturating_sub(ow) / 2;
    let oy = area.y + area.height.saturating_sub(oh) / 2;
    let overlay = Rect::new(ox, oy, ow, oh);
    f.render_widget(Clear, overlay);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("  {}: ", label), Style::default().fg(C_SUBTLE)),
            Span::styled(format!("{}█", value), Style::default().fg(C_WHITE)),
        ])).block(Block::default().borders(Borders::ALL)
            .border_style(Style::default().fg(bc))
            .border_type(app.border_type())),
        overlay,
    );
}
