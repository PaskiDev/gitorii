//! `bisect` TUI view — read-only status of a bisect in progress.
//!
//! Detects an active session by looking for `.git/BISECT_START` and the
//! `.git/BISECT_TERMS` / `BISECT_LOG` siblings that `git bisect` writes.
//! Doesn't drive the state machine yet (use `torii bisect` from the
//! shell for now); 0.7.3 will plumb the start/good/bad/skip/reset keys.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::app::App;
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_YELLOW, C_CYAN, C_GREEN, C_RED};

pub fn refresh(app: &mut App) {
    app.bisect_view = Default::default();

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
        return; // no session
    }
    app.bisect_view.in_progress = true;

    // Current HEAD as the bisect's pivot.
    if let Ok(head) = repo.head() {
        if let Some(oid) = head.target() {
            app.bisect_view.current_hash = Some(format!("{}", &oid.to_string()[..8]));
        }
    }

    // BISECT_NAMES holds `good` and `bad` refs separated by newlines.
    if let Ok(names) = std::fs::read_to_string(gitdir.join("BISECT_NAMES")) {
        for line in names.lines() {
            let l = line.trim();
            if l.is_empty() { continue; }
            // BISECT_NAMES doesn't disambiguate; we also peek at the log.
            app.bisect_view.good_refs.push(l.to_string());
        }
    }
    if let Ok(log) = std::fs::read_to_string(gitdir.join("BISECT_LOG")) {
        // The log is "# bad: <oid>" / "# good: <oid>" / "git bisect …" lines.
        app.bisect_view.good_refs.clear();
        for line in log.lines() {
            if let Some(rest) = line.strip_prefix("# good: ") {
                app.bisect_view.good_refs.push(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("# bad: ") {
                app.bisect_view.bad_refs.push(rest.trim().to_string());
            }
        }
        // Crude steps estimate: each iteration roughly halves the
        // remaining range; libgit2 doesn't expose this directly so we
        // leave it as None for now.
    }
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(10)])
        .split(area);

    // ── Status panel ─────────────────────────────────────────────────────
    let mut lines: Vec<Line> = Vec::new();
    if app.bisect_view.in_progress {
        lines.push(Line::from(vec![Span::styled(
            "  ● Bisect in progress",
            Style::default()
                .fg(C_YELLOW)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(vec![]));
        if let Some(h) = &app.bisect_view.current_hash {
            lines.push(Line::from(vec![
                Span::styled("  Testing:  ", Style::default().fg(C_SUBTLE)),
                Span::styled(h, Style::default().fg(C_CYAN).add_modifier(Modifier::BOLD)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("  Good:     ", Style::default().fg(C_SUBTLE)),
            Span::styled(
                format!("{} ref(s)", app.bisect_view.good_refs.len()),
                Style::default().fg(C_GREEN),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Bad:      ", Style::default().fg(C_SUBTLE)),
            Span::styled(
                format!("{} ref(s)", app.bisect_view.bad_refs.len()),
                Style::default().fg(C_RED),
            ),
        ]));
        lines.push(Line::from(vec![]));
        for r in app.bisect_view.good_refs.iter().take(5) {
            lines.push(Line::from(vec![
                Span::styled("    ✓ ", Style::default().fg(C_GREEN)),
                Span::styled(&r[..r.len().min(40)], Style::default().fg(C_DIM)),
            ]));
        }
        for r in app.bisect_view.bad_refs.iter().take(5) {
            lines.push(Line::from(vec![
                Span::styled("    ✗ ", Style::default().fg(C_RED)),
                Span::styled(&r[..r.len().min(40)], Style::default().fg(C_DIM)),
            ]));
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            "  No bisect in progress.",
            Style::default().fg(C_DIM),
        )]));
        lines.push(Line::from(vec![]));
        lines.push(Line::from(vec![Span::styled(
            "  Start one with:  torii bisect start <bad> <good>…",
            Style::default().fg(C_SUBTLE),
        )]));
    }

    let title_style = if focused {
        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(bc)
    };
    let block = Block::default()
        .title(Span::styled(" bisect ", title_style))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(if focused {
            Style::default().fg(C_WHITE)
        } else {
            Style::default().fg(bc)
        });
    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    // ── Commands panel ───────────────────────────────────────────────────
    let cmd_lines = vec![
        Line::from(vec![Span::styled(
            "  Commands (run in shell for now):",
            Style::default()
                .fg(C_SUBTLE)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![]),
        Line::from(vec![Span::styled(
            "    torii bisect bad             Mark HEAD as bad",
            Style::default().fg(C_DIM),
        )]),
        Line::from(vec![Span::styled(
            "    torii bisect good            Mark HEAD as good",
            Style::default().fg(C_DIM),
        )]),
        Line::from(vec![Span::styled(
            "    torii bisect skip            Skip current commit",
            Style::default().fg(C_DIM),
        )]),
        Line::from(vec![Span::styled(
            "    torii bisect run <cmd>       Automate via exit code",
            Style::default().fg(C_DIM),
        )]),
        Line::from(vec![Span::styled(
            "    torii bisect reset           Finish + restore HEAD",
            Style::default().fg(C_DIM),
        )]),
    ];
    let cblock = Block::default()
        .title(Span::styled(" commands ", Style::default().fg(bc)))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(Paragraph::new(cmd_lines).block(cblock), chunks[1]);
}
