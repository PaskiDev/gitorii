//! `auth` TUI view — show every credential torii knows about (cloud
//! key + per-provider tokens) with masked values and the source of
//! each. Mirrors `torii auth list` / `torii auth doctor` from the CLI.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::tui::app::{App, AuthEntry, AuthFocus, AuthState};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_GREEN, C_RED};

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
                Span::styled(format!(" {:<10}", e.provider), Style::default().fg(bc)),
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

    // Overlays — drawn after the body so they stack on top.
    match app.auth_view.focus {
        AuthFocus::OpsDropdown   => render_ops_dropdown(f, app, area),
        AuthFocus::InputToken    => render_input_overlay(f, app, area),
        AuthFocus::ConfirmRemove => render_confirm_remove(f, app, area),
        AuthFocus::List          => {}
    }
}

/// Contextual operations for the selected provider. The list is built
/// from the current state — already-set tokens get a Remove option;
/// providers without an OAuth flow get the "Set token" path only.
pub fn ops_for(state: &AuthState) -> Vec<(&'static str, &'static str)> {
    let Some(entry) = state.items.get(state.idx) else {
        return Vec::new();
    };
    let provider = entry.provider.as_str();
    let has_token = entry.masked.is_some();
    let mut ops: Vec<(&'static str, &'static str)> = Vec::new();

    let device_supported  = crate::oauth::device_flow_supported(provider);
    let code_supported    = crate::oauth::auth_code_flow_supported(provider);
    let oauth_supported   = device_supported || code_supported;

    if oauth_supported {
        ops.push(("OAuth re-auth", "run device / auth-code flow and save token"));
    }
    if has_token && oauth_supported {
        ops.push(("Rotate (OAuth)", "re-auth, replace, best-effort revoke old"));
    }
    if has_token && provider == "gitlab" {
        ops.push(("Rotate as PAT (GitLab)", "POST /personal_access_tokens/self/rotate"));
    }
    ops.push(("Set token (paste)", "type / paste the token manually"));
    if has_token {
        ops.push(("Remove token", "delete from auth.toml ⚠"));
    }
    ops
}

fn render_ops_dropdown(f: &mut Frame, app: &App, area: Rect) {
    let ops = ops_for(&app.auth_view);
    if ops.is_empty() { return; }
    let bc = app.brand_color();

    let w: u16 = 44;
    let h: u16 = ops.len() as u16 + 2;
    let popup = Rect {
        x: area.x + 4,
        y: area.y + 4,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = ops.iter().enumerate().map(|(i, (label, desc))| {
        let is_sel = i == app.auth_view.dropdown_idx;
        let danger = label.starts_with("Remove");
        let label_color = if danger { C_RED }
                          else if is_sel { C_WHITE }
                          else { C_SUBTLE };
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        let prefix = if is_sel { "▶ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(bc)),
            Span::styled(format!("{:<22}", label), Style::default().fg(label_color)),
            Span::styled(*desc, Style::default().fg(C_DIM)),
        ])).style(style)
    }).collect();

    let mut state = ListState::default();
    state.select(Some(app.auth_view.dropdown_idx));
    let title = format!(" ops · {} — Enter run · Esc close ", app.auth_view.pending_provider);
    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(title, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
        &mut state,
    );
}

fn render_input_overlay(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let w: u16 = 60;
    let h: u16 = 5;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    // We mask the input so a paste of a sensitive token doesn't end
    // up rendered on screen — same convention as `auth set <provider> -`
    // hides stdin echo.
    let dots = "•".repeat(app.auth_view.input_buffer.chars().count());
    let body = vec![
        Line::from(Span::styled(
            format!(" {}", app.auth_view.input_prompt),
            Style::default().fg(C_WHITE),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(dots, Style::default().fg(C_GREEN)),
            Span::styled("█", Style::default().fg(bc)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " paste · Enter save · Esc cancel ",
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
    );
}

fn render_confirm_remove(f: &mut Frame, app: &App, area: Rect) {
    let w: u16 = 50;
    let h: u16 = 5;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);
    let body = vec![
        Line::from(Span::styled(
            format!("  Remove `{}` token from auth.toml?", app.auth_view.pending_provider),
            Style::default().fg(C_WHITE),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  [y] yes   [n] no",
            Style::default().fg(C_DIM),
        )),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " confirm ",
                    Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_RED)),
        ),
        popup,
    );
}
