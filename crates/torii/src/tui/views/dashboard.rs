//! The files view: what changed, and what happened lately.
//!
//! `t` swaps the status columns for the tree of HEAD — folders, files,
//! icons, the usual connectors. `t` or Esc brings the columns back.
//! Folders start folded. Enter, space, h/l, arrows or -/+ open and
//! fold; a click selects, a second click opens or folds.
//! (`e` is the event log, everywhere.) Keys are claimed in
//! `tui/bindings.rs` — a letter cannot be two things in this view.
//!
//! Four boxes became four regions parted by hairlines, which is the shape the
//! site draws and, more usefully, buys back the eight rows of border those
//! boxes were spending.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::tui::app::{App, Panel};
use crate::tui::theme::{self, Tick};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    if app.dashboard.browse {
        render_browse(f, app, area);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // the three file columns
            Constraint::Length(1),  // rule
            Constraint::Min(3),     // log
        ])
        .split(area);

    let spines = render_files(f, app, rows[0]);
    // The columns have to meet the chrome at both ends: the header rule above,
    // which was drawn before this view existed, and the rule below.
    theme::tie_above(f, rows[0], &spines);
    let ticks: Vec<(u16, Tick)> = spines.into_iter().map(|x| (x, Tick::Up)).collect();
    theme::hrule_content(f, rows[1], &ticks);
    render_log(f, app, rows[2]);
}

/// Draws the three columns and reports the x of each rule between them, so
/// the rule below can be tied into them.
fn render_files(f: &mut Frame, app: &App, area: Rect) -> Vec<u16> {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);

    let panels = [
        (
            Panel::Staged,
            &app.staged,
            app.dashboard.staged_idx,
            "staged",
        ),
        (
            Panel::Unstaged,
            &app.unstaged,
            app.dashboard.unstaged_idx,
            "unstaged",
        ),
        (
            Panel::Untracked,
            &app.untracked,
            app.dashboard.untracked_idx,
            "untracked",
        ),
    ];

    let count = panels.len();
    let mut spines = Vec::new();
    for (i, (panel, files, selected, title)) in panels.into_iter().enumerate() {
        // Every column but the last carries the rule to its right.
        let last = i == count - 1;
        let body = if last {
            cols[i]
        } else {
            let divider = theme::divider_right();
            let inner = divider.inner(cols[i]);
            f.render_widget(divider, cols[i]);
            spines.push(cols[i].right().saturating_sub(1));
            inner
        };
        render_file_list(f, app, body, panel, files, selected, title);
    }
    spines
}

fn render_browse(f: &mut Frame, app: &App, area: Rect) {
    let is_active = !app.sidebar_focused;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "tree",
        Some(app.dashboard.browse_paths.len()),
        is_active,
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), rows[0]);

    if let Some(err) = app.dashboard.browse_error.as_ref() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(err.clone(), Style::default().fg(theme::WARN)),
            ]))
            .block(Block::default().padding(Padding::new(1, 1, 0, 0))),
            rows[1],
        );
        return;
    }

    let width = rows[1].width.saturating_sub(4) as usize;
    let items: Vec<ListItem> = if app.dashboard.browse_rows.is_empty() {
        vec![ListItem::new(Span::styled(
            "empty tree — nothing in HEAD yet",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        app.dashboard
            .browse_rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let is_sel = is_active && i == app.dashboard.browse_idx;
                let collapsed = app.dashboard.browse_collapsed.contains(&row.path);
                ListItem::new(browse_line(app, row, is_sel, collapsed, width)).style(if is_sel {
                    Style::default().bg(theme::selection(app))
                } else {
                    Style::default()
                })
            })
            .collect()
    };

    let mut state = ListState::default();
    if is_active && !app.dashboard.browse_rows.is_empty() {
        state.select(Some(app.dashboard.browse_idx));
    }
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        rows[1],
        &mut state,
    );
    if !app.dashboard.browse_rows.is_empty() {
        let first = state.offset();
        app.hits.borrow_mut().rows(
            rows[1],
            "files.tree",
            first,
            app.dashboard.browse_rows.len().saturating_sub(first),
        );
    }
}

fn browse_line(
    app: &App,
    row: &crate::tui::app::BrowseRow,
    is_sel: bool,
    collapsed: bool,
    width: usize,
) -> Line<'static> {
    let icon = browse_icon(&row.name, row.is_dir, collapsed);
    let prefix = row.prefix();
    // caret (2) + connectors + icon (2 cells) + space + name
    let used = 2 + prefix.chars().count() + 3;
    let name_budget = width.saturating_sub(used);
    let mut label = truncate(&row.name, name_budget);
    if row.is_dir {
        label.push('/');
    }
    let name_style = if is_sel {
        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD)
    } else if row.is_dir {
        Style::default().fg(theme::INK)
    } else {
        Style::default().fg(theme::INK_DIM)
    };
    Line::from(vec![
        theme::caret(app, is_sel),
        Span::styled(prefix, Style::default().fg(theme::INK_FAINT)),
        Span::styled(format!("{icon} "), Style::default().fg(theme::INK_DIM)),
        Span::styled(label, name_style),
    ])
}

fn browse_icon(name: &str, is_dir: bool, collapsed: bool) -> &'static str {
    if is_dir {
        return if collapsed { "📁" } else { "📂" };
    }
    let ext = name
        .rsplit('.')
        .next()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "txt" | "rst" => "📝",
        "toml" | "json" | "yml" | "yaml" | "lock" | "ini" => "📋",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" => "🎨",
        "sh" | "bash" | "fish" | "zsh" => "📜",
        _ => "📄",
    }
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

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(title, Some(files.len()), is_active));
    f.render_widget(Paragraph::new(Line::from(heading)), rows[0]);

    // One column of air either side of the text, since there is no longer a
    // border holding it off the rule.
    let width = rows[1].width.saturating_sub(4) as usize;
    let items: Vec<ListItem> = files
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_sel = is_active && i == selected;
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::raw(shorten_path(&entry.path, width)),
            ]))
            .style(theme::row_style(app, is_sel))
        })
        .collect();

    let mut state = ListState::default();
    if is_active && !files.is_empty() {
        state.select(Some(selected));
    }
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        rows[1],
        &mut state,
    );
}

fn render_log(f: &mut Frame, app: &App, area: Rect) {
    let is_active = !app.sidebar_focused && app.dashboard.selected_panel == Panel::Log;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("log", None, is_active));
    f.render_widget(Paragraph::new(Line::from(heading)), rows[0]);

    // caret + hash + time are fixed; the message takes exactly what is left,
    // measured against the widest age on screen so the ages line up at the
    // right edge instead of against a guessed constant.
    let inner_width = rows[1].width.saturating_sub(4) as usize;
    let age_width = app
        .commits
        .iter()
        .map(|c| c.time.chars().count())
        .max()
        .unwrap_or(0);
    let msg_width = inner_width.saturating_sub(9 + age_width);

    let items: Vec<ListItem> = app
        .commits
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let is_sel = is_active && i == app.dashboard.log_idx;
            let msg = truncate(&c.message, msg_width);
            let msg_style = if is_sel {
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::INK_DIM)
            };
            let line = Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(format!("{} ", c.hash), Style::default().fg(theme::WARN)),
                Span::styled(format!("{:<width$}", msg, width = msg_width), msg_style),
                Span::styled(
                    format!(" {:>width$}", c.time, width = age_width),
                    Style::default().fg(theme::INK_FAINT),
                ),
            ]);
            let row = if is_sel {
                Style::default().bg(theme::selection(app))
            } else {
                Style::default()
            };
            ListItem::new(line).style(row)
        })
        .collect();

    let mut state = ListState::default();
    if is_active && !app.commits.is_empty() {
        state.select(Some(app.dashboard.log_idx));
    }
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        rows[1],
        &mut state,
    );
}

fn shorten_path(path: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if path.len() <= max {
        return path.to_string();
    }
    format!("…{}", &path[path.len().saturating_sub(max - 1)..])
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut)
}
