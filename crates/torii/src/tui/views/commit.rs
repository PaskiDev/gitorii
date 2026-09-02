//! The commit view: what is staged, what kind of change it is, and the message.
//!
//! Three sections parted by rules rather than three stacked boxes. Where a
//! border used to change colour to say which section had the focus, the
//! heading now carries it.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Padding, Paragraph},
    Frame,
};

use crate::tui::app::{App, CommitFocus};
use crate::tui::events::COMMIT_TYPES;
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;
    let in_list = focused && app.commit_view.focus == CommitFocus::List;
    let in_selector = focused && app.commit_view.focus == CommitFocus::TypeSelector;
    let in_input = focused && app.commit_view.focus == CommitFocus::Input;

    // staged | rule | type | rule | message. Each section is a heading row and
    // a body, which is what a boxed title was giving for free.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(2),
            Constraint::Length(1),
            Constraint::Length(COMMIT_TYPES.len() as u16 + 1),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(area);

    render_staged(f, app, rows[0], in_list);
    theme::hrule_content(f, rows[1], &[]);
    render_types(f, app, rows[2], in_selector);
    theme::hrule_content(f, rows[3], &[]);
    render_message(f, app, rows[4], in_input);
}

fn render_staged(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("staged", Some(app.staged.len()), active));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let items: Vec<ListItem> = if app.staged.is_empty() {
        vec![ListItem::new(Span::styled(
            "no staged files — press space on the files view to stage one",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        app.staged
            .iter()
            .map(|e| {
                ListItem::new(Line::from(vec![
                    Span::styled("+ ", Style::default().fg(theme::OK)),
                    Span::styled(&e.path, Style::default().fg(theme::INK_DIM)),
                ]))
            })
            .collect()
    };

    f.render_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

fn render_types(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("type", None, active));
    heading.push(Span::raw("  "));
    heading.extend(theme::key_hint(app, "i", "skip"));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let items: Vec<ListItem> = COMMIT_TYPES
        .iter()
        .enumerate()
        .map(|(i, (prefix, desc))| {
            let is_sel = active && i == app.commit_view.type_idx;
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(
                    format!("{:<10}", prefix),
                    Style::default()
                        .fg(if is_sel { theme::INK } else { theme::INK_DIM })
                        .add_modifier(if is_sel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(*desc, Style::default().fg(theme::INK_FAINT)),
            ]))
            .style(if is_sel {
                Style::default().bg(theme::selection(app))
            } else {
                Style::default()
            })
        })
        .collect();

    f.render_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

fn render_message(f: &mut Frame, app: &App, area: Rect, active: bool) {
    let [heading_row, body] = theme::heading_and_body(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("message", None, active));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let msg = &app.commit_view.message;
    let cursor = app.commit_view.cursor;
    let (before, after) = msg.split_at(cursor.min(msg.len()));
    let cursor_char = after.chars().next().unwrap_or(' ');
    let after_cursor = if after.is_empty() {
        ""
    } else {
        &after[cursor_char.len_utf8()..]
    };

    let line = if active {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(before, Style::default().fg(theme::INK)),
            Span::styled(
                cursor_char.to_string(),
                Style::default()
                    .bg(theme::selection(app))
                    .fg(theme::INK)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(after_cursor, Style::default().fg(theme::INK)),
        ])
    } else {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(msg.as_str(), Style::default().fg(theme::INK_DIM)),
        ])
    };

    f.render_widget(Paragraph::new(line), body);
}
