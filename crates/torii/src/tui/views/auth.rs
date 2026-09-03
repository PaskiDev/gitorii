//! `auth` TUI view — show every credential torii knows about (cloud
//! key + per-provider tokens) with masked values and the source of
//! each. Mirrors `torii auth list` / `torii auth doctor` from the CLI.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::tui::app::{App, AuthEntry, AuthFocus, AuthState, OauthStatus};
use crate::tui::theme;

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
    let focused = !app.sidebar_focused;

    // cloud | rule | tokens
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Min(2),
        ])
        .split(area);

    render_cloud(f, app, rows[0]);
    theme::hrule_content(f, rows[1], &[]);
    render_tokens(f, app, rows[2], focused);

    // Overlays — drawn after the body so they stack on top.
    match app.auth_view.focus {
        AuthFocus::OpsDropdown => render_ops_dropdown(f, app, area),
        AuthFocus::InputToken => render_input_overlay(f, app, area),
        AuthFocus::ConfirmRemove => render_confirm_remove(f, app, area),
        AuthFocus::OauthFlow => render_oauth_flow(f, app, area),
        AuthFocus::List => {}
    }
}

fn render_cloud(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("cloud", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let lines: Vec<Line> = if app.auth_view.cloud_key_set {
        vec![
            Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(theme::OK)),
                Span::styled(
                    "gitorii.com API key set",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("    endpoint  ", Style::default().fg(theme::INK_FAINT)),
                Span::styled(
                    &app.auth_view.cloud_endpoint,
                    Style::default().fg(theme::INK_DIM),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "    CLI: torii auth status / torii auth logout",
                Style::default().fg(theme::INK_FAINT),
            )),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("  — ", Style::default().fg(theme::INK_FAINT)),
                Span::styled(
                    "gitorii.com API key not set",
                    Style::default().fg(theme::INK),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "    CLI: torii auth login",
                Style::default().fg(theme::INK_FAINT),
            )),
        ]
    };
    f.render_widget(Paragraph::new(lines), body);
}

fn render_tokens(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let items: Vec<ListItem> = app
        .auth_view
        .items
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let is_sel = active && i == app.auth_view.idx;
            // A set token is the thing worth reading here; a missing one is
            // an em dash, not an error.
            let (value, value_color) = match &e.masked {
                Some(m) => (m.clone(), theme::OK),
                None => ("—".to_string(), theme::INK_FAINT),
            };
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(
                    format!("{:<10}", e.provider),
                    Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                ),
                Span::styled(format!(" {:<22}", value), Style::default().fg(value_color)),
                Span::styled(&e.source, Style::default().fg(theme::INK_FAINT)),
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
    if !app.auth_view.items.is_empty() {
        state.select(Some(app.auth_view.idx));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "tokens",
        Some(app.auth_view.items.len()),
        active,
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
    app.hits.borrow_mut().rows(
        body,
        "auth",
        state.offset(),
        app.auth_view.items.len().saturating_sub(state.offset()),
    );
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

    let device_supported = crate::oauth::device_flow_supported(provider);
    let code_supported = crate::oauth::auth_code_flow_supported(provider);
    let oauth_supported = device_supported || code_supported;

    if oauth_supported {
        ops.push((
            "OAuth re-auth",
            "run device / auth-code flow and save token",
        ));
    }
    if has_token && oauth_supported {
        ops.push(("Rotate (OAuth)", "re-auth, replace, best-effort revoke old"));
    }
    if has_token && provider == "gitlab" {
        ops.push((
            "Rotate as PAT (GitLab)",
            "POST /personal_access_tokens/self/rotate",
        ));
    }
    ops.push(("Set token (paste)", "type / paste the token manually"));
    if has_token {
        ops.push(("Remove token", "delete from auth.toml ⚠"));
    }
    ops
}

fn render_ops_dropdown(f: &mut Frame, app: &App, area: Rect) {
    let ops = ops_for(&app.auth_view);
    if ops.is_empty() {
        return;
    }

    let w: u16 = 44;
    let h: u16 = ops.len() as u16 + 2;
    let popup = Rect {
        x: area.x + 4,
        y: area.y + 4,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = ops
        .iter()
        .enumerate()
        .map(|(i, (label, desc))| {
            let is_sel = i == app.auth_view.dropdown_idx;
            let label_color = if label.starts_with("Remove") {
                theme::BAD
            } else if is_sel {
                theme::INK
            } else {
                theme::INK_DIM
            };
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(format!("{:<22}", label), Style::default().fg(label_color)),
                Span::styled(*desc, Style::default().fg(theme::INK_FAINT)),
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
    state.select(Some(app.auth_view.dropdown_idx));
    let title = format!(
        " ops · {} — Enter run · Esc close ",
        app.auth_view.pending_provider
    );
    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    title,
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::RULE)),
        ),
        popup,
        &mut state,
    );
}

fn render_input_overlay(f: &mut Frame, app: &App, area: Rect) {
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
            Style::default().fg(theme::INK),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(dots, Style::default().fg(theme::OK)),
            Span::styled("█", Style::default().fg(theme::accent(app))),
        ]),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " paste · Enter save · Esc cancel ",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::RULE)),
        ),
        popup,
    );
}

/// Modal for the in-TUI OAuth flow. Renders the URL + user code while
/// the worker thread polls in the background; switches to a success /
/// error message when the worker signals completion.
fn render_oauth_flow(f: &mut Frame, app: &App, area: Rect) {
    let Some(state) = app.auth_view.oauth_flow.as_ref() else {
        return;
    };

    let w: u16 = 72.min(area.width.saturating_sub(4));
    let h: u16 = 13.min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);

    let mut body: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("  provider  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                &state.provider,
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled(
                if state.rotate { "rotate" } else { "re-auth" },
                Style::default().fg(theme::INK_FAINT),
            ),
        ]),
        Line::from(""),
    ];

    match &state.status {
        OauthStatus::Starting => {
            body.push(Line::from(Span::styled(
                "  ▰▱▱▱▱▱▱▱▱▱  requesting device code…",
                Style::default().fg(theme::WARN),
            )));
        }
        OauthStatus::Waiting {
            display_uri,
            user_code,
        } => {
            body.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    "1. Open this URL in your browser:",
                    Style::default().fg(theme::INK),
                ),
            ]));
            body.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    display_uri.clone(),
                    Style::default()
                        .fg(theme::accent(app))
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            body.push(Line::from(""));
            body.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("2. Confirm the user code:", Style::default().fg(theme::INK)),
            ]));
            body.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    user_code.clone(),
                    Style::default().fg(theme::OK).add_modifier(Modifier::BOLD),
                ),
            ]));
            body.push(Line::from(""));
            let bar = progress_bar(app.tick / 2);
            body.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(bar, Style::default().fg(theme::WARN)),
                Span::styled(
                    "  waiting for authorisation…",
                    Style::default().fg(theme::WARN),
                ),
            ]));
        }
        OauthStatus::Saving => {
            body.push(Line::from(Span::styled(
                "  ▰▰▰▰▰▰▰▰▱▱  authorised, saving token…",
                Style::default().fg(theme::WARN),
            )));
        }
        OauthStatus::Done(masked) => {
            body.push(Line::from(Span::styled(
                "  ▰▰▰▰▰▰▰▰▰▰",
                Style::default().fg(theme::OK),
            )));
            body.push(Line::from(""));
            body.push(Line::from(vec![
                Span::styled("  ✓ ", Style::default().fg(theme::OK)),
                Span::styled(
                    format!("token saved: {}", masked),
                    Style::default().fg(theme::INK),
                ),
            ]));
            body.push(Line::from(""));
            body.push(Line::from(Span::styled(
                "  any key closes this",
                Style::default().fg(theme::INK_FAINT),
            )));
        }
        OauthStatus::Error(msg) => {
            body.push(Line::from(Span::styled(
                "  ▰▰▰▰▰▰▰▰▰▰",
                Style::default().fg(theme::BAD),
            )));
            body.push(Line::from(""));
            body.push(Line::from(vec![
                Span::styled("  ✗ ", Style::default().fg(theme::BAD)),
                Span::styled(msg.clone(), Style::default().fg(theme::INK)),
            ]));
            body.push(Line::from(""));
            body.push(Line::from(Span::styled(
                "  any key closes this",
                Style::default().fg(theme::INK_FAINT),
            )));
        }
    }

    let title = if state.rotate {
        " OAuth rotate — Esc cancel "
    } else {
        " OAuth re-auth — Esc cancel "
    };

    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    title,
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::RULE)),
        ),
        popup,
    );
}

/// Same bouncing-ball progress bar that `sync` uses, so the in-TUI
/// OAuth modal feels like the rest of the chrome.
fn progress_bar(tick: usize) -> String {
    const TOTAL: usize = 10;
    const CYCLE: usize = (TOTAL - 1) * 2;
    let pos = tick % CYCLE;
    let ball = if pos < TOTAL { pos } else { CYCLE - pos };
    (0..TOTAL)
        .map(|i| if i == ball { '▰' } else { '▱' })
        .collect()
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
            format!(
                "  Remove `{}` token from auth.toml?",
                app.auth_view.pending_provider
            ),
            Style::default().fg(theme::INK),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("y", Style::default().fg(theme::accent(app))),
            Span::styled("  yes   ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("n", Style::default().fg(theme::accent(app))),
            Span::styled("  no", Style::default().fg(theme::INK_FAINT)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " confirm ",
                    Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::BAD)),
        ),
        popup,
    );
}
