//! `auth` TUI view — show every credential torii knows about (cloud
//! key + per-provider tokens) with masked values and the source of
//! each. Mirrors `torii auth list` / `torii auth doctor` from the CLI.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::tui::app::{App, AuthEntry};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_YELLOW, C_GREEN};

pub fn refresh(app: &mut App) {
    app.auth_view.items.clear();
    app.auth_view.status = None;

    for &p in crate::auth::PROVIDERS {
        let r = crate::auth::resolve_token(p, ".");
        let (masked, source) = match (&r.value, &r.source) {
            (Some(v), src) => (Some(mask(v)), describe_source(src)),
            (None, _) => (None, "(not set)".to_string()),
        };
        app.auth_view.items.push(AuthEntry {
            provider: p.to_string(),
            masked,
            source,
        });
    }
    if app.auth_view.idx >= app.auth_view.items.len() {
        app.auth_view.idx = app.auth_view.items.len().saturating_sub(1);
    }

    // Cloud key state.
    let cloud = crate::auth::load();
    app.auth_view.cloud_key_set = cloud.is_some();
    app.auth_view.cloud_endpoint = cloud
        .map(|c| c.endpoint)
        .unwrap_or_else(crate::auth::default_endpoint);
}

fn mask(t: &str) -> String {
    let chars: Vec<char> = t.chars().collect();
    if chars.len() < 12 {
        return "****".to_string();
    }
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{head}…{tail}")
}

fn describe_source(s: &crate::auth::TokenSource) -> String {
    match s {
        crate::auth::TokenSource::EnvVar(name) => format!("env: ${name}"),
        crate::auth::TokenSource::EnvGeneric => "env: $TORII_HTTPS_TOKEN".to_string(),
        crate::auth::TokenSource::Local => "local .torii/auth.toml".to_string(),
        crate::auth::TokenSource::Global => "global ~/.config/torii/auth.toml".to_string(),
        crate::auth::TokenSource::Missing => "(not set)".to_string(),
    }
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(1)])
        .split(area);

    // ── Cloud panel ──────────────────────────────────────────────────────
    let cloud_lines: Vec<Line> = if app.auth_view.cloud_key_set {
        vec![
            Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(C_GREEN)),
                Span::styled(
                    "gitorii.com API key set",
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("    endpoint  ", Style::default().fg(C_SUBTLE)),
                Span::styled(&app.auth_view.cloud_endpoint, Style::default().fg(C_DIM)),
            ]),
            Line::from(vec![]),
            Line::from(vec![Span::styled(
                "    CLI: torii auth status / torii auth logout",
                Style::default().fg(C_DIM),
            )]),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("  — ", Style::default().fg(C_DIM)),
                Span::styled(
                    "gitorii.com API key not set",
                    Style::default().fg(C_WHITE),
                ),
            ]),
            Line::from(vec![]),
            Line::from(vec![Span::styled(
                "    CLI: torii auth login",
                Style::default().fg(C_DIM),
            )]),
        ]
    };
    let cloud_block = Block::default()
        .title(Span::styled(" cloud ", Style::default().fg(bc)))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(Paragraph::new(cloud_lines).block(cloud_block), chunks[0]);

    // ── Provider tokens list ─────────────────────────────────────────────
    let items: Vec<ListItem> = app
        .auth_view
        .items
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let is_sel = i == app.auth_view.idx;
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let (value_str, color) = match &e.masked {
                Some(m) => (m.clone(), C_GREEN),
                None => ("—".to_string(), C_DIM),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {:<10}", e.provider), Style::default().fg(C_YELLOW)),
                Span::styled(format!(" {:<22}", value_str), Style::default().fg(color)),
                Span::styled(&e.source, Style::default().fg(C_SUBTLE)),
            ]))
            .style(style)
        })
        .collect();

    let mut state = ListState::default();
    if !app.auth_view.items.is_empty() {
        state.select(Some(app.auth_view.idx));
    }
    let list_block = Block::default()
        .title(Span::styled(
            " tokens ",
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
    f.render_stateful_widget(List::new(items).block(list_block), chunks[1], &mut state);
}
