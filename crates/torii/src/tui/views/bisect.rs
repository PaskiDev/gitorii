//! `bisect` TUI view — detect + drive an active `git bisect` session.
//!
//! Detects an active session by looking for `.git/BISECT_START` and the
//! `.git/BISECT_TERMS` / `BISECT_LOG` siblings that `git bisect` writes.
//! Operations (start, good, bad, skip, run, reset) are dispatched
//! through an ops dropdown — same chrome family as the Auth and
//! Platform views.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{
    App, BisectFocus, BisectState, RefEntry, RefKind, RefPickerOp, RefPickerTab,
};
use crate::tui::theme;

pub fn refresh(app: &mut App) {
    let prev_focus = app.bisect_view.focus.clone();
    let prev_input = std::mem::take(&mut app.bisect_view.input_buffer);
    let prev_prompt = std::mem::take(&mut app.bisect_view.input_prompt);
    let prev_op = app.bisect_view.pending_op.clone();
    let prev_idx = app.bisect_view.dropdown_idx;

    app.bisect_view = Default::default();
    app.bisect_view.focus = prev_focus;
    app.bisect_view.input_buffer = prev_input;
    app.bisect_view.input_prompt = prev_prompt;
    app.bisect_view.pending_op = prev_op;
    app.bisect_view.dropdown_idx = prev_idx;

    let repo = match git2::Repository::open(".") {
        Ok(r) => r,
        Err(e) => {
            app.bisect_view.status = Some(format!("open: {}", e));
            return;
        }
    };
    let gitdir = repo.path();
    let started = gitdir.join("BISECT_START");
    if !started.exists() {
        return;
    }
    app.bisect_view.in_progress = true;

    if let Ok(head) = repo.head() {
        if let Some(oid) = head.target() {
            app.bisect_view.current_hash = Some(oid.to_string()[..8].to_string());
        }
    }

    if let Ok(log) = std::fs::read_to_string(gitdir.join("BISECT_LOG")) {
        for line in log.lines() {
            if let Some(rest) = line.strip_prefix("# good: ") {
                app.bisect_view.good_refs.push(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("# bad: ") {
                app.bisect_view.bad_refs.push(rest.trim().to_string());
            }
        }
    }
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // The status pane carries the rule; the detail sits the other side of it.
    let divider = theme::divider_right();
    let status_pane = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = [panes[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    render_status(f, app, status_pane);
    render_detail(f, app, panes[1]);

    // Overlays — drawn last so they sit on top.
    match app.bisect_view.focus {
        BisectFocus::OpsDropdown => render_ops_dropdown(f, app, area),
        BisectFocus::InputArgs => render_input_overlay(f, app, area),
        BisectFocus::RefPicker => render_ref_picker(f, app, area),
        BisectFocus::ConfirmReset => render_confirm_reset(f, app, area),
        BisectFocus::List => {}
    }
}

/// Ref picker overlay. Two-tab dance (Bad → Good) when the op is
/// Start; single-tab for Mark / Skip. Filter is typed inline at the
/// bottom of the title and applied case-insensitively to the display
/// strings, so the user can narrow ~100 refs into a couple in a few
/// keystrokes.
fn render_ref_picker(f: &mut Frame, app: &App, area: Rect) {
    let picker = &app.bisect_view.picker;

    let w: u16 = 72.min(area.width.saturating_sub(4));
    let h: u16 = (area.height.saturating_sub(2)).min(22);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);

    // Build the filtered list.
    let filtered_idx = filter_indexes(&picker.all, &picker.filter);
    let mut items: Vec<ListItem> = Vec::new();
    let sel = picker.idx.min(filtered_idx.len().saturating_sub(1));
    for (visible, &orig_idx) in filtered_idx.iter().enumerate() {
        let e = &picker.all[orig_idx];
        let is_sel = visible == sel;
        let kind_label = match e.kind {
            RefKind::Head => "HEAD  ",
            RefKind::Branch => "branch",
            RefKind::Tag => "tag   ",
            RefKind::Remote => "remote",
            RefKind::Commit => "commit",
        };
        let kind_color = match e.kind {
            RefKind::Head => theme::OK,
            RefKind::Branch => theme::INK,
            RefKind::Tag => theme::WARN,
            RefKind::Remote => theme::INK_FAINT,
            RefKind::Commit => theme::INK_FAINT,
        };
        // For Start in Good tab, mark already-picked good refs with ✓.
        let marker = if matches!(picker.op, RefPickerOp::Start)
            && picker.tab == RefPickerTab::Good
            && picker.good_picks.iter().any(|g| g.target == e.target)
        {
            Span::styled("✓ ", Style::default().fg(theme::OK))
        } else {
            Span::raw("  ")
        };
        let style = if is_sel {
            Style::default()
                .bg(theme::selection(app))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        items.push(
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                marker,
                Span::styled(format!("{}  ", kind_label), Style::default().fg(kind_color)),
                Span::styled(e.display.clone(), Style::default().fg(theme::INK)),
            ]))
            .style(style),
        );
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  no match",
            Style::default().fg(theme::INK_FAINT),
        ))));
    }

    // Title shows the active op + (for Start) the current tab + the
    // current filter buffer.
    let mut title: Vec<Span> = Vec::new();
    title.push(Span::styled(
        " ref picker ",
        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
    ));
    title.push(Span::styled("· ", Style::default().fg(theme::INK_FAINT)));
    match picker.op {
        RefPickerOp::Start => {
            let (bad_style, good_style) = match picker.tab {
                RefPickerTab::Bad => (
                    Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
                    Style::default().fg(theme::INK_FAINT),
                ),
                RefPickerTab::Good => (
                    Style::default().fg(theme::INK_FAINT),
                    Style::default().fg(theme::OK).add_modifier(Modifier::BOLD),
                ),
            };
            title.push(Span::styled("Bad", bad_style));
            title.push(Span::styled(" · ", Style::default().fg(theme::INK_FAINT)));
            title.push(Span::styled("Good", good_style));
        }
        RefPickerOp::MarkGood => {
            title.push(Span::styled("mark good", Style::default().fg(theme::OK)))
        }
        RefPickerOp::MarkBad => title.push(Span::styled("mark bad", Style::default().fg(theme::BAD))),
        RefPickerOp::Skip => title.push(Span::styled("skip", Style::default().fg(theme::WARN))),
    }
    if !picker.filter.is_empty() {
        title.push(Span::styled("  /", Style::default().fg(theme::INK_FAINT)));
        title.push(Span::styled(
            picker.filter.clone(),
            Style::default().fg(theme::INK),
        ));
    }
    title.push(Span::raw(" "));

    let mut state = ListState::default();
    state.select(if filtered_idx.is_empty() {
        None
    } else {
        Some(sel)
    });

    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Line::from(title))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::RULE)),
        ),
        popup,
        &mut state,
    );
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let active = !app.sidebar_focused;
    let pv = &app.bisect_view;

    let mut lines: Vec<Line> = Vec::new();
    if pv.in_progress {
        lines.push(Line::from(Span::styled(
            "  ● bisecting",
            Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        if let Some(h) = &pv.current_hash {
            lines.push(Line::from(vec![
                Span::styled("  testing   ", Style::default().fg(theme::INK_FAINT)),
                Span::styled(
                    h.clone(),
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("  good      ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                format!("{}", pv.good_refs.len()),
                Style::default().fg(theme::OK),
            ),
            Span::styled(" ref(s)", Style::default().fg(theme::INK_FAINT)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  bad       ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                format!("{}", pv.bad_refs.len()),
                Style::default().fg(theme::BAD),
            ),
            Span::styled(" ref(s)", Style::default().fg(theme::INK_FAINT)),
        ]));
        lines.push(Line::from(""));
        for r in pv.good_refs.iter().take(8) {
            lines.push(Line::from(vec![
                Span::styled("    ✓ ", Style::default().fg(theme::OK)),
                Span::styled(short(r), Style::default().fg(theme::INK_DIM)),
            ]));
        }
        for r in pv.bad_refs.iter().take(8) {
            lines.push(Line::from(vec![
                Span::styled("    ✗ ", Style::default().fg(theme::BAD)),
                Span::styled(short(r), Style::default().fg(theme::INK_DIM)),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  no bisect in progress",
            Style::default().fg(theme::INK_FAINT),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("o", Style::default().fg(theme::accent(app))),
            Span::styled("  Start begins one", Style::default().fg(theme::INK_FAINT)),
        ]));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("bisect", None, active));
    if pv.in_progress {
        heading.push(Span::styled(
            "  in progress",
            Style::default().fg(theme::WARN),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);
    f.render_widget(Paragraph::new(lines), body);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body_area] = theme::heading_and_body(area);
    let pv = &app.bisect_view;

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        if pv.in_progress { "ops" } else { "detail" },
        None,
        false,
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let mut body: Vec<Line> = Vec::new();
    if pv.in_progress {
        for (label, desc) in ops_for(pv) {
            body.push(Line::from(vec![
                Span::styled(
                    format!("  {:<16}", label),
                    Style::default().fg(theme::INK_DIM),
                ),
                Span::styled(desc, Style::default().fg(theme::INK_FAINT)),
            ]));
        }
    } else {
        for line in [
            "  Mark a known-bad and one or more known-good",
            "  commits. torii (via libgit2) bisects between",
            "  them, checks out a candidate, and asks you to",
            "  mark it good / bad / skip.",
        ] {
            body.push(Line::from(Span::styled(
                line,
                Style::default().fg(theme::INK),
            )));
        }
        body.push(Line::from(""));
        body.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("o", Style::default().fg(theme::accent(app))),
            Span::styled("  → Start", Style::default().fg(theme::INK_FAINT)),
        ]));
    }

    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }),
        body_area,
    );
}

/// Contextual ops for the current bisect state. List shrinks to the
/// only sensible action (Start) when no session is active.
pub fn ops_for(state: &BisectState) -> Vec<(&'static str, &'static str)> {
    if !state.in_progress {
        return vec![("Start", "begin a new bisect session")];
    }
    vec![
        ("Mark HEAD good", "current commit is known-good"),
        ("Mark HEAD bad", "current commit is known-bad"),
        ("Mark good <ref>…", "pick a known-good ref/commit"),
        ("Mark bad <ref>…", "pick a known-bad ref/commit"),
        ("Skip HEAD", "untestable; pick another candidate"),
        ("Skip <ref>…", "skip a specific commit"),
        (
            "Run command",
            "auto-bisect via exit code (0=good, ≠0=bad, 125=skip)",
        ),
        ("Reset", "finish bisect + restore original HEAD ⚠"),
    ]
}

/// Load the ref picker's source list from the current repo. The order
/// matches what a human usually wants on top: HEAD first, then local
/// branches, then tags (newest first), then remotes, then the recent
/// log. Filtering is applied at render time against `display`.
pub fn load_refs() -> Vec<RefEntry> {
    let mut out: Vec<RefEntry> = Vec::new();
    let Ok(repo) = git2::Repository::open(".") else {
        return out;
    };

    // HEAD as a synthetic entry — most bisect starts include it.
    if let Ok(head) = repo.head() {
        if let Some(oid) = head.target() {
            let short = &oid.to_string()[..8];
            let label = head.shorthand().unwrap_or("HEAD").to_string();
            out.push(RefEntry {
                display: format!("HEAD ({}, {})", label, short),
                target: "HEAD".to_string(),
                kind: RefKind::Head,
                subject: None,
            });
        }
    }

    // Local branches.
    if let Ok(iter) = repo.branches(Some(git2::BranchType::Local)) {
        for b in iter.flatten() {
            let (br, _) = b;
            if let Some(name) = br.name().ok().flatten() {
                if name == "HEAD" {
                    continue;
                }
                out.push(RefEntry {
                    display: name.to_string(),
                    target: name.to_string(),
                    kind: RefKind::Branch,
                    subject: None,
                });
            }
        }
    }

    // Tags (newest first). Tag listing doesn't preserve order so we
    // resolve each tag to its commit time and sort.
    let mut tags: Vec<(String, i64)> = Vec::new();
    let _ = repo.tag_foreach(|oid, name| {
        let n = String::from_utf8_lossy(name).to_string();
        let n = n.strip_prefix("refs/tags/").unwrap_or(&n).to_string();
        let t = repo
            .find_commit(oid)
            .map(|c| c.time().seconds())
            .unwrap_or(0);
        tags.push((n, t));
        true
    });
    tags.sort_by_key(|t| std::cmp::Reverse(t.1));
    for (n, _) in tags.into_iter().take(50) {
        out.push(RefEntry {
            display: n.clone(),
            target: n,
            kind: RefKind::Tag,
            subject: None,
        });
    }

    // Remote branches.
    if let Ok(iter) = repo.branches(Some(git2::BranchType::Remote)) {
        for b in iter.flatten() {
            let (br, _) = b;
            if let Some(name) = br.name().ok().flatten() {
                if name.ends_with("/HEAD") {
                    continue;
                }
                out.push(RefEntry {
                    display: name.to_string(),
                    target: name.to_string(),
                    kind: RefKind::Remote,
                    subject: None,
                });
            }
        }
    }

    // Recent commits — last 30 reachable from HEAD.
    if let Ok(mut walk) = repo.revwalk() {
        let _ = walk.push_head();
        for oid in walk.flatten().take(30) {
            if let Ok(c) = repo.find_commit(oid) {
                let short = &oid.to_string()[..8];
                let subject = c.summary().unwrap_or("").to_string();
                out.push(RefEntry {
                    display: format!("{}  {}", short, truncate_subject(&subject, 60)),
                    target: oid.to_string(),
                    kind: RefKind::Commit,
                    subject: Some(subject),
                });
            }
        }
    }

    out
}

fn truncate_subject(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n.saturating_sub(1)).collect();
    format!("{}…", cut)
}

/// Filter the picker entries by case-insensitive substring on the
/// display text. Returns indexes into `all` so callers can keep the
/// slice ordering when rendering.
pub fn filter_indexes(all: &[RefEntry], filter: &str) -> Vec<usize> {
    if filter.is_empty() {
        return (0..all.len()).collect();
    }
    let needle = filter.to_lowercase();
    all.iter()
        .enumerate()
        .filter_map(|(i, r)| {
            if r.display.to_lowercase().contains(&needle) {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

fn render_ops_dropdown(f: &mut Frame, app: &App, area: Rect) {
    let ops = ops_for(&app.bisect_view);
    if ops.is_empty() {
        return;
    }

    let w: u16 = 50;
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
            let is_sel = i == app.bisect_view.dropdown_idx;
            let label_color = if label.starts_with("Reset") {
                theme::BAD
            } else if is_sel {
                theme::INK
            } else {
                theme::INK_DIM
            };
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(format!("{:<18}", label), Style::default().fg(label_color)),
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
    state.select(Some(app.bisect_view.dropdown_idx));
    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    " ops — Enter run · Esc close ",
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
    let w: u16 = 70.min(area.width.saturating_sub(4));
    let h: u16 = 5;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let body = vec![
        Line::from(Span::styled(
            format!(" {}", app.bisect_view.input_prompt),
            Style::default().fg(theme::INK),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                &app.bisect_view.input_buffer,
                Style::default().fg(theme::INK),
            ),
            Span::styled("█", Style::default().fg(theme::accent(app))),
        ]),
    ];
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " input · Enter run · Esc cancel ",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::RULE)),
        ),
        popup,
    );
}

fn render_confirm_reset(f: &mut Frame, app: &App, area: Rect) {
    let w: u16 = 56;
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
            "  Reset bisect and restore original HEAD?",
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

fn short(s: &str) -> String {
    s.chars().take(40).collect()
}
