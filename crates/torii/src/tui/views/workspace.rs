//! The workspace view: the workspaces, the repos of the selected one, and what
//! the selection holds.
//!
//! Three columns parted by rules rather than three boxes; the ops dropdown
//! keeps its box, because a popup is a window.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::tui::app::{App, WorkspaceFocus};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;

    if app.workspace_view.workspaces.is_empty() {
        let [heading_row, body] = theme::heading_and_body(area);
        let mut heading = vec![Span::raw(" ")];
        heading.extend(theme::panel_title("workspaces", Some(0), focused));
        f.render_widget(Paragraph::new(Line::from(heading)), heading_row);
        f.render_widget(
            Paragraph::new(Span::styled(
                "  no workspaces — run `torii workspace add <name> <path>` to create one",
                Style::default().fg(theme::INK_FAINT),
            )),
            body,
        );
        return;
    }

    let focus_ws = app.workspace_view.focus == WorkspaceFocus::Workspaces;
    let focus_repos = !focus_ws;
    let ws_active = focus_ws && focused;
    let repos_active = focus_repos && focused;

    // workspaces(26) │ repos(min) │ info(36)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(26),
            Constraint::Min(1),
            Constraint::Length(36),
        ])
        .split(area);

    // The first two columns carry the rule to their right, and both rules meet
    // the chrome above and below.
    let ws_divider = theme::divider_right();
    let ws_pane = ws_divider.inner(cols[0]);
    f.render_widget(ws_divider, cols[0]);
    let repos_divider = theme::divider_right();
    let repos_pane = repos_divider.inner(cols[1]);
    f.render_widget(repos_divider, cols[1]);
    let spines = [
        cols[0].right().saturating_sub(1),
        cols[1].right().saturating_sub(1),
    ];
    theme::tie_above(f, area, &spines);
    theme::tie_below(f, area, &spines);

    let [ws_heading, ws_body] = theme::heading_and_body(ws_pane);
    let [repos_heading, repos_body] = theme::heading_and_body(repos_pane);
    let [info_heading, info_body] = theme::heading_and_body(cols[2]);

    // ── Workspaces ────────────────────────────────────────────────────────────
    let ws_items: Vec<ListItem> = app
        .workspace_view
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let is_sel = i == app.workspace_view.ws_idx;
            let is_active = is_sel && ws_active;

            let dirty: usize = ws.repos.iter().filter(|r| r.dirty).count();
            let ahead: usize = ws.repos.iter().map(|r| r.ahead).sum();
            let behind: usize = ws.repos.iter().map(|r| r.behind).sum();
            let (sym, sym_color) = match (ahead > 0, behind > 0) {
                (true, true) => ("⇅", theme::WARN),
                (true, false) => ("↑", theme::WARN),
                (false, true) => ("↓", theme::WARN),
                (false, false) => ("✓", theme::OK),
            };

            ListItem::new(Line::from(vec![
                theme::caret(app, is_active),
                Span::styled(
                    format!("{:<18}", &ws.name),
                    Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                ),
                Span::styled(
                    format!("{}", ws.repos.len()),
                    Style::default().fg(theme::INK_FAINT),
                ),
                Span::styled(format!(" {} ", sym), Style::default().fg(sym_color)),
                if dirty > 0 {
                    Span::styled(format!("*{}", dirty), Style::default().fg(theme::WARN))
                } else {
                    Span::raw("")
                },
            ]))
            .style(row_style(app, is_sel))
        })
        .collect();

    let mut ws_state = ListState::default();
    ws_state.select(Some(app.workspace_view.ws_idx));

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "workspaces",
        Some(app.workspace_view.workspaces.len()),
        ws_active,
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), ws_heading);
    f.render_stateful_widget(
        List::new(ws_items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        ws_body,
        &mut ws_state,
    );

    // ── Repos of the selected workspace ───────────────────────────────────────
    let mut sel_repo_pos = 0usize;
    let repo_items: Vec<ListItem> = app
        .workspace_view
        .workspaces
        .get(app.workspace_view.ws_idx)
        .map(|ws| {
            ws.repos
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let is_sel = focus_repos && i == app.workspace_view.repo_idx;
                    if is_sel {
                        sel_repo_pos = i;
                    }
                    ListItem::new(Line::from(vec![
                        theme::caret(app, is_sel && focused),
                        Span::styled(
                            format!("{:<20}", repo_name(&r.path)),
                            Style::default()
                                .fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                        ),
                        Span::styled(
                            format!(" {:<10}", &r.branch),
                            Style::default().fg(theme::OK),
                        ),
                        sync_span(r.ahead, r.behind),
                        if r.dirty {
                            Span::styled(
                                " *",
                                Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw("")
                        },
                    ]))
                    .style(row_style(app, is_sel))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut repo_state = ListState::default();
    if focus_repos {
        repo_state.select(Some(app.workspace_view.repo_idx));
    }

    let ws_name = app
        .workspace_view
        .workspaces
        .get(app.workspace_view.ws_idx)
        .map(|ws| ws.name.as_str())
        .unwrap_or("");
    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("repos", None, repos_active));
    heading.push(Span::styled(
        format!("  {}", ws_name),
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), repos_heading);
    f.render_stateful_widget(
        List::new(repo_items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        repos_body,
        &mut repo_state,
    );

    // ── Info ──────────────────────────────────────────────────────────────────
    let mut info_title = vec![Span::raw(" ")];
    info_title.extend(theme::panel_title("info", None, false));
    f.render_widget(Paragraph::new(Line::from(info_title)), info_heading);

    let ws = app.workspace_view.workspaces.get(app.workspace_view.ws_idx);
    let info_lines: Vec<Line> = if focus_repos {
        match ws.and_then(|ws| ws.repos.get(app.workspace_view.repo_idx)) {
            Some(r) => {
                let (sync_text, sync_color) = match (r.ahead > 0, r.behind > 0) {
                    (true, true) => (format!("↑{} ↓{}", r.ahead, r.behind), theme::WARN),
                    (true, false) => (format!("↑{} ahead", r.ahead), theme::WARN),
                    (false, true) => (format!("↓{} behind", r.behind), theme::WARN),
                    (false, false) => ("synced".to_string(), theme::OK),
                };
                vec![
                    field(
                        "name",
                        Span::styled(
                            repo_name(&r.path),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                    ),
                    field(
                        "branch",
                        Span::styled(
                            r.branch.clone(),
                            Style::default().fg(theme::OK).add_modifier(Modifier::BOLD),
                        ),
                    ),
                    field("sync", Span::styled(sync_text, Style::default().fg(sync_color))),
                    field(
                        "dirty",
                        Span::styled(
                            if r.dirty { "yes" } else { "no" },
                            Style::default().fg(if r.dirty { theme::WARN } else { theme::OK }),
                        ),
                    ),
                    field(
                        "path",
                        Span::styled(r.path.clone(), Style::default().fg(theme::INK_FAINT)),
                    ),
                ]
            }
            None => vec![Line::from(Span::styled(
                "  no repo selected",
                Style::default().fg(theme::INK_FAINT),
            ))],
        }
    } else {
        match ws {
            Some(ws) => {
                let dirty: usize = ws.repos.iter().filter(|r| r.dirty).count();
                let ahead: usize = ws.repos.iter().map(|r| r.ahead).sum();
                let behind: usize = ws.repos.iter().map(|r| r.behind).sum();
                vec![
                    field(
                        "name",
                        Span::styled(
                            ws.name.clone(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                    ),
                    field(
                        "repos",
                        Span::styled(
                            format!("{}", ws.repos.len()),
                            Style::default().fg(theme::INK),
                        ),
                    ),
                    field("ahead", count_span(ahead)),
                    field("behind", count_span(behind)),
                    field("dirty", count_span(dirty)),
                ]
            }
            None => vec![],
        }
    };
    f.render_widget(Paragraph::new(info_lines), info_body);

    if app.workspace_view.ops_mode {
        let anchor = if focus_repos { repos_body } else { ws_body };
        let row = if focus_repos {
            sel_repo_pos
        } else {
            app.workspace_view.ws_idx
        };
        render_ops(f, app, anchor, row, focus_repos);
    }
}

/// The ops dropdown, anchored under the selected entry of the focused column.
fn render_ops(f: &mut Frame, app: &App, body: Rect, row: usize, on_repo: bool) {
    let ops: &[(&str, bool)] = if on_repo {
        &[
            ("open repo", false),
            ("sync repo", false),
            ("sync workspace", false),
            ("remove from ws ⚠", true),
        ]
    } else {
        &[
            ("sync all", false),
            ("save all…", false),
            ("rename…", false),
            ("add repo…", false),
            ("delete ws ⚠", true),
        ]
    };

    let dropdown_w = 22u16;
    let dropdown_h = ops.len() as u16 + 2;
    let entry_y = body.y + row as u16 + 1;
    let drop_y = if entry_y + dropdown_h < body.y + body.height {
        entry_y
    } else {
        body.y + body.height.saturating_sub(dropdown_h)
    };
    let drop_area = Rect::new(body.x + 2, drop_y, dropdown_w, dropdown_h);

    let items: Vec<ListItem> = ops
        .iter()
        .enumerate()
        .map(|(i, (label, danger))| {
            let is_sel = i == app.workspace_view.ops_idx;
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
    state.select(Some(app.workspace_view.ops_idx));

    // A popup keeps its box: it is a window, not a column.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE));

    f.render_widget(Clear, drop_area);
    f.render_stateful_widget(List::new(items).block(block), drop_area, &mut state);
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

fn field(label: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<8}", label),
            Style::default().fg(theme::INK_FAINT),
        ),
        value,
    ])
}

fn count_span(n: usize) -> Span<'static> {
    Span::styled(
        format!("{}", n),
        Style::default().fg(if n > 0 { theme::WARN } else { theme::INK_FAINT }),
    )
}

fn sync_span(ahead: usize, behind: usize) -> Span<'static> {
    match (ahead > 0, behind > 0) {
        (true, true) => Span::styled(
            format!("↑{} ↓{}", ahead, behind),
            Style::default().fg(theme::WARN),
        ),
        (true, false) => Span::styled(format!("↑{}", ahead), Style::default().fg(theme::WARN)),
        (false, true) => Span::styled(format!("↓{}", behind), Style::default().fg(theme::WARN)),
        (false, false) => Span::styled("✓", Style::default().fg(theme::OK)),
    }
}

fn repo_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
