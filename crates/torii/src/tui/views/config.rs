//! The config view: the sections down the left, the entries of the current
//! one beside them.
//!
//! Two columns parted by a rule rather than two boxes. The per-section rainbow
//! is gone: a section is either the one you are in or it is not, and that is
//! what ink weight says.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::tui::app::{App, ConfigScope};
use crate::tui::theme;

const SECTIONS: &[&str] = &["user", "auth", "git", "mirror", "snapshot", "ui"];

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    // 0.7.5: the "status" box used to live below as a third row with
    // mode-aware hints. Those hints moved into the global hint bar
    // (render_hint in ui.rs) so they sit with every other view's bottom
    // legend. The view now uses the full area for sections + entries.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(1)])
        .split(area);

    // The sections column carries the rule; the entries sit the other side.
    let divider = theme::divider_right();
    let sections_pane = divider.inner(cols[0]);
    f.render_widget(divider, cols[0]);
    let spine = [cols[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    render_sections(f, app, sections_pane);
    render_entries(f, app, cols[1]);
}

fn render_sections(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let focused = !app.sidebar_focused;

    let current = app
        .config_view
        .entries
        .get(app.config_view.idx)
        .map(|e| e.section.as_str())
        .unwrap_or("");

    let items: Vec<ListItem> = SECTIONS
        .iter()
        .map(|s| {
            let is_active = *s == current;
            ListItem::new(Line::from(vec![
                theme::caret(app, is_active && focused),
                Span::styled(
                    *s,
                    Style::default().fg(if is_active {
                        theme::INK
                    } else {
                        theme::INK_DIM
                    }),
                ),
            ]))
            .style(if is_active {
                Style::default()
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        })
        .collect();

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("sections", None, focused));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);
    f.render_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

fn render_entries(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let focused = !app.sidebar_focused;

    let items: Vec<ListItem> = if app.config_view.entries.is_empty() {
        vec![ListItem::new(Span::styled(
            "no config entries",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        app.config_view
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let is_sel = focused && i == app.config_view.idx;
                let is_editing = is_sel && app.config_view.editing;

                let line = if is_editing {
                    let buf = &app.config_view.edit_buf;
                    // convert char index to byte index safely
                    let byte_cur = buf
                        .char_indices()
                        .nth(app.config_view.edit_cursor)
                        .map(|(b, _)| b)
                        .unwrap_or(buf.len());
                    let before = &buf[..byte_cur];
                    let cursor_char = buf[byte_cur..].chars().next().unwrap_or(' ');
                    let after_start = byte_cur
                        + if buf[byte_cur..].is_empty() {
                            0
                        } else {
                            cursor_char.len_utf8()
                        };
                    let after = &buf[after_start..];
                    Line::from(vec![
                        theme::caret(app, true),
                        Span::styled(
                            format!("{:<32}", &e.key),
                            Style::default().fg(theme::INK_DIM),
                        ),
                        Span::styled(before, Style::default().fg(theme::INK)),
                        Span::styled(
                            cursor_char.to_string(),
                            Style::default()
                                .bg(theme::accent(app))
                                .fg(theme::INK)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(after, Style::default().fg(theme::INK)),
                    ])
                } else {
                    // A secret is never printed, and something unset is stated
                    // rather than left blank.
                    let value = if e.value.contains("[set]") {
                        Span::styled("••••••", Style::default().fg(theme::INK_DIM))
                    } else if e.value.contains("[not set]") {
                        Span::styled("not set", Style::default().fg(theme::INK_FAINT))
                    } else {
                        Span::styled(&e.value, Style::default().fg(theme::INK))
                    };
                    Line::from(vec![
                        theme::caret(app, is_sel),
                        Span::styled(
                            format!("{:<32}", &e.key),
                            Style::default().fg(theme::INK_DIM),
                        ),
                        value,
                    ])
                };

                ListItem::new(line).style(if is_sel {
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
    if !app.config_view.entries.is_empty() {
        state.select(Some(app.config_view.idx));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("config", None, focused));
    heading.push(Span::styled(
        if app.config_view.scope == ConfigScope::Global {
            "  global"
        } else {
            "  local"
        },
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
}

// `render_status` was removed in 0.7.5; hints moved to the global hint
// bar in `ui.rs::render_hint` (where every other view's legend lives).
// Removed altogether rather than #[allow(dead_code)] because it would
// become a maintenance trap — drifting copy-paste of the hint strings
// that already exist elsewhere.
