use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use super::super::ui::{C_CYAN, C_DIM, C_GREEN, C_RED, C_SUBTLE, C_WHITE, C_YELLOW};
use crate::tui::app::{App, ConfigScope};

const SECTIONS: &[&str] = &["user", "auth", "git", "mirror", "snapshot", "ui"];
const SECTION_COLORS: &[ratatui::style::Color] =
    &[C_CYAN, C_YELLOW, C_GREEN, C_CYAN, C_YELLOW, C_GREEN];

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    // 0.7.5: the "status" box used to live below as a third row with
    // mode-aware hints. Those hints moved into the global hint bar
    // (render_hint in ui.rs) so they sit with every other view's bottom
    // legend. The view now uses the full area for sections + entries.
    // Transient status_msg (post-save confirmation) is surfaced via the
    // App-wide status line for the same reason.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(1)])
        .split(area);

    render_sections(f, app, cols[0]);
    render_entries(f, app, cols[1]);
}

fn render_sections(f: &mut Frame, app: &App, area: Rect) {
    let current_section = app
        .config_view
        .entries
        .get(app.config_view.idx)
        .map(|e| e.section.as_str())
        .unwrap_or("");

    let bc = app.brand_color();
    let focused = !app.sidebar_focused;
    let items: Vec<ListItem> = SECTIONS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_active = *s == current_section;
            let color = SECTION_COLORS.get(i).copied().unwrap_or(C_SUBTLE);
            let prefix = if is_active { "█ " } else { "  " };
            let style = if is_active {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(
                    *s,
                    Style::default().fg(if is_active { color } else { C_SUBTLE }),
                ),
            ]))
            .style(style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(
            " sections ",
            if focused {
                Style::default().fg(C_WHITE)
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
    f.render_widget(List::new(items).block(block), area);
}

fn render_entries(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;
    let scope_label = if app.config_view.scope == ConfigScope::Global {
        "global"
    } else {
        "local"
    };

    let items: Vec<ListItem> = if app.config_view.entries.is_empty() {
        vec![ListItem::new(Span::styled(
            "  no config entries",
            Style::default().fg(C_DIM),
        ))]
    } else {
        app.config_view
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let is_sel = i == app.config_view.idx;
                let is_editing = is_sel && app.config_view.editing;

                let style = if is_sel {
                    Style::default()
                        .bg(app.selected_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let prefix = if is_sel { "█ " } else { "  " };
                let key_color = if e.value.contains("[not set]") {
                    C_DIM
                } else {
                    C_WHITE
                };

                let value_span = if is_editing {
                    let buf = &app.config_view.edit_buf;
                    let char_cur = app.config_view.edit_cursor;
                    // convert char index to byte index safely
                    let byte_cur = buf
                        .char_indices()
                        .nth(char_cur)
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
                        Span::styled(prefix, Style::default().fg(bc)),
                        Span::styled(format!("{:<32}", &e.key), Style::default().fg(C_CYAN)),
                        Span::styled(before, Style::default().fg(C_WHITE)),
                        Span::styled(cursor_char.to_string(), Style::default().bg(bc).fg(C_WHITE)),
                        Span::styled(after, Style::default().fg(C_WHITE)),
                    ])
                } else {
                    let value_display = if e.value.contains("[set]") {
                        Span::styled("••••••", Style::default().fg(C_DIM))
                    } else if e.value.contains("[not set]") {
                        Span::styled("not set", Style::default().fg(C_RED))
                    } else {
                        Span::styled(&e.value, Style::default().fg(key_color))
                    };
                    Line::from(vec![
                        Span::styled(prefix, Style::default().fg(bc)),
                        Span::styled(format!("{:<32}", &e.key), Style::default().fg(C_SUBTLE)),
                        value_display,
                    ])
                };

                ListItem::new(value_span).style(style)
            })
            .collect()
    };

    let mut state = ListState::default();
    if !app.config_view.entries.is_empty() {
        state.select(Some(app.config_view.idx));
    }

    let title = format!(" config ({}) ", scope_label);
    let block = Block::default()
        .title(Span::styled(
            title,
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
    f.render_stateful_widget(List::new(items).block(block), area, &mut state);
}

// `render_status` was removed in 0.7.5; hints moved to the global hint
// bar in `ui.rs::render_hint` (where every other view's legend lives).
// Removed altogether rather than #[allow(dead_code)] because it would
// become a maintenance trap — drifting copy-paste of the hint strings
// that already exist elsewhere.
