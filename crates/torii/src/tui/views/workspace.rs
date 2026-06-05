use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::tui::app::{App, WorkspaceFocus};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_CYAN, C_YELLOW, C_GREEN, C_RED};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    if app.workspace_view.workspaces.is_empty() {
        let block = Block::default()
            .title(Span::styled(" workspaces ", Style::default().fg(bc)))
            .borders(Borders::ALL).border_type(app.border_type())
            .border_style(Style::default().fg(bc));
        f.render_widget(
            Paragraph::new(Span::styled(
                "  no workspaces — run `torii workspace add <name> <path>` to create one",
                Style::default().fg(C_DIM),
            )).block(block),
            area,
        );
        return;
    }

    let focus_ws    = app.workspace_view.focus == WorkspaceFocus::Workspaces;
    let focus_repos = !focus_ws;

    // ── Layout: workspaces(26) | repos(min) | info(36) ───────────────────────
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(26),
            Constraint::Min(1),
            Constraint::Length(36),
        ])
        .split(area);

    // ── Workspaces list ───────────────────────────────────────────────────────
    let ws_items: Vec<ListItem> = app.workspace_view.workspaces.iter().enumerate().map(|(i, ws)| {
        let is_sel    = i == app.workspace_view.ws_idx;
        let is_active = is_sel && focus_ws && focused;
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let prefix = if is_active { "█ " } else if is_sel { "▶ " } else { "  " };

        let dirty: usize        = ws.repos.iter().filter(|r| r.dirty).count();
        let ahead_total: usize  = ws.repos.iter().map(|r| r.ahead).sum();
        let behind_total: usize = ws.repos.iter().map(|r| r.behind).sum();
        let sync_color = if ahead_total > 0 || behind_total > 0 { C_YELLOW } else { C_GREEN };
        let sync_sym = if ahead_total > 0 && behind_total > 0 { "⇅" }
            else if ahead_total > 0 { "↑" }
            else if behind_total > 0 { "↓" }
            else { "✓" };

        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(bc)),
            Span::styled(format!("{:<18}", &ws.name), Style::default().fg(if is_sel { C_WHITE } else { C_SUBTLE })),
            Span::styled(format!("{}", ws.repos.len()), Style::default().fg(C_DIM)),
            Span::styled(format!(" {} ", sync_sym), Style::default().fg(sync_color)),
            if dirty > 0 { Span::styled(format!("*{}", dirty), Style::default().fg(C_YELLOW)) }
            else { Span::raw("") },
        ])).style(style)
    }).collect();

    let mut ws_state = ListState::default();
    ws_state.select(Some(app.workspace_view.ws_idx));

    let ws_active = focus_ws && focused;
    let ws_block = Block::default()
        .title(Span::styled(" workspaces ",
            if ws_active { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(if ws_active { C_WHITE } else { bc }));
    f.render_stateful_widget(List::new(ws_items).block(ws_block), cols[0], &mut ws_state);

    // ── Repos list ────────────────────────────────────────────────────────────
    let repos_active = focus_repos && focused;

    let mut sel_repo_pos = 0usize;
    let repo_items: Vec<ListItem> = app.workspace_view.workspaces
        .get(app.workspace_view.ws_idx)
        .map(|ws| {
            ws.repos.iter().enumerate().map(|(i, r)| {
                let is_sel = focus_repos && i == app.workspace_view.repo_idx;
                if is_sel { sel_repo_pos = i; }
                let style = if is_sel {
                    Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let prefix = if is_sel && focused { "█ " } else if is_sel { "▶ " } else { "  " };

                let name = std::path::Path::new(&r.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| r.path.clone());

                let sync_span = if r.ahead > 0 && r.behind > 0 {
                    Span::styled(format!("↑{} ↓{}", r.ahead, r.behind), Style::default().fg(C_YELLOW))
                } else if r.ahead > 0 {
                    Span::styled(format!("↑{}", r.ahead), Style::default().fg(C_CYAN))
                } else if r.behind > 0 {
                    Span::styled(format!("↓{}", r.behind), Style::default().fg(C_RED))
                } else {
                    Span::styled("✓", Style::default().fg(C_GREEN))
                };

                let dirty_span = if r.dirty {
                    Span::styled(" *", Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD))
                } else {
                    Span::raw("")
                };

                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(bc)),
                    Span::styled(format!("{:<20}", name), Style::default().fg(if is_sel { C_WHITE } else { C_SUBTLE })),
                    Span::styled(format!(" {:<10}", &r.branch), Style::default().fg(C_GREEN)),
                    sync_span,
                    dirty_span,
                ])).style(style)
            }).collect()
        })
        .unwrap_or_default();

    let mut repo_state = ListState::default();
    if focus_repos { repo_state.select(Some(app.workspace_view.repo_idx)); }

    let ws_name = app.workspace_view.workspaces
        .get(app.workspace_view.ws_idx)
        .map(|ws| ws.name.as_str())
        .unwrap_or("");
    let repos_block = Block::default()
        .title(Span::styled(
            format!(" {} — repos ", ws_name),
            if repos_active { Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(bc) },
        ))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(if repos_active { C_WHITE } else { bc }));
    f.render_stateful_widget(List::new(repo_items).block(repos_block), cols[1], &mut repo_state);

    // ── Info panel ────────────────────────────────────────────────────────────
    let info_lines: Vec<Line> = if focus_repos {
        if let Some(ws) = app.workspace_view.workspaces.get(app.workspace_view.ws_idx) {
            if let Some(r) = ws.repos.get(app.workspace_view.repo_idx) {
                let name = std::path::Path::new(&r.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| r.path.clone());
                let sync_str = if r.ahead > 0 && r.behind > 0 {
                    format!("↑{} ↓{}", r.ahead, r.behind)
                } else if r.ahead > 0 {
                    format!("↑{} ahead", r.ahead)
                } else if r.behind > 0 {
                    format!("↓{} behind", r.behind)
                } else {
                    "synced".to_string()
                };
                let sync_color = if r.ahead > 0 || r.behind > 0 { C_YELLOW } else { C_GREEN };
                vec![
                    Line::from(vec![
                        Span::styled("  name    ", Style::default().fg(C_SUBTLE)),
                        Span::styled(name, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("  branch  ", Style::default().fg(C_SUBTLE)),
                        Span::styled(&r.branch, Style::default().fg(C_GREEN).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("  sync    ", Style::default().fg(C_SUBTLE)),
                        Span::styled(sync_str, Style::default().fg(sync_color)),
                    ]),
                    Line::from(vec![
                        Span::styled("  dirty   ", Style::default().fg(C_SUBTLE)),
                        Span::styled(
                            if r.dirty { "yes" } else { "no" },
                            Style::default().fg(if r.dirty { C_YELLOW } else { C_GREEN }),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("  path    ", Style::default().fg(C_SUBTLE)),
                        Span::styled(&r.path, Style::default().fg(C_DIM)),
                    ]),
                ]
            } else {
                vec![Line::from(Span::styled("  no repo selected", Style::default().fg(C_DIM)))]
            }
        } else {
            vec![]
        }
    } else {
        if let Some(ws) = app.workspace_view.workspaces.get(app.workspace_view.ws_idx) {
            let total         = ws.repos.len();
            let dirty: usize  = ws.repos.iter().filter(|r| r.dirty).count();
            let ahead: usize  = ws.repos.iter().map(|r| r.ahead).sum();
            let behind: usize = ws.repos.iter().map(|r| r.behind).sum();
            vec![
                Line::from(vec![
                    Span::styled("  name    ", Style::default().fg(C_SUBTLE)),
                    Span::styled(&ws.name, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("  repos   ", Style::default().fg(C_SUBTLE)),
                    Span::styled(format!("{}", total), Style::default().fg(C_WHITE)),
                ]),
                Line::from(vec![
                    Span::styled("  ahead   ", Style::default().fg(C_SUBTLE)),
                    Span::styled(format!("{}", ahead), Style::default().fg(if ahead > 0 { C_CYAN } else { C_DIM })),
                ]),
                Line::from(vec![
                    Span::styled("  behind  ", Style::default().fg(C_SUBTLE)),
                    Span::styled(format!("{}", behind), Style::default().fg(if behind > 0 { C_RED } else { C_DIM })),
                ]),
                Line::from(vec![
                    Span::styled("  dirty   ", Style::default().fg(C_SUBTLE)),
                    Span::styled(format!("{}", dirty), Style::default().fg(if dirty > 0 { C_YELLOW } else { C_DIM })),
                ]),
            ]
        } else {
            vec![]
        }
    };

    let info_block = Block::default()
        .title(Span::styled(" info ", Style::default().fg(bc)))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(Paragraph::new(info_lines).block(info_block), cols[2]);

    // ── Ops dropdown ──────────────────────────────────────────────────────────
    if app.workspace_view.ops_mode {
        let (ops, dropdown_w): (&[(&str, bool)], u16) = if focus_repos {
            (&[
                ("open repo",          false),
                ("sync repo",          false),
                ("sync workspace",     false),
                ("remove from ws ⚠",  true),
            ], 22)
        } else {
            (&[
                ("sync all",           false),
                ("save all…",          false),
                ("rename…",            false),
                ("add repo…",          false),
                ("delete ws ⚠",       true),
            ], 22)
        };

        let dropdown_h = ops.len() as u16 + 2;
        let col = if focus_repos { cols[1] } else { cols[0] };
        let entry_y = col.y + 1 + sel_repo_pos as u16 + 1;
        let drop_y = if entry_y + dropdown_h < col.y + col.height {
            entry_y
        } else {
            col.y + col.height.saturating_sub(dropdown_h)
        };
        let drop_area = Rect::new(col.x + 2, drop_y, dropdown_w, dropdown_h);

        let drop_items: Vec<ListItem> = ops.iter().enumerate().map(|(i, (label, danger))| {
            let is_sel = i == app.workspace_view.ops_idx;
            let color  = if *danger { C_RED } else if is_sel { C_WHITE } else { C_SUBTLE };
            let prefix = if is_sel { "▶ " } else { "  " };
            let style  = if is_sel {
                Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(*label, Style::default().fg(color)),
            ])).style(style)
        }).collect();

        let mut drop_state = ListState::default();
        drop_state.select(Some(app.workspace_view.ops_idx));

        let drop_block = Block::default()
            .borders(Borders::ALL).border_type(app.border_type())
            .border_style(Style::default().fg(bc));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(List::new(drop_items).block(drop_block), drop_area, &mut drop_state);
    }
}
