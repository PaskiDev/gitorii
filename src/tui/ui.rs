use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph},
};

use super::app::{App, EventKind, View};
use super::views;

pub const BRAND_COLOR: Color = Color::Rgb(255, 76, 76);
#[allow(dead_code)]
pub const SELECTED_BG: Color = Color::Rgb(40, 40, 60);

// Paleta viva
pub const C_WHITE: Color    = Color::Rgb(220, 220, 220);
pub const C_SUBTLE: Color   = Color::Rgb(140, 140, 160);
pub const C_DIM: Color      = Color::Rgb(80, 80, 100);
pub const C_CYAN: Color     = Color::Rgb(80, 220, 200);
pub const C_YELLOW: Color   = Color::Rgb(255, 210, 80);
pub const C_GREEN: Color    = Color::Rgb(100, 220, 100);
pub const C_RED: Color      = Color::Rgb(255, 100, 100);
#[allow(dead_code)]
pub const C_BORDER: Color   = Color::Rgb(60, 60, 80);

const SIDEBAR_WIDTH: u16 = 20;

#[allow(dead_code)]
struct Tab {
    key: &'static str,
    label: &'static str,
    view: View,
}

// Sidebar reorganised in 0.7.26 by user flow:
//   entry → local action → navigation → broadcast → multi-platform → admin.
// Keep this aligned with `view_for_idx` + the sidebar_idx maps in
// `go_to` / `go_back` (src/tui/app.rs).
const TABS: &[Tab] = &[
    // entry
    Tab { key: "f", label: "files",     view: View::Dashboard  },
    // local action
    Tab { key: "c", label: "save",      view: View::Commit     },
    Tab { key: "s", label: "sync",      view: View::Sync       },
    Tab { key: "p", label: "snapshot",  view: View::Snapshot   },
    // navigation / history
    Tab { key: "l", label: "log",       view: View::Log        },
    Tab { key: "b", label: "branch",    view: View::Branch     },
    Tab { key: "t", label: "tags",      view: View::Tag        },
    // broadcast / platform-side
    Tab { key: "n", label: "pr/mr",     view: View::Pr         },
    Tab { key: "i", label: "issues",    view: View::Issue      },
    Tab { key: "y", label: "platform",  view: View::Platform   },
    // multi-platform / repo layout
    Tab { key: "r", label: "remote",    view: View::Remote     },
    Tab { key: "w", label: "workspace", view: View::Workspace  },
    Tab { key: "k", label: "worktrees", view: View::Worktree   },
    Tab { key: "m", label: "submodules",view: View::Submodule  },
    // admin / advanced
    Tab { key: "v", label: "bisect",    view: View::Bisect     },
    Tab { key: "a", label: "auth",      view: View::Auth       },
    Tab { key: "g", label: "config",    view: View::Config     },
];

pub fn render(f: &mut Frame, app: &App) {
    if app.view == View::Diff || app.view == View::Help {
        match app.view {
            View::Diff => views::diff::render(f, app),
            View::Help => views::help::render(f, app),
            _          => {}
        }
        return;
    }

    let area = f.area();

    // Global layout: header (3 lines) | body
    let global_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);

    render_header(f, app, global_rows[0]);

    let body = global_rows[1];

    // Body: sidebar | content
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(SIDEBAR_WIDTH),
            Constraint::Min(1),
        ])
        .split(body);

    // Content area: main view + 1 line hint at bottom
    let content_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(cols[1]);

    render_sidebar(f, app, cols[0]);

    match app.view {
        View::Dashboard => views::dashboard::render(f, app, content_rows[0]),
        View::Commit    => views::commit::render(f, app, content_rows[0]),
        View::Sync      => views::sync::render(f, app, content_rows[0]),
        View::Snapshot  => views::snapshot::render(f, app, content_rows[0]),
        // Log absorbs History since 0.7.2: same listing, history ops are
        // exposed as actions within Log itself.
        View::Log | View::History => views::log::render(f, app, content_rows[0]),
        View::Branch    => views::branch::render(f, app, content_rows[0]),
        View::Tag       => views::tag::render(f, app, content_rows[0]),
        // Remote absorbs Mirror in 0.7.2 — same view, mirrors are a tab.
        View::Remote | View::Mirror => views::remote::render(f, app, content_rows[0]),
        View::Workspace => views::workspace::render(f, app, content_rows[0]),
        View::Pr        => views::pr::render(f, app, content_rows[0]),
        View::Issue     => views::issue::render(f, app, content_rows[0]),
        View::Worktree  => views::worktree::render(f, app, content_rows[0]),
        View::Submodule => views::submodule::render(f, app, content_rows[0]),
        View::Bisect    => views::bisect::render(f, app, content_rows[0]),
        View::Auth      => views::auth::render(f, app, content_rows[0]),
        // 0.7.12: unified Platform view.
        View::Platform  => views::platform::render(f, app, content_rows[0]),
        // Config absorbs Settings via tabs since 0.7.2.
        View::Config | View::Settings => views::config::render(f, app, content_rows[0]),
        View::Diff | View::Help => {}
    }

    render_hint(f, app, content_rows[1]);


    if app.show_event_log {
        render_event_log(f, app, area);
    }

    if app.repo_picker_open {
        render_repo_picker(f, app, global_rows[0]);
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();

    let (status_label, status_color) = if app.ahead > 0 && app.behind > 0 {
        (format!("↑{} ↓{}", app.ahead, app.behind), C_YELLOW)
    } else if app.ahead > 0 {
        (format!("↑{} ahead", app.ahead), C_CYAN)
    } else if app.behind > 0 {
        (format!("↓{} behind", app.behind), C_RED)
    } else {
        ("synced".to_string(), C_GREEN)
    };

    let repo_name: String = std::fs::canonicalize(&app.repo_path)
        .ok()
        .as_deref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or(&app.repo_path)
        .to_string();

    // Inner width = area.width - 2 borders
    let inner_w = area.width.saturating_sub(2) as usize;
    let left_spans: Vec<Span> = vec![
        Span::styled("⛩  gitorii", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
        Span::styled("  /  ", Style::default().fg(C_DIM)),
        Span::styled(repo_name, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
    ];
    let mut right_spans: Vec<Span> = Vec::new();
    if let Some(new_v) = &app.update_available {
        right_spans.push(Span::styled(
            format!("⬆ v{} available  ", new_v),
            Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
        ));
    }
    right_spans.extend(vec![
        Span::styled("branch: ", Style::default().fg(C_SUBTLE)),
        Span::styled(&app.branch, Style::default().fg(C_GREEN).add_modifier(Modifier::BOLD)),
        Span::styled("  status: ", Style::default().fg(C_SUBTLE)),
        Span::styled(status_label, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
    ]);
    let left_len: usize = left_spans.iter().map(|s| s.content.chars().count()).sum::<usize>() + 1;
    let right_len: usize = right_spans.iter().map(|s| s.content.chars().count()).sum::<usize>() + 1;
    let pad = inner_w.saturating_sub(left_len + right_len);

    let mut spans = vec![Span::raw(" ")];
    spans.extend(left_spans);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.extend(right_spans);
    spans.push(Span::raw(" "));

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .block(Block::default()
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(bc))),
        area,
    );
}

fn render_repo_picker(f: &mut Frame, app: &App, header_area: Rect) {
    let bc = app.brand_color();
    let paths = app.workspace_repo_paths();
    if paths.is_empty() { return; }

    let dropdown_w: u16 = paths.iter()
        .map(|p| std::path::Path::new(p).file_name()
            .map(|n| n.to_string_lossy().len()).unwrap_or(p.len()) + 4)
        .max().unwrap_or(20)
        .min(40) as u16;
    let dropdown_h = paths.len() as u16 + 2;

    // Position: just below "⛩  gitorii  /  " prefix (~18 chars + 1 border + 1 space)
    let x = header_area.x + 18;
    let y = header_area.y + header_area.height; // just below header
    let drop_area = Rect::new(x, y, dropdown_w, dropdown_h.min(header_area.height + 10));

    let current = std::fs::canonicalize(&app.repo_path).ok();
    let items: Vec<ListItem> = paths.iter().enumerate().map(|(i, p)| {
        let name = std::path::Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| p.clone());
        let is_current = std::fs::canonicalize(p).ok() == current;
        let is_sel = i == app.repo_picker_idx;
        let color = if is_sel { C_WHITE } else if is_current { C_GREEN } else { C_SUBTLE };
        let prefix = if is_sel { "▶ " } else if is_current { "✓ " } else { "  " };
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(bc)),
            Span::styled(name, Style::default().fg(color)),
        ])).style(style)
    }).collect();

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.repo_picker_idx));

    let block = Block::default()
        .title(Span::styled(" switch repo ", Style::default().fg(bc).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL).border_type(app.border_type())
        .border_style(Style::default().fg(bc));

    f.render_widget(Clear, drop_area);
    f.render_stateful_widget(List::new(items).block(block), drop_area, &mut state);
}

fn render_event_log(f: &mut Frame, app: &App, area: Rect) {
    let panel_w = (area.width / 3).max(28).min(55);
    let panel_h = (area.height / 2).max(6).min(24);
    let x = (area.x + area.width).saturating_sub(panel_w + 1);
    let y = (area.y + area.height).saturating_sub(panel_h + 1);
    let panel_area = Rect::new(x, y, panel_w, panel_h);

    let bc = app.brand_color();
    let hint = Line::from(vec![
        Span::styled(" [e]", Style::default().fg(bc)),
        Span::styled(" close  ", Style::default().fg(C_SUBTLE)),
        Span::styled("[c]", Style::default().fg(bc)),
        Span::styled(" clear ", Style::default().fg(C_SUBTLE)),
    ]);
    let block = Block::default()
        .title(Span::styled(
            format!(" events ({}) ", app.event_log.len()),
            Style::default().fg(bc).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(hint)
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(bc));

    let inner = block.inner(panel_area);
    f.render_widget(Clear, panel_area);
    f.render_widget(block, panel_area);

    let items: Vec<ListItem> = app.event_log.iter().map(|e| {
        let kind_color = match e.kind {
            EventKind::Error   => C_RED,
            EventKind::Success => C_GREEN,
            EventKind::Info    => C_CYAN,
        };
        let kind_sym = match e.kind {
            EventKind::Error   => "✗",
            EventKind::Success => "✓",
            EventKind::Info    => "·",
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", e.timestamp), Style::default().fg(C_DIM)),
            Span::styled(kind_sym, Style::default().fg(kind_color)),
            Span::raw(" "),
            Span::styled(&e.message, Style::default().fg(C_WHITE)),
        ]))
    }).collect();

    f.render_widget(List::new(items), inner);
}

fn render_hint(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let bc = app.brand_color();
    if app.sidebar_focused {
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
            Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[Enter]", Style::default().fg(bc)),
            Span::styled(" open  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
        ]);
        let events_label = if app.show_event_log { " events ✓" } else { " events" };
        let has_siblings = app.workspace_has_siblings();
        let right_str = if has_siblings {
            format!("[W] repos  [e]{} ", events_label)
        } else {
            format!("[e]{} ", events_label)
        };
        let left_len: usize = line.spans.iter().map(|s| s.content.len()).sum();
        let pad = (area.width as usize).saturating_sub(left_len + right_str.len());
        let mut spans = line.spans;
        spans.push(Span::raw(" ".repeat(pad)));
        if has_siblings {
            spans.push(Span::styled("[W]", Style::default().fg(bc)));
            spans.push(Span::styled(" repos  ", Style::default().fg(C_SUBTLE)));
        }
        spans.push(Span::styled("[e]", Style::default().fg(bc)));
        spans.push(Span::styled(events_label, Style::default().fg(C_SUBTLE)));
        spans.push(Span::raw(" "));
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }
    let line = match app.view {
        View::Dashboard => {
            use crate::tui::app::Panel;
            match app.dashboard.selected_panel {
                Panel::Staged => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[space]", Style::default().fg(bc)),
                    Span::styled(" unstage  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[d]", Style::default().fg(bc)),
                    Span::styled(" diff", Style::default().fg(C_SUBTLE)),
                ]),
                Panel::Unstaged => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[space]", Style::default().fg(bc)),
                    Span::styled(" stage  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[d]", Style::default().fg(bc)),
                    Span::styled(" diff", Style::default().fg(C_SUBTLE)),
                ]),
                Panel::Untracked => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[space]", Style::default().fg(bc)),
                    Span::styled(" stage", Style::default().fg(C_SUBTLE)),
                ]),
                Panel::Log => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[d]", Style::default().fg(bc)),
                    Span::styled(" diff  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[l]", Style::default().fg(bc)),
                    Span::styled(" expand", Style::default().fg(C_SUBTLE)),
                ]),
            }
        }
        View::Commit => {
            let amend_style = if app.commit_view.amend {
                Style::default().fg(bc).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(C_DIM)
            };
            Line::from(vec![
                Span::raw(" "),
                Span::styled("[Enter]", Style::default().fg(bc)),
                Span::styled(" save  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[←→]", Style::default().fg(bc)),
                Span::styled(" cursor  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[a]", Style::default().fg(bc)),
                Span::styled(" amend ", Style::default().fg(C_SUBTLE)),
                Span::styled(if app.commit_view.amend { "[amend ✓]" } else { "" }, amend_style),
                Span::styled("  [Esc]", Style::default().fg(bc)),
                Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
            ])
        },
        View::Sync => Line::from(vec![
            Span::raw(" "),
            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
            Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[Enter]", Style::default().fg(bc)),
            Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
        ]),
        View::Log => if app.log.search_mode {
            Line::from(vec![
                Span::raw(" "),
                Span::styled("[Enter]", Style::default().fg(bc)),
                Span::styled(" confirm  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[Esc]", Style::default().fg(bc)),
                Span::styled(" cancel search", Style::default().fg(C_SUBTLE)),
            ])
        } else if app.log.ops_mode {
            Line::from(vec![
                Span::raw(" "),
                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[Enter]", Style::default().fg(bc)),
                Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[Esc]", Style::default().fg(bc)),
                Span::styled(" close", Style::default().fg(C_SUBTLE)),
            ])
        } else {
            Line::from(vec![
                Span::raw(" "),
                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[o]", Style::default().fg(bc)),
                Span::styled(" operations  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[/]", Style::default().fg(bc)),
                Span::styled(" search", Style::default().fg(C_SUBTLE)),
            ])
        },
        View::Branch => {
            use crate::tui::app::BranchConfirm;
            match &app.branch_view.confirm {
                BranchConfirm::Delete => {
                    let name = app.branch_view.branches.get(app.branch_view.idx)
                        .map(|b| b.name.as_str()).unwrap_or("?");
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("delete ", Style::default().fg(C_SUBTLE)),
                        Span::styled(name.to_string(), Style::default().fg(C_RED).add_modifier(Modifier::BOLD)),
                        Span::styled("?  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[y]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                        Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                        Span::styled("[any]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                        Span::styled(" cancel", Style::default().fg(C_DIM)),
                    ])
                }
                BranchConfirm::NewBranch => {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("new branch: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(app.branch_view.new_name.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(bc)),
                    ])
                }
                BranchConfirm::None => {
                    if app.branch_view.search_mode {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("search: ", Style::default().fg(C_SUBTLE)),
                            Span::styled(app.branch_view.search_query.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                            Span::styled("█  ", Style::default().fg(bc)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" confirm  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                        ])
                    } else if app.branch_view.ops_mode {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" close", Style::default().fg(C_SUBTLE)),
                        ])
                    } else if let Some(s) = &app.branch_view.status {
                        let color = if s.starts_with("checkout:") || s.starts_with("created") || s.starts_with("pushed") || s.starts_with("deleted") {
                            C_GREEN
                        } else if s.contains("failed") || s.contains("cannot") {
                            C_RED
                        } else {
                            C_YELLOW
                        };
                        Line::from(vec![Span::raw(" "), Span::styled(s.clone(), Style::default().fg(color))])
                    } else {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[o]", Style::default().fg(bc)),
                            Span::styled(" operations  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[/]", Style::default().fg(bc)),
                            Span::styled(" search", Style::default().fg(C_SUBTLE)),
                        ])
                    }
                }
            }
        },
        View::Snapshot => {
            use crate::tui::app::SnapshotFocus;
            if app.snapshot_view.search_mode {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" confirm  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" cancel search", Style::default().fg(C_SUBTLE)),
                ])
            } else if app.snapshot_view.ops_mode && app.snapshot_view.focus == SnapshotFocus::List {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" close", Style::default().fg(C_SUBTLE)),
                ])
            } else if app.snapshot_view.focus == SnapshotFocus::Create {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("snapshot name: ", Style::default().fg(C_SUBTLE)),
                    Span::styled(app.snapshot_view.create_name.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(bc)),
                ])
            } else if app.snapshot_view.focus == SnapshotFocus::AutoConfig {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" set  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" back", Style::default().fg(C_SUBTLE)),
                ])
            } else {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[o]", Style::default().fg(bc)),
                    Span::styled(" operations  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[/]", Style::default().fg(bc)),
                    Span::styled(" search  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[a]", Style::default().fg(bc)),
                    Span::styled(" auto-config", Style::default().fg(C_SUBTLE)),
                ])
            }
        },
        View::Tag => {
            use crate::tui::app::TagConfirm;
            match &app.tag_view.confirm {
                TagConfirm::Delete => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("delete tag?  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[y]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                    Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                    Span::styled("[any]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                    Span::styled(" cancel", Style::default().fg(C_DIM)),
                ]),
                TagConfirm::CreateName => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("tag name: ", Style::default().fg(C_SUBTLE)),
                    Span::styled(app.tag_view.new_name.as_str(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(bc)),
                ]),
                TagConfirm::CreateMessage => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("message: ", Style::default().fg(C_SUBTLE)),
                    Span::styled(app.tag_view.new_message.as_str(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(bc)),
                ]),
                TagConfirm::None => {
                    if app.tag_view.search_mode {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("search: ", Style::default().fg(C_SUBTLE)),
                            Span::styled(app.tag_view.search_query.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                            Span::styled("█  ", Style::default().fg(bc)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" confirm  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                        ])
                    } else if app.tag_view.ops_mode {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" close", Style::default().fg(C_SUBTLE)),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[o]", Style::default().fg(bc)),
                            Span::styled(" operations  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[/]", Style::default().fg(bc)),
                            Span::styled(" search", Style::default().fg(C_SUBTLE)),
                        ])
                    }
                }
            }
        },
        View::History => {
            use crate::tui::app::HistoryConfirm;
            match &app.history_view.confirm {
                HistoryConfirm::CherryPick => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("cherry-pick commit?  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[y]", Style::default().fg(bc)),
                    Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                    Span::styled("[any]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(C_DIM)),
                ]),
                HistoryConfirm::Clean => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("clean history & GC?  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[y]", Style::default().fg(bc)),
                    Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                    Span::styled("[any]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(C_DIM)),
                ]),
                HistoryConfirm::Rebase => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("rebase onto: ", Style::default().fg(C_SUBTLE)),
                    Span::styled(app.history_view.input.as_str(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(bc)),
                ]),
                HistoryConfirm::RemoveFile => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("remove file from history: ", Style::default().fg(C_RED)),
                    Span::styled(app.history_view.input.as_str(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(bc)),
                ]),
                HistoryConfirm::RewriteStart => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("rewrite start date (YYYY-MM-DD HH:MM): ", Style::default().fg(C_SUBTLE)),
                    Span::styled(app.history_view.input.as_str(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(bc)),
                ]),
                HistoryConfirm::RewriteEnd => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("rewrite end date (YYYY-MM-DD HH:MM): ", Style::default().fg(C_SUBTLE)),
                    Span::styled(app.history_view.input2.as_str(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(bc)),
                ]),
                HistoryConfirm::Blame => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("blame file: ", Style::default().fg(C_SUBTLE)),
                    Span::styled(app.history_view.input.as_str(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(bc)),
                ]),
                HistoryConfirm::Scan => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[f]", Style::default().fg(bc)),
                    Span::styled(" toggle mode  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" run scan  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                ]),
                HistoryConfirm::None => {
                    if app.history_view.ops_mode {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" close", Style::default().fg(C_SUBTLE)),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[o]", Style::default().fg(bc)),
                            Span::styled(" operations", Style::default().fg(C_SUBTLE)),
                        ])
                    }
                }
            }
        },
        View::Remote => {
            use crate::tui::app::RemoteConfirm;
            if app.remote_view.ops_mode {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" close", Style::default().fg(C_SUBTLE)),
                ])
            } else {
                match &app.remote_view.confirm {
                    RemoteConfirm::AddName => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("remote name: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(app.remote_view.new_name.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    RemoteConfirm::AddUrl => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("remote url: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(app.remote_view.new_url.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    RemoteConfirm::Rename => {
                        let old = app.remote_view.remotes.get(app.remote_view.idx)
                            .map(|r| r.name.as_str()).unwrap_or("?");
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("rename ", Style::default().fg(C_SUBTLE)),
                            Span::styled(old.to_string(), Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)),
                            Span::styled(" → ", Style::default().fg(C_SUBTLE)),
                            Span::styled(app.remote_view.new_name.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                            Span::styled("█", Style::default().fg(bc)),
                        ])
                    }
                    RemoteConfirm::EditUrl => {
                        let name = app.remote_view.remotes.get(app.remote_view.idx)
                            .map(|r| r.name.as_str()).unwrap_or("?");
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("edit url for ", Style::default().fg(C_SUBTLE)),
                            Span::styled(name.to_string(), Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)),
                            Span::styled(": ", Style::default().fg(C_SUBTLE)),
                            Span::styled(app.remote_view.new_url.clone(), Style::default().fg(C_WHITE)),
                            Span::styled("█", Style::default().fg(bc)),
                        ])
                    }
                    RemoteConfirm::MirrorRename => {
                        let old = app.remote_view.selected_mirror()
                            .map(|m| m.name.as_str()).unwrap_or("?");
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("rename mirror ", Style::default().fg(C_SUBTLE)),
                            Span::styled(old.to_string(), Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)),
                            Span::styled(" → ", Style::default().fg(C_SUBTLE)),
                            Span::styled(app.remote_view.new_name.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                            Span::styled("█", Style::default().fg(bc)),
                        ])
                    }
                    RemoteConfirm::MirrorAddPlatform => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("mirror platform (github/gitlab/…): ", Style::default().fg(C_SUBTLE)),
                        Span::styled(app.remote_view.new_mirror_platform.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    RemoteConfirm::MirrorAddAccount => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("account: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(app.remote_view.new_mirror_account.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    RemoteConfirm::MirrorAddRepo => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("repo name: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(app.remote_view.new_mirror_repo.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    RemoteConfirm::MirrorAddType => {
                        let (replica_style, primary_style) = if app.remote_view.new_mirror_type == 0 {
                            (Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                             Style::default().fg(C_SUBTLE))
                        } else {
                            (Style::default().fg(C_SUBTLE),
                             Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD))
                        };
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("type: ", Style::default().fg(C_SUBTLE)),
                            Span::styled("replica", replica_style),
                            Span::styled(" / ", Style::default().fg(C_DIM)),
                            Span::styled("primary", primary_style),
                            Span::styled("  [←→]", Style::default().fg(bc)),
                            Span::styled(" toggle  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" confirm", Style::default().fg(C_SUBTLE)),
                        ])
                    }
                    RemoteConfirm::Remove => {
                        let name = app.remote_view.remotes.get(app.remote_view.idx)
                            .map(|r| r.name.as_str()).unwrap_or("?");
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("remove remote ", Style::default().fg(C_SUBTLE)),
                            Span::styled(name.to_string(), Style::default().fg(C_RED).add_modifier(Modifier::BOLD)),
                            Span::styled("?  ", Style::default().fg(C_SUBTLE)),
                            Span::styled("[y]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                            Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                            Span::styled("[any]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                            Span::styled(" cancel", Style::default().fg(C_DIM)),
                        ])
                    }
                    RemoteConfirm::None => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" operations", Style::default().fg(C_SUBTLE)),
                    ]),
                }
            }
        },
        View::Mirror => Line::from(vec![]),
        View::Issue => {
            use crate::tui::app::IssueConfirm;
            if app.issue_view.ops_mode {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" close", Style::default().fg(C_SUBTLE)),
                ])
            } else {
                match &app.issue_view.confirm {
                    IssueConfirm::CreateTitle => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("title  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" next  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    IssueConfirm::CreateDesc => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("description  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" create  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    IssueConfirm::Comment => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("comment  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" send  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    IssueConfirm::Close => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" confirm close  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[any]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    IssueConfirm::None => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" operations  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[^r]", Style::default().fg(bc)),
                        Span::styled(" refresh", Style::default().fg(C_SUBTLE)),
                    ]),
                }
            }
        },
        View::Pr => {
            use crate::tui::app::PrConfirm;
            if app.pr_view.ops_mode {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" close", Style::default().fg(C_SUBTLE)),
                ])
            } else {
                match &app.pr_view.confirm {
                    PrConfirm::Merge => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[←→]", Style::default().fg(bc)),
                        Span::styled(" method  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" merge  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::Close => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" confirm close  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[any]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::CreateTitle => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" next  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::CreateHead => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select source branch  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" next  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::CreateBase => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select base  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" next  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::CreateDesc => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" new line  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[^S]", Style::default().fg(bc)),
                        Span::styled(" next  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Tab]", Style::default().fg(bc)),
                        Span::styled(" draft  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::CreatePlatforms => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[Space]", Style::default().fg(bc)),
                        Span::styled(" toggle  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[a]", Style::default().fg(bc)),
                        Span::styled(" all  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" create  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::SwitchPlatform => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" switch  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::EditTitle => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" next  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::EditDesc => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" new line  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[^S]", Style::default().fg(bc)),
                        Span::styled(" next  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::EditBase => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select base  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" confirm  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    PrConfirm::None => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" operations  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Tab]", Style::default().fg(bc)),
                        Span::styled(" filter  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[^r]", Style::default().fg(bc)),
                        Span::styled(" refresh", Style::default().fg(C_SUBTLE)),
                    ]),
                }
            }
        },
        View::Workspace => {
            use crate::tui::app::{WorkspaceConfirm, WorkspaceFocus};
            if app.workspace_view.ops_mode {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" close", Style::default().fg(C_SUBTLE)),
                ])
            } else {
                match &app.workspace_view.confirm {
                    WorkspaceConfirm::DeleteWorkspace => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("delete workspace?  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[y]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                        Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                        Span::styled("[any]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                        Span::styled(" cancel", Style::default().fg(C_DIM)),
                    ]),
                    WorkspaceConfirm::RemoveRepo => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("remove repo from workspace?  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[y]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                        Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                        Span::styled("[any]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                        Span::styled(" cancel", Style::default().fg(C_DIM)),
                    ]),
                    WorkspaceConfirm::SaveMessage => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("commit message: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(app.workspace_view.input.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    WorkspaceConfirm::AddRepoPath => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("repo path: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(app.workspace_view.input.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    WorkspaceConfirm::RenameWorkspace => {
                        let old = app.workspace_view.workspaces.get(app.workspace_view.ws_idx)
                            .map(|ws| ws.name.as_str()).unwrap_or("?");
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("rename ", Style::default().fg(C_SUBTLE)),
                            Span::styled(old.to_string(), Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)),
                            Span::styled(" → ", Style::default().fg(C_SUBTLE)),
                            Span::styled(app.workspace_view.input.clone(), Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)),
                            Span::styled("█", Style::default().fg(bc)),
                        ])
                    }
                    WorkspaceConfirm::None => {
                        if app.workspace_view.focus == WorkspaceFocus::Workspaces {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                                Span::styled("[→/l]", Style::default().fg(bc)),
                                Span::styled(" repos  ", Style::default().fg(C_SUBTLE)),
                                Span::styled("[o]", Style::default().fg(bc)),
                                Span::styled(" operations", Style::default().fg(C_SUBTLE)),
                            ])
                        } else {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                                Span::styled("[Enter]", Style::default().fg(bc)),
                                Span::styled(" open  ", Style::default().fg(C_SUBTLE)),
                                Span::styled("[o]", Style::default().fg(bc)),
                                Span::styled(" operations  ", Style::default().fg(C_SUBTLE)),
                                Span::styled("[←/h]", Style::default().fg(bc)),
                                Span::styled(" workspaces", Style::default().fg(C_SUBTLE)),
                            ])
                        }
                    }
                }
            }
        },
        View::Config => {
            // Hint adapts to mode (0.7.5): editing → Enter saves, Esc
            // cancels; otherwise Enter opens edit + Tab toggles scope.
            // The dedicated "status" box inside the view itself was
            // removed in 0.7.5; the line below replaces it.
            if app.config_view.editing {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" save  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                ])
            } else {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" edit  ", Style::default().fg(C_SUBTLE)),
                    Span::styled("[Tab]", Style::default().fg(bc)),
                    Span::styled(" toggle scope", Style::default().fg(C_SUBTLE)),
                ])
            }
        },
        View::Settings => Line::from(vec![
            Span::raw(" "),
            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
            Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[Enter]", Style::default().fg(bc)),
            Span::styled(" toggle/edit  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[s]", Style::default().fg(bc)),
            Span::styled(" save", Style::default().fg(C_SUBTLE)),
        ]),
        _ => Line::from(""),
    };

    // Push [e] events (and [W] repo picker if available) to the right edge
    let events_label = if app.show_event_log { " events ✓" } else { " events" };
    let has_siblings = app.workspace_has_siblings();
    let right_str = if has_siblings {
        format!("[W] repos  [e]{} ", events_label)
    } else {
        format!("[e]{} ", events_label)
    };
    let left_len: usize = line.spans.iter().map(|s| s.content.len()).sum();
    let pad = (area.width as usize).saturating_sub(left_len + right_str.len());

    let mut spans = line.spans;
    spans.push(Span::raw(" ".repeat(pad)));
    if has_siblings {
        spans.push(Span::styled("[W]", Style::default().fg(bc)));
        spans.push(Span::styled(" repos  ", Style::default().fg(C_SUBTLE)));
    }
    spans.push(Span::styled("[e]", Style::default().fg(bc)));
    spans.push(Span::styled(events_label, Style::default().fg(C_SUBTLE)));
    spans.push(Span::raw(" "));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_sidebar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let border_color = if app.sidebar_focused { C_WHITE } else { app.brand_color() };

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(border_color));
    let inner_area = outer.inner(area);
    f.render_widget(outer, area);


    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),     // tabs
            Constraint::Length(2),  // help + quit
        ])
        .split(inner_area);

    // Tabs
    let tab_items: Vec<ListItem> = TABS.iter().enumerate().map(|(i, tab)| {
        let is_current_view = app.view == tab.view;
        let is_sidebar_sel  = app.sidebar_focused && i == app.sidebar_idx;
        let sel_bg = app.selected_bg();
        let brand  = app.brand_color();

        let (prefix, label_style, bg) = if is_current_view {
            ("█ ", Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD), Some(sel_bg))
        } else if is_sidebar_sel {
            ("▶ ", Style::default().fg(C_WHITE), Some(sel_bg))
        } else {
            ("  ", Style::default().fg(C_SUBTLE), None)
        };

        let label: &str = if tab.view == View::Pr { "pr/mr" } else { tab.label };

        let mut item = ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(brand)),
            Span::styled(label.to_string(), label_style),
        ]));
        if let Some(color) = bg {
            item = item.style(Style::default().bg(color));
        }
        item
    }).collect();

    f.render_widget(
        List::new(tab_items)
            .block(Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(border_color))
                .padding(Padding::new(1, 1, 0, 0))),
        rows[0],
    );

    // Help + quit — aligned with hint row at bottom
    let bottom = List::new(vec![
        ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("help  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[?]", Style::default().fg(BRAND_COLOR)),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("quit  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[q]", Style::default().fg(BRAND_COLOR)),
        ])),
    ]);
    f.render_widget(
        bottom.block(Block::default().borders(Borders::NONE)),
        rows[1],
    );
}
