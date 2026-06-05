use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::tui::app::{App, Panel};
use super::super::ui::{C_WHITE, C_DIM, C_SUBTLE, C_YELLOW};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12),
            Constraint::Min(6),
        ])
        .split(area);

    render_files(f, app, chunks[0]);
    render_log(f, app, chunks[1]);
}

fn render_files(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);

    render_file_list(f, app, cols[0], Panel::Staged,    &app.staged,    app.dashboard.staged_idx,    "staged");
    render_file_list(f, app, cols[1], Panel::Unstaged,  &app.unstaged,  app.dashboard.unstaged_idx,  "unstaged");
    render_file_list(f, app, cols[2], Panel::Untracked, &app.untracked, app.dashboard.untracked_idx, "untracked");
}

fn render_file_list(
    f: &mut Frame,
    app: &App,
    area: Rect,
    panel: Panel,
    files: &[crate::tui::app::FileEntry],
    selected: usize,
    title: &str,
) {
    let is_active = !app.sidebar_focused && app.dashboard.selected_panel == panel;
    let bc = app.brand_color();
    let border_style = if is_active {
        Style::default().fg(C_WHITE)
    } else {
        Style::default().fg(bc)
    };
    let title_style = if is_active {
        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(bc)
    };

    let block = Block::default()
        .title(Span::styled(format!(" {} ({}) ", title, files.len()), title_style))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(border_style);

    let items: Vec<ListItem> = files.iter().enumerate().map(|(i, entry)| {
        let style = if is_active && i == selected {
            Style::default().bg(app.selected_bg()).fg(C_WHITE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(C_SUBTLE)
        };
        let prefix = if is_active && i == selected { "▶ " } else { "  " };
        ListItem::new(format!("{}{}", prefix, shorten_path(&entry.path, area.width as usize - 4)))
            .style(style)
    }).collect();

    let mut state = ListState::default();
    if is_active && !files.is_empty() {
        state.select(Some(selected));
    }
    f.render_stateful_widget(List::new(items).block(block), area, &mut state);
}

fn render_log(f: &mut Frame, app: &App, area: Rect) {
    let is_active = !app.sidebar_focused && app.dashboard.selected_panel == Panel::Log;
    let bc = app.brand_color();
    let border_style = if is_active {
        Style::default().fg(C_WHITE)
    } else {
        Style::default().fg(bc)
    };
    let title_style = if is_active {
        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(bc)
    };

    let block = Block::default()
        .title(Span::styled(" log ", title_style))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(border_style);

    let inner_width = area.width.saturating_sub(4) as usize;
    let msg_width = inner_width.saturating_sub(22);

    let items: Vec<ListItem> = app.commits.iter().enumerate().map(|(i, c)| {
        let is_sel = is_active && i == app.dashboard.log_idx;
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let prefix = if is_sel { "▶ " } else { "  " };
        let msg = truncate(&c.message, msg_width);
        let line = Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{} ", c.hash), Style::default().fg(C_YELLOW)),
            Span::styled(format!("{:<width$}", msg, width = msg_width), Style::default().fg(C_WHITE)),
            Span::styled(format!(" {}", c.time), Style::default().fg(C_DIM)),
        ]);
        ListItem::new(line).style(style)
    }).collect();

    let mut state = ListState::default();
    if is_active && !app.commits.is_empty() {
        state.select(Some(app.dashboard.log_idx));
    }
    f.render_stateful_widget(List::new(items).block(block), area, &mut state);
}

fn shorten_path(path: &str, max: usize) -> String {
    if path.len() <= max { return path.to_string(); }
    format!("…{}", &path[path.len().saturating_sub(max - 1)..])
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { return s.to_string(); }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut)
}
