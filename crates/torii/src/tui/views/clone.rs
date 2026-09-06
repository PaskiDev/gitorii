//! `clone` TUI view — the repositories an account can reach, and a box
//! for everything that list cannot name.
//!
//! The layout is one column: a header saying which host is being asked
//! and where a clone would land, the list itself, and the URL box under
//! a rule. Overlays (the platform dropdown, the running clone) are
//! drawn after the body so they sit on top of it.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::tui::app::{App, CloneFocus, CloneStatus};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;

    // where | rule | repositories | rule | url box
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(area);

    render_where(f, app, rows[0]);
    theme::hrule_content(f, rows[1], &[]);
    render_repos(f, app, rows[2], focused);
    theme::hrule_content(f, rows[3], &[]);
    render_url_box(f, app, rows[4]);

    if app.clone_view.focus == CloneFocus::Platforms {
        render_platform_menu(f, app, area);
    }
    if app.clone_view.run.is_some() {
        render_run(f, app, area);
    }
}

/// Which host is being asked, over which protocol, and where the clone
/// under the caret would land. Three lines that answer the three
/// questions the list itself cannot.
fn render_where(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("from", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let has_token = app.clone_platform_has_token(&app.clone_view.platform);
    let lines = vec![
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                app.clone_view.platform.clone(),
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  [p]  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                if app.clone_view.ssh { "ssh" } else { "https" },
                Style::default().fg(theme::INK_DIM),
            ),
            Span::styled("  [t]", Style::default().fg(theme::INK_FAINT)),
            // A missing token is the whole explanation for an empty
            // list, so it is said here rather than left to the error.
            if has_token {
                Span::raw("")
            } else {
                Span::styled("   no token", Style::default().fg(theme::WARN))
            },
        ]),
        Line::from(vec![
            Span::styled("  into  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                app.clone_view.dest_input.clone(),
                Style::default().fg(if app.clone_view.focus == CloneFocus::Dest {
                    theme::INK
                } else {
                    theme::INK_DIM
                }),
            ),
            Span::styled("  [d]", Style::default().fg(theme::INK_FAINT)),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), body);
}

fn render_repos(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "repositories",
        Some(app.clone_view.visible_len()),
        active,
    ));
    if !app.clone_view.filter.is_empty() {
        heading.push(Span::styled(
            format!("  /{}", app.clone_view.filter),
            Style::default().fg(theme::INK_DIM),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    // Nothing to list is three different situations, and saying which
    // one it is saves the user guessing at an empty panel.
    if app.clone_view.loading || app.clone_view.filtered.is_empty() {
        let msg = if app.clone_view.loading {
            vec![Line::from(Span::styled(
                "  asking…",
                Style::default().fg(theme::INK_FAINT),
            ))]
        } else if let Some(err) = &app.clone_view.error {
            vec![
                Line::from(Span::styled(
                    format!("  {}", err),
                    Style::default().fg(theme::BAD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  a URL still works — press [u]",
                    Style::default().fg(theme::INK_FAINT),
                )),
            ]
        } else if app.clone_view.repos.is_empty() {
            vec![Line::from(Span::styled(
                "  nothing here — [p] another host, [r] ask again, [u] a URL",
                Style::default().fg(theme::INK_FAINT),
            ))]
        } else {
            vec![Line::from(Span::styled(
                format!("  no repository matches “{}”", app.clone_view.filter),
                Style::default().fg(theme::INK_FAINT),
            ))]
        };
        f.render_widget(Paragraph::new(msg), body);
        return;
    }

    let items: Vec<ListItem> = app
        .clone_view
        .filtered
        .iter()
        .enumerate()
        .map(|(row, &i)| {
            let repo = &app.clone_view.repos[i];
            let is_sel = active && row == app.clone_view.idx;
            let desc = repo.description.as_deref().unwrap_or("");
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(
                    format!("{:<34}", truncate(&repo.slug, 33)),
                    Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                ),
                Span::styled(
                    format!("{:<8}", if repo.private { "private" } else { "public" }),
                    Style::default().fg(if repo.private {
                        theme::WARN
                    } else {
                        theme::INK_FAINT
                    }),
                ),
                Span::styled(truncate(desc, 44), Style::default().fg(theme::INK_FAINT)),
            ]))
            .style(theme::row_style(app, is_sel))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.clone_view.idx));
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
    app.hits.borrow_mut().rows(
        body,
        "clone",
        state.offset(),
        app.clone_view.filtered.len().saturating_sub(state.offset()),
    );
}

/// The box for a URL the list cannot offer. Always visible, because a
/// field you have to discover is a field nobody uses.
fn render_url_box(f: &mut Frame, app: &App, area: Rect) {
    let editing = app.clone_view.focus == CloneFocus::Url;
    let typed = app.clone_view.url_input.trim();
    let value = if typed.is_empty() && !editing {
        Span::styled(
            "paste a URL or a path — [u]",
            Style::default().fg(theme::INK_FAINT),
        )
    } else {
        Span::styled(
            app.clone_view.url_input.clone(),
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        )
    };
    let mut line = vec![
        Span::styled("  url   ", Style::default().fg(theme::INK_FAINT)),
        value,
    ];
    if editing {
        line.push(Span::styled("█", Style::default().fg(theme::accent(app))));
    } else if !typed.is_empty() {
        // A URL in the box beats the selection, so say so where the
        // selection is still visible right above it.
        line.push(Span::styled(
            "   Enter clones this",
            Style::default().fg(theme::INK_FAINT),
        ));
    }
    f.render_widget(Paragraph::new(vec![Line::from(line)]), area);
}

/// The host dropdown. Same box as the log's author menu, in the same
/// place a menu opens everywhere else in the TUI.
fn render_platform_menu(f: &mut Frame, app: &App, area: Rect) {
    let rows = app.clone_platform_rows();
    if rows.is_empty() || area.height < 3 {
        return;
    }
    let w: u16 = 34;
    let h = (rows.len() as u16 + 2).min(area.height);
    let popup = Rect::new(area.x + 3, area.y + 1, w.min(area.width), h);
    let popup = theme::clear(f, popup);

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_sel = i == app.clone_view.platform_idx;
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(
                    format!("{:<14}", row.name),
                    Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                ),
                // Not a gate, a hint: a host with no token is still
                // selectable, and the error it gives names the command
                // that fixes it.
                Span::styled(
                    if row.has_token { "token" } else { "—" },
                    Style::default().fg(if row.has_token {
                        theme::OK
                    } else {
                        theme::INK_FAINT
                    }),
                ),
            ]))
            .style(theme::row_style(app, is_sel))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.clone_view.platform_idx));
    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    " host — Enter switch · Esc close ",
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

/// The clone itself: what is being cloned, where to, and how far along.
/// It stays up after it ends — a clone that scrolled past is a clone you
/// have to go looking for.
fn render_run(f: &mut Frame, app: &App, area: Rect) {
    let Some(run) = app.clone_view.run.as_ref() else {
        return;
    };
    let w: u16 = 66;
    let h: u16 = 8;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    let popup = theme::clear(f, popup);

    let mut body = vec![
        Line::from(vec![
            Span::styled(" from  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(truncate(&run.url, 55), Style::default().fg(theme::INK_DIM)),
        ]),
        Line::from(vec![
            Span::styled(" into  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(truncate(&run.dest, 55), Style::default().fg(theme::INK_DIM)),
        ]),
        Line::from(""),
    ];

    match &run.status {
        // Before anything else: a stop that has been asked for and not
        // yet answered. The bar is dropped on purpose — it would keep
        // climbing for another report or two, which reads as the key
        // having done nothing.
        CloneStatus::Starting | CloneStatus::Working(_) if run.cancel_asked() => {
            body.push(Line::from(Span::styled(
                " stopping…",
                Style::default().fg(theme::WARN),
            )));
        }
        CloneStatus::Starting => body.push(Line::from(Span::styled(
            " connecting…",
            Style::default().fg(theme::INK),
        ))),
        CloneStatus::Working(p) => {
            let width = 40usize;
            let filled = (p.percent().min(100) * width) / 100;
            body.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("█".repeat(filled), Style::default().fg(theme::accent(app))),
                Span::styled(
                    "░".repeat(width.saturating_sub(filled)),
                    Style::default().fg(theme::RULE),
                ),
                Span::styled(
                    format!("  {:>3}%", p.percent().min(100)),
                    Style::default().fg(theme::INK),
                ),
            ]));
            let mb = p.received_bytes as f64 / (1024.0 * 1024.0);
            body.push(Line::from(Span::styled(
                if p.resolving_deltas() {
                    format!(" resolving deltas  {}/{}", p.indexed_deltas, p.total_deltas)
                } else {
                    format!(
                        " {}/{} objects  {:.1} MB",
                        p.received_objects, p.total_objects, mb
                    )
                },
                Style::default().fg(theme::INK_FAINT),
            )));
        }
        CloneStatus::Done(path) => {
            body.push(Line::from(vec![
                Span::styled(" ✓ ", Style::default().fg(theme::OK)),
                Span::styled(
                    "cloned",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ),
            ]));
            body.push(Line::from(Span::styled(
                format!("   {}", truncate(path, 58)),
                Style::default().fg(theme::INK_FAINT),
            )));
        }
        CloneStatus::Cancelled => {
            body.push(Line::from(vec![
                Span::styled(" — ", Style::default().fg(theme::WARN)),
                Span::styled(
                    "stopped",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ),
            ]));
            body.push(Line::from(Span::styled(
                "   nothing was left on disk",
                Style::default().fg(theme::INK_FAINT),
            )));
        }
        CloneStatus::Error(msg) => {
            body.push(Line::from(vec![
                Span::styled(" ✗ ", Style::default().fg(theme::BAD)),
                Span::styled(truncate(msg, 58), Style::default().fg(theme::INK)),
            ]));
        }
    }

    let title = match &run.status {
        CloneStatus::Done(_) => " cloned — Esc dismiss ",
        CloneStatus::Cancelled => " stopped — Esc dismiss ",
        CloneStatus::Error(_) => " failed — Esc dismiss ",
        _ if run.cancel_asked() => " stopping ",
        _ => " cloning — Esc stops it ",
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

/// Cut to `max` characters, with an ellipsis standing for what was cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}
