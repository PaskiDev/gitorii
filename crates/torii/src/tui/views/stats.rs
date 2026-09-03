//! The stats view: what the repo — or the whole workspace — looks like from a
//! distance.
//!
//! Two modes on one screen. The repo mode is four blocks (activity, authors,
//! churn, shape); the workspace mode is a table with a totals line. Churn
//! arrives from a worker, so it says so while it is being measured rather than
//! showing an empty list that looks like an answer.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::stats::{human_bytes, sparkline};
use crate::tui::app::{App, StatsMode};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // mode strip
            Constraint::Length(1), // rule
            Constraint::Min(1),
        ])
        .split(area);

    render_mode_strip(f, app, rows[0]);

    match app.stats_view.mode {
        StatsMode::Repo => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
                .split(rows[2]);

            let divider = theme::divider_right();
            let left = divider.inner(panes[0]);
            f.render_widget(divider, panes[0]);
            let spine = panes[0].right().saturating_sub(1);
            theme::hrule_content(f, rows[1], &[(spine, theme::Tick::Down)]);
            theme::tie_below(f, rows[2], &[spine]);

            render_activity(f, app, left);
            render_shape(f, app, panes[1]);
        }
        StatsMode::Workspace => {
            theme::hrule_content(f, rows[1], &[]);
            render_workspace(f, app, rows[2]);
        }
        StatsMode::People => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
                .split(rows[2]);

            let divider = theme::divider_right();
            let left = divider.inner(panes[0]);
            f.render_widget(divider, panes[0]);
            let spine = panes[0].right().saturating_sub(1);
            theme::hrule_content(f, rows[1], &[(spine, theme::Tick::Down)]);
            theme::tie_below(f, rows[2], &[spine]);

            render_people(f, app, left);
            render_person(f, app, panes[1]);
        }
    }
}

fn render_mode_strip(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;
    let mode = app.stats_view.mode;
    let style = |mine: StatsMode| {
        if mode == mine && focused {
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD)
        } else if mode == mine {
            Style::default()
                .fg(theme::INK_DIM)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::INK_FAINT)
        }
    };
    let mut spans = vec![
        Span::raw(" "),
        Span::styled("1 repo", style(StatsMode::Repo)),
        Span::styled(" · ", Style::default().fg(theme::RULE)),
        Span::styled("2 workspace", style(StatsMode::Workspace)),
        Span::styled(" · ", Style::default().fg(theme::RULE)),
        Span::styled("3 people", style(StatsMode::People)),
    ];
    if app.stats_view.mode == StatsMode::Workspace {
        if let Some(ws) = &app.active_workspace {
            spans.push(Span::styled(
                format!("  {ws}"),
                Style::default().fg(theme::INK_FAINT),
            ));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Activity, authors and churn — the three answers that need history.
fn render_activity(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let sv = &app.stats_view;

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "activity",
        Some(sv.history.commits),
        !app.sidebar_focused,
    ));
    if sv.history.capped {
        // Say the number is a floor, not a total: silently capping would be a
        // lie the size of the repo.
        heading.push(Span::styled(
            format!("  first {} walked", crate::stats::HISTORY_CAP),
            Style::default().fg(theme::INK_FAINT),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let mut lines: Vec<Line> = Vec::new();

    let weeks: Vec<usize> = sv.history.weeks.to_vec();
    let recent: usize = weeks.iter().sum();
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<10}", "12 weeks"),
            Style::default().fg(theme::INK_FAINT),
        ),
        Span::styled(
            sparkline(&weeks),
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {recent} commits"),
            Style::default().fg(theme::INK_DIM),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<10}", "last"),
            Style::default().fg(theme::INK_FAINT),
        ),
        Span::styled(
            sv.history
                .last
                .map(age)
                .unwrap_or_else(|| "no commits yet".into()),
            Style::default().fg(theme::INK_DIM),
        ),
    ]));
    lines.push(Line::from(""));

    // Authors, as a share of what was walked.
    lines.push(Line::from(Span::styled(
        "authors",
        Style::default()
            .fg(theme::INK_FAINT)
            .add_modifier(Modifier::BOLD),
    )));
    let total = sv.history.commits.max(1);
    for (name, count) in sv.history.authors.iter().take(5) {
        let pct = count * 100 / total;
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<22}", truncate(name, 22)),
                Style::default().fg(theme::INK),
            ),
            Span::styled(format!("{count:>5}  "), Style::default().fg(theme::INK_DIM)),
            Span::styled(bar(pct, 10), Style::default().fg(theme::accent(app))),
            Span::styled(format!(" {pct:>3}%"), Style::default().fg(theme::INK_FAINT)),
        ]));
    }
    if sv.history.authors.is_empty() {
        lines.push(Line::from(Span::styled(
            "  nobody yet",
            Style::default().fg(theme::INK_FAINT),
        )));
    }
    lines.push(Line::from(""));

    // Churn, once the worker answers.
    lines.push(Line::from(Span::styled(
        "most touched",
        Style::default()
            .fg(theme::INK_FAINT)
            .add_modifier(Modifier::BOLD),
    )));
    match &sv.churn {
        None => lines.push(Line::from(Span::styled(
            "  measuring…",
            Style::default().fg(theme::WARN),
        ))),
        Some(churn) if churn.hot.is_empty() => lines.push(Line::from(Span::styled(
            "  nothing to measure",
            Style::default().fg(theme::INK_FAINT),
        ))),
        Some(churn) => {
            // The count leads and the path takes whatever is left: a path cut
            // at the right loses the file name, which is the useful half.
            let path_room = (area.width as usize).saturating_sub(12);
            for (path, count) in churn.hot.iter().take(6) {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {count:>4}× "),
                        Style::default().fg(theme::INK_FAINT),
                    ),
                    Span::styled(tail(path, path_room), Style::default().fg(theme::INK_DIM)),
                ]));
            }
            lines.push(Line::from(Span::styled(
                format!("  over the last {} commits", churn.commits),
                Style::default().fg(theme::INK_FAINT),
            )));
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

/// What the repo is made of, and where it stands.
fn render_shape(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let sv = &app.stats_view;

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("shape", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let field = |label: &str, value: Span<'static>| {
        Line::from(vec![
            Span::styled(
                format!("{label:<10}"),
                Style::default().fg(theme::INK_FAINT),
            ),
            value,
        ])
    };

    let mut lines = vec![
        field(
            "branch",
            Span::styled(
                sv.shape.branch.clone(),
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
        ),
        field(
            "state",
            if sv.shape.dirty == 0 {
                Span::styled("clean", Style::default().fg(theme::OK))
            } else {
                Span::styled(
                    format!("{} changed", sv.shape.dirty),
                    Style::default().fg(theme::WARN),
                )
            },
        ),
        field(
            "branches",
            Span::styled(
                format!(
                    "{} local · {} remote",
                    sv.shape.local_branches, sv.shape.remote_branches
                ),
                Style::default().fg(theme::INK_DIM),
            ),
        ),
        field(
            "tags",
            Span::styled(
                sv.shape.tags.to_string(),
                Style::default().fg(theme::INK_DIM),
            ),
        ),
        field(
            "remotes",
            Span::styled(
                sv.shape
                    .remotes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "none".to_string()),
                Style::default().fg(theme::INK_DIM),
            ),
        ),
        field(
            "files",
            Span::styled(
                format!("{} · {}", sv.shape.files, human_bytes(sv.shape.bytes)),
                Style::default().fg(theme::INK_DIM),
            ),
        ),
        Line::from(""),
        Line::from(Span::styled(
            "by extension",
            Style::default()
                .fg(theme::INK_FAINT)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    // A second remote goes on its own line rather than off the edge.
    for extra in sv.shape.remotes.iter().skip(1) {
        lines.insert(
            5,
            Line::from(vec![
                Span::raw(" ".repeat(10)),
                Span::styled(extra.clone(), Style::default().fg(theme::INK_DIM)),
            ]),
        );
    }

    let total = sv.shape.files.max(1);
    for (ext, count) in sv.shape.languages.iter().take(6) {
        let pct = count * 100 / total;
        lines.push(Line::from(vec![
            Span::styled(format!("  {ext:<8}"), Style::default().fg(theme::INK)),
            Span::styled(format!("{count:>5}  "), Style::default().fg(theme::INK_DIM)),
            Span::styled(bar(pct, 8), Style::default().fg(theme::accent(app))),
            Span::styled(format!(" {pct:>3}%"), Style::default().fg(theme::INK_FAINT)),
        ]));
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

/// One row per repo of the workspace, and a totals line.
fn render_workspace(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let rows = &app.stats_view.rows;

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "repos",
        Some(rows.len()),
        !app.sidebar_focused,
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  no workspace open — `torii workspace add <name> <path>`",
                Style::default().fg(theme::INK_FAINT),
            )),
            body,
        );
        return;
    }

    let name_w = rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(8);
    let branch_w = rows
        .iter()
        .map(|r| r.branch.chars().count())
        .max()
        .unwrap_or(8);

    let mut items: Vec<ListItem> = rows
        .iter()
        .map(|r| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<name_w$}  ", r.name),
                    Style::default().fg(theme::INK),
                ),
                Span::styled(
                    format!("{:<branch_w$}  ", r.branch),
                    Style::default().fg(theme::INK_DIM),
                ),
                sync_span(r.ahead, r.behind),
                if r.dirty > 0 {
                    Span::styled(
                        format!("  *{:<4}", r.dirty),
                        Style::default().fg(theme::WARN),
                    )
                } else {
                    Span::styled("  clean", Style::default().fg(theme::OK))
                },
                Span::styled(
                    format!("  {:>6} files  {:>9}", r.files, human_bytes(r.bytes)),
                    Style::default().fg(theme::INK_FAINT),
                ),
            ]))
        })
        .collect();

    let (files, bytes, dirty, ahead, behind) = app.stats_workspace_totals();
    items.push(ListItem::new(Line::from("")));
    items.push(ListItem::new(Line::from(vec![
        Span::styled(
            format!("{:<width$}", "totals", width = name_w + branch_w + 4),
            Style::default()
                .fg(theme::INK_FAINT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{files} files · {}", human_bytes(bytes)),
            Style::default().fg(theme::INK),
        ),
        Span::styled(
            format!("  ·  {dirty} dirty  ↑{ahead}  ↓{behind}"),
            Style::default().fg(if dirty > 0 || behind > 0 {
                theme::WARN
            } else {
                theme::OK
            }),
        ),
    ])));

    f.render_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

/// Everyone who has committed here, most commits first.
fn render_people(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let sv = &app.stats_view;
    let active = !app.sidebar_focused;

    let signed_total: usize = sv.people.iter().map(|p| p.signed).sum();
    let commits_total: usize = sv.people.iter().map(|p| p.commits).sum();

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("people", Some(sv.people.len()), active));
    heading.push(Span::styled(
        format!("  {}% signed", signed_total * 100 / commits_total.max(1)),
        Style::default().fg(if signed_total == 0 {
            theme::INK_FAINT
        } else {
            theme::OK
        }),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    // Caret, commits, signing share and age all have fixed widths; the name
    // takes what they leave. A name that still does not fit gets a second
    // line rather than an ellipsis — it is a person, not a column.
    const AFTER_NAME: usize = 2 + 7 + 6 + 9;
    let room = (area.width as usize).saturating_sub(2);
    let longest = sv
        .people
        .iter()
        .map(|p| p.name.chars().count())
        .max()
        .unwrap_or(10);
    let fits = longest + AFTER_NAME <= room;
    let name_w = if fits { longest } else { 0 };

    let items: Vec<ListItem> = if sv.people.is_empty() {
        vec![ListItem::new(Span::styled(
            "nobody has committed here yet",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        sv.people
            .iter()
            .enumerate()
            .map(|(i, person)| {
                let is_sel = active && i == sv.people_idx;
                let name_style =
                    Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM });
                // The signing marker is about presence, not validity: a key
                // glyph would suggest this has been verified, and it has not.
                let sig = match (person.signed, person.commits) {
                    (0, _) => Span::styled("  —   ", Style::default().fg(theme::INK_FAINT)),
                    (s, c) if s == c => Span::styled("  all ", Style::default().fg(theme::OK)),
                    (s, c) => Span::styled(
                        format!("  {:>3}% ", s * 100 / c.max(1)),
                        Style::default().fg(theme::WARN),
                    ),
                };
                let numbers = vec![
                    Span::styled(
                        format!("{:>5}", person.commits),
                        Style::default().fg(theme::INK_DIM),
                    ),
                    sig,
                    Span::styled(
                        format!("{:>8}", person.last.map(age).unwrap_or_default()),
                        Style::default().fg(theme::INK_FAINT),
                    ),
                ];

                let lines = if fits {
                    let mut row = vec![
                        theme::caret(app, is_sel),
                        Span::styled(format!("{:<name_w$}  ", person.name), name_style),
                    ];
                    row.extend(numbers);
                    vec![Line::from(row)]
                } else {
                    let mut second = vec![Span::raw("    ")];
                    second.extend(numbers);
                    vec![
                        Line::from(vec![
                            theme::caret(app, is_sel),
                            Span::styled(person.name.clone(), name_style),
                        ]),
                        Line::from(second),
                    ]
                };

                ListItem::new(lines).style(if is_sel {
                    Style::default()
                        .bg(theme::selection(app))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                })
            })
            .collect()
    };

    let mut state = ListState::default();
    state.select(Some(sv.people_idx));
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
}

/// Everything the repository records about the selected person.
fn render_person(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("identity", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let Some(person) = app.stats_selected_person() else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  nobody selected",
                Style::default().fg(theme::INK_FAINT),
            )),
            body,
        );
        return;
    };

    // An address is 40 characters more often than not, and half an address is
    // not an address: the value wraps under its label.
    let room = (body.width as usize).saturating_sub(12);
    let field = move |label: &str, value: String, style: Style| -> Vec<Line<'static>> {
        let chunks = theme::wrap(&value, room.max(8));
        chunks
            .into_iter()
            .enumerate()
            .map(|(i, part)| {
                Line::from(vec![
                    Span::styled(
                        format!("{:<10}", if i == 0 { label } else { "" }),
                        Style::default().fg(theme::INK_FAINT),
                    ),
                    Span::styled(part, style),
                ])
            })
            .collect()
    };

    let mut lines = Vec::new();
    lines.extend(field(
        "name",
        person.name.clone(),
        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
    ));
    lines.extend(field(
        "email",
        person.email.clone(),
        Style::default().fg(theme::INK),
    ));
    // Every other address and spelling is shown: they are the same person to
    // git only by accident, and knowing about them is the point of the screen.
    for other in &person.other_emails {
        lines.extend(field(
            "",
            other.clone(),
            Style::default().fg(theme::INK_DIM),
        ));
    }
    for alias in &person.also_known_as {
        lines.extend(field(
            "also",
            alias.clone(),
            Style::default().fg(theme::INK_DIM),
        ));
    }

    lines.push(Line::from(""));
    lines.extend(field(
        "commits",
        person.commits.to_string(),
        Style::default().fg(theme::INK),
    ));
    lines.extend(field(
        "first",
        person.first.map(stamp).unwrap_or_default(),
        Style::default().fg(theme::INK_DIM),
    ));
    lines.extend(field(
        "last",
        person.last.map(stamp).unwrap_or_default(),
        Style::default().fg(theme::INK_DIM),
    ));
    if person.committed_by_other > 0 {
        lines.extend(field(
            "applied",
            format!("{} by someone else", person.committed_by_other),
            Style::default().fg(theme::INK_DIM),
        ));
    }

    lines.push(Line::from(""));
    let signed = person.signed;
    lines.extend(if signed == 0 {
        field(
            "signed",
            "none".to_string(),
            Style::default().fg(theme::INK_FAINT),
        )
    } else {
        field(
            "signed",
            format!("{signed} of {} commits", person.commits),
            Style::default().fg(if signed == person.commits {
                theme::OK
            } else {
                theme::WARN
            }),
        )
    });
    if !person.sig_kinds.is_empty() {
        lines.extend(field(
            "format",
            person
                .sig_kinds
                .iter()
                .map(|k| k.label())
                .collect::<Vec<_>>()
                .join(" · "),
            Style::default().fg(theme::INK_DIM),
        ));
    }
    // The honest caveat, on screen and not only in the code.
    lines.push(Line::from(Span::styled(
        "presence, not validity",
        Style::default().fg(theme::INK_FAINT),
    )));
    lines.push(Line::from(Span::styled(
        "verify one with S in the log",
        Style::default().fg(theme::INK_FAINT),
    )));

    f.render_widget(
        Paragraph::new(lines).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

/// A date, in the European order this project writes dates in.
fn stamp(when: i64) -> String {
    let days = when / 86_400;
    // Civil-from-days, the standard algorithm — no chrono needed for a date.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:02}-{:02}-{}", d, m, y)
}

fn sync_span(ahead: usize, behind: usize) -> Span<'static> {
    match (ahead > 0, behind > 0) {
        (true, true) => Span::styled(
            format!("↑{ahead} ↓{behind}"),
            Style::default().fg(theme::WARN),
        ),
        (true, false) => Span::styled(format!("↑{ahead}   "), Style::default().fg(theme::WARN)),
        (false, true) => Span::styled(format!("↓{behind}   "), Style::default().fg(theme::WARN)),
        (false, false) => Span::styled("✓   ", Style::default().fg(theme::OK)),
    }
}

/// A proportion, drawn in blocks.
fn bar(pct: usize, width: usize) -> String {
    let filled = (pct * width).div_ceil(100).min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// How long ago, in the same words the log uses.
fn age(when: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(when);
    let secs = (now - when).max(0);
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86400),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// Keep the end of a path: `crates/torii/src/tui/ui.rs` says more from the
/// right than from the left.
fn tail(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let skip = count - (max - 1);
    format!("…{}", s.chars().skip(skip).collect::<String>())
}
