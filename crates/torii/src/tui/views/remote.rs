//! The remote view: git remotes and mirrors in one list, and what the selected
//! one points at.
//!
//! Two panes parted by a rule rather than two boxes. The ops dropdown and the
//! url editor keep their boxes: a popup is a window.

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
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // The list carries the rule; the info pane sits the other side of it.
    let divider = theme::divider_right();
    let list_pane = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = [panes[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    let [list_heading, list_body] = theme::heading_and_body(list_pane);
    let [info_heading, info_body] = theme::heading_and_body(panes[1]);

    let remotes = &app.remote_view.remotes;
    let mirrors = &app.remote_view.mirrors;

    // ── The list: remotes, then mirrors, under their own headers ──────────────
    let mut items: Vec<ListItem> = vec![];
    let mut sel_list_pos = 0usize;

    if !remotes.is_empty() {
        items.push(group_header("git remotes"));
        for (i, r) in remotes.iter().enumerate() {
            let is_sel = focused && i == app.remote_view.idx;
            if is_sel {
                sel_list_pos = items.len();
            }
            items.push(
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        format!("{:<12} ", &r.name),
                        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<10}", &r.platform),
                        Style::default().fg(platform_color(&r.platform)),
                    ),
                    Span::styled(
                        truncate(&r.url, 30),
                        Style::default().fg(theme::INK_FAINT),
                    ),
                ]))
                .style(row_style(app, is_sel)),
            );
        }
    }

    if !mirrors.is_empty() {
        if !remotes.is_empty() {
            items.push(ListItem::new(Line::from(Span::raw(" "))));
        }
        items.push(group_header("mirrors"));
        for (i, m) in mirrors.iter().enumerate() {
            let is_sel = focused && remotes.len() + i == app.remote_view.idx;
            if is_sel {
                sel_list_pos = items.len();
            }
            items.push(
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        format!("{:<12} ", &m.name),
                        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<9}", &m.kind),
                        Style::default().fg(kind_color(&m.kind)),
                    ),
                    Span::styled(
                        format!(" {:<10}", &m.platform),
                        Style::default().fg(platform_color(&m.platform)),
                    ),
                ]))
                .style(row_style(app, is_sel)),
            );
        }
    }

    if remotes.is_empty() && mirrors.is_empty() {
        items.push(ListItem::new(Span::styled(
            "no remotes configured",
            Style::default().fg(theme::INK_FAINT),
        )));
    }

    let mut state = ListState::default();
    if !remotes.is_empty() || !mirrors.is_empty() {
        state.select(Some(sel_list_pos));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("remotes", None, focused));
    heading.push(Span::styled(
        format!("  {} git  {} mirrors", remotes.len(), mirrors.len()),
        Style::default().fg(theme::INK_FAINT),
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

    let info_lines: Vec<Line> = if app.remote_view.selected_is_mirror() {
        match app.remote_view.selected_mirror() {
            Some(m) => vec![
                field(
                    "name",
                    Span::styled(
                        m.name.clone(),
                        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                    ),
                ),
                field(
                    "kind",
                    Span::styled(
                        m.kind.clone(),
                        Style::default()
                            .fg(kind_color(&m.kind))
                            .add_modifier(Modifier::BOLD),
                    ),
                ),
                field(
                    "platform",
                    Span::styled(
                        m.platform.clone(),
                        Style::default().fg(platform_color(&m.platform)),
                    ),
                ),
                field(
                    "account",
                    Span::styled(m.account.clone(), Style::default().fg(theme::INK)),
                ),
                field(
                    "repo",
                    Span::styled(m.repo.clone(), Style::default().fg(theme::INK_DIM)),
                ),
                field(
                    "url",
                    Span::styled(m.url.clone(), Style::default().fg(theme::INK_FAINT)),
                ),
                field(
                    "https",
                    Span::styled(ssh_to_https(&m.url), Style::default().fg(theme::INK_FAINT)),
                ),
            ],
            None => vec![empty("no mirror selected")],
        }
    } else {
        match app.remote_view.selected_remote() {
            Some(r) => vec![
                field(
                    "name",
                    Span::styled(
                        r.name.clone(),
                        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                    ),
                ),
                field(
                    "platform",
                    Span::styled(
                        r.platform.clone(),
                        Style::default().fg(platform_color(&r.platform)),
                    ),
                ),
                field(
                    "url",
                    Span::styled(r.url.clone(), Style::default().fg(theme::INK_DIM)),
                ),
                field(
                    "https",
                    Span::styled(ssh_to_https(&r.url), Style::default().fg(theme::INK_FAINT)),
                ),
            ],
            None => vec![empty("no remote selected")],
        }
    };
    f.render_widget(Paragraph::new(info_lines), info_body);

    if app.remote_view.ops_mode {
        render_ops(f, app, list_body, sel_list_pos);
    }

    use crate::tui::app::RemoteConfirm;
    if app.remote_view.confirm == RemoteConfirm::EditUrl {
        render_url_editor(f, app, area);
    }
}

/// The ops dropdown, anchored under the selected entry.
fn render_ops(f: &mut Frame, app: &App, body: Rect, sel_list_pos: usize) {
    let ops: &[(&str, bool)] = if app.remote_view.selected_is_mirror() {
        &[
            ("sync all", false),
            ("force sync", false),
            ("add mirror", false),
            ("set primary", false),
            ("rename", false),
            ("remove ⚠", true),
        ]
    } else {
        &[
            ("fetch", false),
            ("add remote", false),
            ("add mirror", false),
            ("rename", false),
            ("edit url", false),
            ("remove ⚠", true),
            ("open in browser", false),
        ]
    };

    let dropdown_w = 22u16;
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
            let is_sel = i == app.remote_view.ops_idx;
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
            .style(row_style(app, is_sel))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.remote_view.ops_idx));

    f.render_widget(Clear, drop_area);
    f.render_stateful_widget(
        List::new(items).block(popup(app)),
        drop_area,
        &mut state,
    );
}

fn render_url_editor(f: &mut Frame, app: &App, area: Rect) {
    let ow = 60u16;
    let oh = 3u16;
    let ox = area.x + area.width.saturating_sub(ow) / 2;
    let oy = area.y + area.height.saturating_sub(oh) / 2;
    let overlay = Rect::new(ox, oy, ow, oh);

    f.render_widget(Clear, overlay);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  new url: ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                app.remote_view.new_url.clone(),
                Style::default().fg(theme::INK),
            ),
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

fn row_style(app: &App, selected: bool) -> Style {
    if selected {
        Style::default()
            .bg(theme::selection(app))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

/// The `git remotes` / `mirrors` divider inside the list.
fn group_header(label: &str) -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled(
        label.to_string(),
        Style::default()
            .fg(theme::INK_FAINT)
            .add_modifier(Modifier::BOLD),
    )))
}

fn field(label: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<10}", label),
            Style::default().fg(theme::INK_FAINT),
        ),
        value,
    ])
}

fn empty(text: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {}", text),
        Style::default().fg(theme::INK_FAINT),
    ))
}

fn kind_color(kind: &str) -> ratatui::style::Color {
    if kind == "primary" {
        theme::WARN
    } else {
        theme::INK_DIM
    }
}

fn platform_color(platform: &str) -> ratatui::style::Color {
    match platform.to_lowercase().as_str() {
        "github" => theme::INK,
        "gitlab" => theme::WARN,
        "codeberg" => theme::OK,
        "bitbucket" => theme::INK_DIM,
        _ => theme::INK_FAINT,
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
