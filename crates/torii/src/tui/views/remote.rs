use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::super::ui::{C_CYAN, C_DIM, C_GREEN, C_RED, C_SUBTLE, C_WHITE, C_YELLOW};
use crate::tui::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let remotes = &app.remote_view.remotes;
    let mirrors = &app.remote_view.mirrors;

    // ── Build unified list ────────────────────────────────────────────────────
    let mut items: Vec<ListItem> = vec![];
    let mut sel_list_pos = 0usize;

    if !remotes.is_empty() {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            " git remotes ",
            Style::default().fg(C_SUBTLE).add_modifier(Modifier::BOLD),
        )])));
        for (i, r) in remotes.iter().enumerate() {
            let is_sel = i == app.remote_view.idx;
            if is_sel {
                sel_list_pos = items.len();
            }
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            items.push(
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(bc)),
                    Span::styled(
                        format!("{:<12} ", &r.name),
                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<10}", &r.platform),
                        Style::default().fg(platform_color(&r.platform)),
                    ),
                    Span::styled(truncate(&r.url, 30), Style::default().fg(C_DIM)),
                ]))
                .style(style),
            );
        }
    }

    if !mirrors.is_empty() {
        if !remotes.is_empty() {
            items.push(ListItem::new(Line::from(Span::raw(" "))));
        }
        items.push(ListItem::new(Line::from(vec![Span::styled(
            " mirrors ",
            Style::default().fg(C_SUBTLE).add_modifier(Modifier::BOLD),
        )])));
        for (i, m) in mirrors.iter().enumerate() {
            let abs_idx = remotes.len() + i;
            let is_sel = abs_idx == app.remote_view.idx;
            if is_sel {
                sel_list_pos = items.len();
            }
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            let kind_color = if m.kind == "primary" {
                C_YELLOW
            } else {
                C_SUBTLE
            };
            items.push(
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(bc)),
                    Span::styled(
                        format!("{:<12} ", &m.name),
                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("{:<9}", &m.kind), Style::default().fg(kind_color)),
                    Span::styled(
                        format!(" {:<10}", &m.platform),
                        Style::default().fg(platform_color(&m.platform)),
                    ),
                ]))
                .style(style),
            );
        }
    }

    if remotes.is_empty() && mirrors.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  no remotes configured",
            Style::default().fg(C_DIM),
        )));
    }

    let total = remotes.len() + mirrors.len();
    let title = format!(
        " remote — {} git  {} mirrors ",
        remotes.len(),
        mirrors.len()
    );

    let mut state = ListState::default();
    if total > 0 {
        state.select(Some(sel_list_pos));
    }

    let list_block = Block::default()
        .title(Span::styled(
            title,
            if focused {
                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(bc)
            },
        ))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(if focused {
            Style::default().fg(C_WHITE)
        } else {
            Style::default().fg(bc)
        });
    f.render_stateful_widget(List::new(items).block(list_block), chunks[0], &mut state);

    // ── Info panel ────────────────────────────────────────────────────────────
    let info_lines: Vec<Line> = if app.remote_view.selected_is_mirror() {
        if let Some(m) = app.remote_view.selected_mirror() {
            let https_url = ssh_to_https(&m.url);
            vec![
                Line::from(vec![
                    Span::styled("  name      ", Style::default().fg(C_SUBTLE)),
                    Span::styled(
                        &m.name,
                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  kind      ", Style::default().fg(C_SUBTLE)),
                    Span::styled(
                        &m.kind,
                        Style::default()
                            .fg(if m.kind == "primary" {
                                C_YELLOW
                            } else {
                                C_SUBTLE
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  platform  ", Style::default().fg(C_SUBTLE)),
                    Span::styled(
                        &m.platform,
                        Style::default().fg(platform_color(&m.platform)),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  account   ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&m.account, Style::default().fg(C_WHITE)),
                ]),
                Line::from(vec![
                    Span::styled("  repo      ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&m.repo, Style::default().fg(C_CYAN)),
                ]),
                Line::from(vec![
                    Span::styled("  url       ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&m.url, Style::default().fg(C_DIM)),
                ]),
                Line::from(vec![
                    Span::styled("  https     ", Style::default().fg(C_SUBTLE)),
                    Span::styled(https_url, Style::default().fg(C_DIM)),
                ]),
            ]
        } else {
            vec![Line::from(Span::styled(
                "  no mirror selected",
                Style::default().fg(C_DIM),
            ))]
        }
    } else if let Some(r) = app.remote_view.selected_remote() {
        let https_url = ssh_to_https(&r.url);
        vec![
            Line::from(vec![
                Span::styled("  name      ", Style::default().fg(C_SUBTLE)),
                Span::styled(
                    &r.name,
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("  platform  ", Style::default().fg(C_SUBTLE)),
                Span::styled(
                    &r.platform,
                    Style::default().fg(platform_color(&r.platform)),
                ),
            ]),
            Line::from(vec![
                Span::styled("  url       ", Style::default().fg(C_SUBTLE)),
                Span::styled(&r.url, Style::default().fg(C_CYAN)),
            ]),
            Line::from(vec![
                Span::styled("  https     ", Style::default().fg(C_SUBTLE)),
                Span::styled(https_url, Style::default().fg(C_DIM)),
            ]),
        ]
    } else {
        vec![Line::from(Span::styled(
            "  no remote selected",
            Style::default().fg(C_DIM),
        ))]
    };

    let info_block = Block::default()
        .title(Span::styled(" info ", Style::default().fg(bc)))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(Paragraph::new(info_lines).block(info_block), chunks[1]);

    // ── Ops dropdown overlay ──────────────────────────────────────────────────
    if app.remote_view.ops_mode {
        let is_mirror = app.remote_view.selected_is_mirror();
        let (ops, dropdown_w): (&[(&str, bool)], u16) = if is_mirror {
            (
                &[
                    ("sync all", false),
                    ("force sync", false),
                    ("add mirror", false),
                    ("set primary", false),
                    ("rename", false),
                    ("remove ⚠", true),
                ],
                22,
            )
        } else {
            (
                &[
                    ("fetch", false),
                    ("add remote", false),
                    ("add mirror", false),
                    ("rename", false),
                    ("edit url", false),
                    ("remove ⚠", true),
                    ("open in browser", false),
                ],
                22,
            )
        };

        let dropdown_h = ops.len() as u16 + 2;
        let entry_y = chunks[0].y + 1 + sel_list_pos as u16 + 1;
        let drop_y = if entry_y + dropdown_h < chunks[0].y + chunks[0].height {
            entry_y
        } else {
            chunks[0].y + chunks[0].height - dropdown_h
        };
        let drop_area = Rect::new(chunks[0].x + 3, drop_y, dropdown_w, dropdown_h);

        let drop_items: Vec<ListItem> = ops
            .iter()
            .enumerate()
            .map(|(i, (label, danger))| {
                let is_sel = i == app.remote_view.ops_idx;
                let color = if *danger {
                    C_RED
                } else if is_sel {
                    C_WHITE
                } else {
                    C_SUBTLE
                };
                let prefix = if is_sel { "▶ " } else { "  " };
                let style = if is_sel {
                    Style::default()
                        .bg(app.selected_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(bc)),
                    Span::styled(*label, Style::default().fg(color)),
                ]))
                .style(style)
            })
            .collect();

        let mut drop_state = ListState::default();
        drop_state.select(Some(app.remote_view.ops_idx));

        let drop_block = Block::default()
            .borders(Borders::ALL)
            .border_type(app.border_type())
            .border_style(Style::default().fg(bc));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(drop_items).block(drop_block),
            drop_area,
            &mut drop_state,
        );
    }

    // ── Edit URL overlay ──────────────────────────────────────────────────────
    use crate::tui::app::RemoteConfirm;
    if app.remote_view.confirm == RemoteConfirm::EditUrl {
        let bc = app.brand_color();
        let ow = 60u16;
        let oh = 3u16;
        let ox = area.x + area.width.saturating_sub(ow) / 2;
        let oy = area.y + area.height.saturating_sub(oh) / 2;
        let overlay = Rect::new(ox, oy, ow, oh);
        f.render_widget(Clear, overlay);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  new url: ", Style::default().fg(C_SUBTLE)),
                Span::styled(
                    format!("{}█", app.remote_view.new_url),
                    Style::default().fg(C_WHITE),
                ),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(bc))
                    .border_type(app.border_type()),
            ),
            overlay,
        );
    }
}

fn platform_color(platform: &str) -> ratatui::style::Color {
    match platform.to_lowercase().as_str() {
        "github" => C_WHITE,
        "gitlab" => C_YELLOW,
        "bitbucket" => C_CYAN,
        "codeberg" => C_GREEN,
        _ => C_DIM,
    }
}

fn ssh_to_https(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("git@") {
        let s = rest.replacen(':', "/", 1);
        let s = s.strip_suffix(".git").unwrap_or(&s);
        return format!("https://{}", s);
    }
    url.strip_suffix(".git").unwrap_or(url).to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut)
}
