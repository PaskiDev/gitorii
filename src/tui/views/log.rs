use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

use crate::tui::app::App;
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_CYAN, C_YELLOW, C_GREEN, C_RED};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(area);

    // ── Commit list ───────────────────────────────────────────────────────────
    let inner_width = chunks[0].width.saturating_sub(4) as usize;
    let msg_width = inner_width.saturating_sub(32);

    let display_indices: Vec<usize> = if app.log.filtered.is_empty() && app.log.search_query.is_empty() {
        (0..app.commits.len()).collect()
    } else {
        app.log.filtered.clone()
    };

    // Graph prefixes are always rendered in the Log view (toggleable via
    // Settings → graph style if the user wants to fall back to ascii).
    let graph_on = !app.log.graph_rows.is_empty();
    let graph_width = if graph_on {
        app.log
            .graph_rows
            .iter()
            .map(|r| r.commit_line.chars().count())
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let msg_width = msg_width.saturating_sub(graph_width + 1);

    let items: Vec<ListItem> = display_indices.iter().map(|&i| {
        let c = &app.commits[i];
        let is_sel = i == app.log.idx;
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let prefix = if is_sel { "█ " } else { "  " };
        let msg = truncate(&c.message, msg_width);
        let msg_color = if !app.log.search_query.is_empty() && !app.log.filtered.is_empty() {
            C_GREEN
        } else {
            C_WHITE
        };

        let mut spans = vec![
            Span::styled(prefix, Style::default().fg(bc)),
        ];
        if graph_on {
            let row = app.log.graph_rows.get(i);
            let glyphs = row.map(|r| r.commit_line.as_str()).unwrap_or("");
            let color = row
                .map(|r| ratatui::style::Color::Indexed(crate::graph::lane_color(r.lane)))
                .unwrap_or(C_CYAN);
            // Explicitly cancel BOLD here — the parent ListItem style applies
            // BOLD when selected, but many monospace fonts ship narrower
            // bullseye/half-circle glyphs in bold (or fall back to a thinner
            // proxy), making the selected commit glyph visibly shrink.
            spans.push(Span::styled(
                format!("{:<width$} ", glyphs, width = graph_width),
                Style::default().fg(color).remove_modifier(Modifier::BOLD),
            ));
        }
        if app.log.show_signatures {
            // Letter + colour mirror the CLI's `log --signatures`
            // column: G good (green), U unknown (yellow), B bad
            // (red), ? other error (subtle), N none (dim), - cached
            // miss (subtle).
            let letter = app.log.signature_cache
                .get(&c.full_hash).copied().unwrap_or('…');
            let color = match letter {
                'G' => C_GREEN,
                'U' => C_YELLOW,
                'B' => C_RED,
                '?' => C_SUBTLE,
                'N' => C_DIM,
                _   => C_SUBTLE,
            };
            spans.push(Span::styled(
                format!("{} ", letter),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }
        spans.push(Span::styled(format!("{} ", c.hash), Style::default().fg(C_YELLOW)));
        spans.push(Span::styled(format!("{:<width$}", msg, width = msg_width), Style::default().fg(if is_sel { C_WHITE } else { msg_color })));
        spans.push(Span::styled(format!(" {:>10}", truncate(&c.author, 10)), Style::default().fg(C_CYAN)));
        spans.push(Span::styled(format!(" {}", c.time), Style::default().fg(C_DIM)));

        ListItem::new(Line::from(spans)).style(style)
    }).collect();

    let sel_pos = display_indices.iter().position(|&i| i == app.log.idx);
    let mut state = ListState::default();
    if let Some(pos) = sel_pos { state.select(Some(pos)); }

    let total = app.commits.len();
    let loaded_hint = if app.log.all_loaded { String::new() } else { "  ↓ more".to_string() };
    let title = if app.log.search_mode {
        format!(" log — search: {}█ ", app.log.search_query)
    } else if !app.log.search_query.is_empty() {
        format!(" log — \"{}\"  {} matches ", app.log.search_query, display_indices.len())
    } else {
        format!(" log — {} ({} commits){} ", app.branch, total, loaded_hint)
    };

    let title_color = if app.log.search_mode { C_YELLOW } else if focused { C_WHITE } else { bc };

    let list_block = Block::default()
        .title(Span::styled(title, Style::default().fg(title_color).add_modifier(
            if focused || app.log.search_mode { Modifier::BOLD } else { Modifier::empty() }
        )))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(if focused { Style::default().fg(C_WHITE) } else { Style::default().fg(bc) });
    f.render_stateful_widget(List::new(items).block(list_block), chunks[0], &mut state);

    // ── Files panel ───────────────────────────────────────────────────────────
    let file_items: Vec<ListItem> = if app.log.commit_files.is_empty() {
        vec![ListItem::new(Span::styled("  no changes", Style::default().fg(C_DIM)))]
    } else {
        app.log.commit_files.iter().map(|f| {
            let (status_str, status_color) = match f.status {
                'A' => ("+ ", C_GREEN),
                'D' => ("- ", C_RED),
                'R' => ("→ ", C_CYAN),
                _   => ("~ ", C_YELLOW),
            };
            ListItem::new(Line::from(vec![
                Span::styled(status_str, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
                Span::styled(file_basename(&f.path), Style::default().fg(C_WHITE)),
                Span::styled(file_dir(&f.path), Style::default().fg(C_DIM)),
            ]))
        }).collect()
    };

    let commit_info = app.commits.get(app.log.idx).map(|c| {
        format!(" {} — {} files ", &c.hash, app.log.commit_files.len())
    }).unwrap_or_default();

    let files_block = Block::default()
        .title(Span::styled(commit_info, Style::default().fg(bc)))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(bc));
    f.render_widget(List::new(file_items).block(files_block), chunks[1]);

    // ── Ops dropdown overlay ──────────────────────────────────────────────────
    if app.log.ops_mode {
        const OPS: &[(&str, bool)] = &[
            ("diff",       false),
            ("copy hash",  false),
            ("search",     false),
        ];
        let dropdown_w = 16u16;
        let dropdown_h = OPS.len() as u16 + 2;
        let sel_display_pos = sel_pos.unwrap_or(0);
        let entry_y = chunks[0].y + 1 + sel_display_pos as u16 + 1;
        let drop_y = if entry_y + dropdown_h < chunks[0].y + chunks[0].height {
            entry_y
        } else {
            chunks[0].y + chunks[0].height - dropdown_h
        };
        let drop_area = Rect::new(chunks[0].x + 3, drop_y, dropdown_w, dropdown_h);

        let drop_items: Vec<ListItem> = OPS.iter().enumerate().map(|(i, (label, danger))| {
            let is_sel = i == app.log.ops_idx;
            let color = if *danger { C_RED } else if is_sel { C_WHITE } else { C_SUBTLE };
            let prefix = if is_sel { "▶ " } else { "  " };
            let style = if is_sel {
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
        drop_state.select(Some(app.log.ops_idx));

        let drop_block = Block::default()
            .borders(Borders::ALL).border_type(app.border_type())
            .border_style(Style::default().fg(bc));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(List::new(drop_items).block(drop_block), drop_area, &mut drop_state);
    }

    if app.log.signature_overlay.is_some() {
        render_signature_overlay(f, app, area);
    }
}

/// 0.7.36 — armor overlay. Centered modal showing the ASCII-armored
/// signature of the highlighted commit + the verify verdict. Any key
/// closes (handled in events.rs); the worker thread populates
/// `Done` / `Error` once gpg returns.
fn render_signature_overlay(f: &mut Frame, app: &App, area: Rect) {
    use crate::tui::app::{SignatureOverlay, SignatureVerdictColor};
    let Some(state) = app.log.signature_overlay.as_ref() else { return };

    let w: u16 = 78.min(area.width.saturating_sub(4));
    let h: u16 = 22.min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);

    let (title, body): (String, Vec<Line>) = match state {
        SignatureOverlay::Loading { oid } => (
            format!(" signature · {} · loading… ", &oid[..oid.len().min(8)]),
            vec![
                Line::from(Span::styled(
                    "  ▰▱▱▱▱▱▱▱▱▱  verifying signature…",
                    Style::default().fg(C_YELLOW),
                )),
            ],
        ),
        SignatureOverlay::Done { oid, armor, verdict, verdict_color } => {
            let vcolor = match verdict_color {
                SignatureVerdictColor::Good    => C_GREEN,
                SignatureVerdictColor::Unknown => C_YELLOW,
                SignatureVerdictColor::Bad     => C_RED,
                SignatureVerdictColor::Other   => C_SUBTLE,
            };
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("  verdict   ", Style::default().fg(C_SUBTLE)),
                Span::styled(verdict.clone(), Style::default().fg(vcolor).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(""));
            for line in armor.lines().take((h as usize).saturating_sub(6)) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(C_WHITE),
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  [any key] close",
                Style::default().fg(C_DIM),
            )));
            (format!(" signature · {} ", &oid[..oid.len().min(8)]), lines)
        }
        SignatureOverlay::Error { oid, message } => (
            format!(" signature · {} ", &oid[..oid.len().min(8)]),
            vec![
                Line::from(vec![
                    Span::styled("  ✗ ", Style::default().fg(C_RED)),
                    Span::styled(message.clone(), Style::default().fg(C_WHITE)),
                ]),
                Line::from(""),
                Line::from(Span::styled("  [any key] close", Style::default().fg(C_DIM))),
            ],
        ),
    };

    f.render_widget(
        ratatui::widgets::Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(title, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
    );
}

fn file_basename(path: &str) -> String {
    path.rfind('/').map(|i| path[i+1..].to_string()).unwrap_or_else(|| path.to_string())
}

fn file_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => format!("  {}/", &path[..i]),
        None    => String::new(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 { return String::new(); }
    if s.chars().count() <= max { return s.to_string(); }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut)
}
