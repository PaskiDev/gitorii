use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph},
    Frame,
};

use super::app::{App, EventKind, View};
use super::theme;
use super::views;

#[allow(dead_code)]
pub const SELECTED_BG: Color = Color::Rgb(40, 40, 60);

// Paleta viva
pub const C_WHITE: Color = Color::Rgb(220, 220, 220);
pub const C_SUBTLE: Color = Color::Rgb(140, 140, 160);
pub const C_DIM: Color = Color::Rgb(80, 80, 100);
pub const C_CYAN: Color = Color::Rgb(80, 220, 200);
pub const C_YELLOW: Color = Color::Rgb(255, 210, 80);
pub const C_GREEN: Color = Color::Rgb(100, 220, 100);
pub const C_RED: Color = Color::Rgb(255, 100, 100);
#[allow(dead_code)]
pub const C_BORDER: Color = Color::Rgb(60, 60, 80);

const SIDEBAR_WIDTH: u16 = 18;

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
    Tab {
        key: "f",
        label: "files",
        view: View::Dashboard,
    },
    // local action
    Tab {
        key: "c",
        label: "save",
        view: View::Commit,
    },
    Tab {
        key: "s",
        label: "sync",
        view: View::Sync,
    },
    Tab {
        key: "p",
        label: "snapshot",
        view: View::Snapshot,
    },
    // navigation / history
    Tab {
        key: "l",
        label: "log",
        view: View::Log,
    },
    Tab {
        key: "b",
        label: "branch",
        view: View::Branch,
    },
    Tab {
        key: "t",
        label: "tags",
        view: View::Tag,
    },
    // broadcast / platform-side
    Tab {
        key: "n",
        label: "pr/mr",
        view: View::Pr,
    },
    Tab {
        key: "i",
        label: "issues",
        view: View::Issue,
    },
    Tab {
        key: "y",
        label: "platform",
        view: View::Platform,
    },
    // multi-platform / repo layout
    Tab {
        key: "r",
        label: "remote",
        view: View::Remote,
    },
    Tab {
        key: "w",
        label: "workspace",
        view: View::Workspace,
    },
    Tab {
        key: "k",
        label: "worktrees",
        view: View::Worktree,
    },
    Tab {
        key: "m",
        label: "submodules",
        view: View::Submodule,
    },
    // admin / advanced
    Tab {
        key: "v",
        label: "bisect",
        view: View::Bisect,
    },
    Tab {
        key: "a",
        label: "auth",
        view: View::Auth,
    },
    Tab {
        key: "g",
        label: "config",
        view: View::Config,
    },
];

pub fn render(f: &mut Frame, app: &App) {
    if app.view == View::Diff || app.view == View::Help {
        match app.view {
            View::Diff => views::diff::render(f, app),
            View::Help => views::help::render(f, app),
            _ => {}
        }
        return;
    }

    let area = f.area();

    // One window, the way the site draws it: a single border, and hairline
    // rules inside it wherever there used to be another box.
    let window = theme::frame(app);
    let inner = window.inner(area);
    f.render_widget(window, area);

    // chrome bar | rule | body | rule | key line
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    // Body: sidebar | content, parted by the sidebar's own right-hand rule.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(1)])
        .split(rows[2]);
    let spine = cols[0].right().saturating_sub(1);

    // The view gets a column of air either side of the rule rather than a
    // border to sit against.
    let content = Rect {
        x: cols[1].x + 1,
        y: cols[1].y,
        width: cols[1].width.saturating_sub(2),
        height: cols[1].height,
    };

    render_header(f, app, rows[0]);
    // Both body rules are drawn before the view, so the view can tie its own
    // columns into either of them; the sidebar's spine crosses both.
    theme::hrule(f, rows[1], &[(spine, theme::Tick::Down)]);
    theme::hrule(f, rows[3], &[(spine, theme::Tick::Up)]);
    render_sidebar(f, app, cols[0]);

    let content_rows = [content];

    match app.view {
        View::Dashboard => views::dashboard::render(f, app, content_rows[0]),
        View::Commit => views::commit::render(f, app, content_rows[0]),
        View::Sync => views::sync::render(f, app, content_rows[0]),
        View::Snapshot => views::snapshot::render(f, app, content_rows[0]),
        // Log absorbs History since 0.7.2: same listing, history ops are
        // exposed as actions within Log itself.
        View::Log | View::History => views::log::render(f, app, content_rows[0]),
        View::Branch => views::branch::render(f, app, content_rows[0]),
        View::Tag => views::tag::render(f, app, content_rows[0]),
        // Remote absorbs Mirror in 0.7.2 — same view, mirrors are a tab.
        View::Remote | View::Mirror => views::remote::render(f, app, content_rows[0]),
        View::Workspace => views::workspace::render(f, app, content_rows[0]),
        View::Pr => views::pr::render(f, app, content_rows[0]),
        View::Issue => views::issue::render(f, app, content_rows[0]),
        View::Worktree => views::worktree::render(f, app, content_rows[0]),
        View::Submodule => views::submodule::render(f, app, content_rows[0]),
        View::Bisect => views::bisect::render(f, app, content_rows[0]),
        View::Auth => views::auth::render(f, app, content_rows[0]),
        // 0.7.12: unified Platform view.
        View::Platform => views::platform::render(f, app, content_rows[0]),
        // Config absorbs Settings via tabs since 0.7.2.
        View::Config | View::Settings => views::config::render(f, app, content_rows[0]),
        View::Diff | View::Help => {}
    }

    render_hint(f, app, rows[4]);

    if app.show_event_log {
        render_event_log(f, app, area);
    }

    if app.repo_picker_open {
        render_repo_picker(f, app, rows[0]);
    }
}

/// The name the sidebar gives the current view, for the chrome bar. Views
/// that were folded into another one report the survivor's name.
fn view_label(view: &View) -> &'static str {
    let target = match view {
        View::History => View::Log,
        View::Mirror => View::Remote,
        View::Settings => View::Config,
        other => other.clone(),
    };
    TABS.iter()
        .find(|t| t.view == target)
        .map(|t| t.label)
        .unwrap_or("")
}

/// The chrome bar: identity on the left, the current view next to it, and one
/// badge on the right. One line, because the site's is one line and because
/// three lines of border spent on a repository name is two lines of log.
fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let accent = theme::accent(app);

    let (status_label, status_color) = if app.ahead > 0 && app.behind > 0 {
        (format!("↑{} ↓{}", app.ahead, app.behind), theme::WARN)
    } else if app.ahead > 0 {
        (format!("↑{}", app.ahead), theme::WARN)
    } else if app.behind > 0 {
        (format!("↓{}", app.behind), theme::BAD)
    } else {
        ("synced".to_string(), theme::OK)
    };

    let repo_name: String = std::fs::canonicalize(&app.repo_path)
        .ok()
        .as_deref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or(&app.repo_path)
        .to_string();

    let left_spans: Vec<Span> = vec![
        Span::styled("⛩", Style::default().fg(accent)),
        Span::raw(" "),
        Span::styled(
            repo_name,
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", view_label(&app.view)),
            Style::default().fg(theme::INK_DIM),
        ),
    ];

    let mut right_spans: Vec<Span> = Vec::new();
    if let Some(new_v) = &app.update_available {
        right_spans.push(Span::styled(
            format!("⬆ v{}  ", new_v),
            Style::default().fg(theme::WARN),
        ));
    }
    right_spans.extend(vec![
        Span::styled(&app.branch, Style::default().fg(theme::INK_DIM)),
        Span::raw("  "),
        Span::styled(status_label, Style::default().fg(status_color)),
    ]);

    let width = |spans: &[Span]| -> usize { spans.iter().map(|s| s.content.chars().count()).sum() };
    let pad = (area.width as usize).saturating_sub(width(&left_spans) + width(&right_spans) + 2);

    let mut spans = vec![Span::raw(" ")];
    spans.extend(left_spans);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.extend(right_spans);
    spans.push(Span::raw(" "));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_repo_picker(f: &mut Frame, app: &App, header_area: Rect) {
    let bc = app.brand_color();
    let paths = app.workspace_repo_paths();
    if paths.is_empty() {
        return;
    }

    let dropdown_w: u16 = paths
        .iter()
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().len())
                .unwrap_or(p.len())
                + 4
        })
        .max()
        .unwrap_or(20)
        .min(40) as u16;
    let dropdown_h = paths.len() as u16 + 2;

    // Position: just below "⛩  gitorii  /  " prefix (~18 chars + 1 border + 1 space)
    let x = header_area.x + 18;
    let y = header_area.y + header_area.height; // just below header
    let drop_area = Rect::new(x, y, dropdown_w, dropdown_h.min(header_area.height + 10));

    let current = std::fs::canonicalize(&app.repo_path).ok();
    let items: Vec<ListItem> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = std::path::Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.clone());
            let is_current = std::fs::canonicalize(p).ok() == current;
            let is_sel = i == app.repo_picker_idx;
            let color = if is_sel {
                C_WHITE
            } else if is_current {
                C_GREEN
            } else {
                C_SUBTLE
            };
            let prefix = if is_sel {
                "▶ "
            } else if is_current {
                "✓ "
            } else {
                "  "
            };
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(name, Style::default().fg(color)),
            ]))
            .style(style)
        })
        .collect();

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.repo_picker_idx));

    let block = Block::default()
        .title(Span::styled(
            " switch repo ",
            Style::default().fg(bc).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(bc));

    f.render_widget(Clear, drop_area);
    f.render_stateful_widget(List::new(items).block(block), drop_area, &mut state);
}

fn render_event_log(f: &mut Frame, app: &App, area: Rect) {
    let panel_w = (area.width / 3).clamp(28, 55);
    let panel_h = (area.height / 2).clamp(6, 24);
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

    let items: Vec<ListItem> = app
        .event_log
        .iter()
        .map(|e| {
            let kind_color = match e.kind {
                EventKind::Error => C_RED,
                EventKind::Success => C_GREEN,
                EventKind::Info => C_CYAN,
            };
            let kind_sym = match e.kind {
                EventKind::Error => "✗",
                EventKind::Success => "✓",
                EventKind::Info => "·",
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", e.timestamp), Style::default().fg(C_DIM)),
                Span::styled(kind_sym, Style::default().fg(kind_color)),
                Span::raw(" "),
                Span::styled(&e.message, Style::default().fg(C_WHITE)),
            ]))
        })
        .collect();

    f.render_widget(List::new(items), inner);
}

fn render_hint(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let bc = app.brand_color();
    // The sidebar's own keys, and then every view's, land in the same strip:
    // one place that pads, unbrackets and hangs the right-hand pair.
    let line = if app.sidebar_focused {
        Line::from(vec![
            Span::raw(" "),
            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
            Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[Enter]", Style::default().fg(bc)),
            Span::styled(" open  ", Style::default().fg(C_SUBTLE)),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
        ])
    } else {
        match app.view {
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
                    Span::styled(
                        if app.commit_view.amend {
                            "[amend ✓]"
                        } else {
                            ""
                        },
                        amend_style,
                    ),
                    Span::styled("  [Esc]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                ])
            }
            View::Sync => Line::from(vec![
                Span::raw(" "),
                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[Enter]", Style::default().fg(bc)),
                Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[Esc]", Style::default().fg(bc)),
                Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
            ]),
            View::Log => {
                if app.log.search_mode {
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
                }
            }
            View::Branch => {
                use crate::tui::app::BranchConfirm;
                match &app.branch_view.confirm {
                    BranchConfirm::Delete => {
                        let name = app
                            .branch_view
                            .branches
                            .get(app.branch_view.idx)
                            .map(|b| b.name.as_str())
                            .unwrap_or("?");
                        Line::from(vec![
                            Span::raw(" "),
                            Span::styled("delete ", Style::default().fg(C_SUBTLE)),
                            Span::styled(
                                name.to_string(),
                                Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("?  ", Style::default().fg(C_SUBTLE)),
                            Span::styled(
                                "[y]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                            Span::styled(
                                "[any]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" cancel", Style::default().fg(C_DIM)),
                        ])
                    }
                    BranchConfirm::NewBranch => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("new branch: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(
                            app.branch_view.new_name.clone(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    BranchConfirm::None => {
                        if app.branch_view.search_mode {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("search: ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    app.branch_view.search_query.clone(),
                                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                                ),
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
                            let color = if s.starts_with("checkout:")
                                || s.starts_with("created")
                                || s.starts_with("pushed")
                                || s.starts_with("deleted")
                            {
                                C_GREEN
                            } else if s.contains("failed") || s.contains("cannot") {
                                C_RED
                            } else {
                                C_YELLOW
                            };
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled(s.clone(), Style::default().fg(color)),
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
            }
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
                } else if app.snapshot_view.ops_mode
                    && app.snapshot_view.focus == SnapshotFocus::List
                {
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
                        Span::styled(
                            app.snapshot_view.create_name.clone(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
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
            }
            View::Tag => {
                use crate::tui::app::TagConfirm;
                match &app.tag_view.confirm {
                    TagConfirm::Delete => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("delete tag?  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[y]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                        Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                        Span::styled(
                            "[any]",
                            Style::default().fg(bc).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" cancel", Style::default().fg(C_DIM)),
                    ]),
                    TagConfirm::CreateName => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("tag name: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(
                            app.tag_view.new_name.as_str(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    TagConfirm::CreateMessage => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("message: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(
                            app.tag_view.new_message.as_str(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    TagConfirm::None => {
                        if app.tag_view.search_mode {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("search: ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    app.tag_view.search_query.clone(),
                                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                                ),
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
            }
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
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::RemoveFile => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("remove file from history: ", Style::default().fg(C_RED)),
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::RewriteStart => Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            "rewrite start date (YYYY-MM-DD HH:MM): ",
                            Style::default().fg(C_SUBTLE),
                        ),
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::RewriteEnd => Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            "rewrite end date (YYYY-MM-DD HH:MM): ",
                            Style::default().fg(C_SUBTLE),
                        ),
                        Span::styled(
                            app.history_view.input2.as_str(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::Blame => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("blame file: ", Style::default().fg(C_SUBTLE)),
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                        ),
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
            }
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
                            Span::styled(
                                app.remote_view.new_name.clone(),
                                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::AddUrl => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("remote url: ", Style::default().fg(C_SUBTLE)),
                            Span::styled(
                                app.remote_view.new_url.clone(),
                                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::Rename => {
                            let old = app
                                .remote_view
                                .remotes
                                .get(app.remote_view.idx)
                                .map(|r| r.name.as_str())
                                .unwrap_or("?");
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("rename ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    old.to_string(),
                                    Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" → ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    app.remote_view.new_name.clone(),
                                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled("█", Style::default().fg(bc)),
                            ])
                        }
                        RemoteConfirm::EditUrl => {
                            let name = app
                                .remote_view
                                .remotes
                                .get(app.remote_view.idx)
                                .map(|r| r.name.as_str())
                                .unwrap_or("?");
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("edit url for ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    name.to_string(),
                                    Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(": ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    app.remote_view.new_url.clone(),
                                    Style::default().fg(C_WHITE),
                                ),
                                Span::styled("█", Style::default().fg(bc)),
                            ])
                        }
                        RemoteConfirm::MirrorRename => {
                            let old = app
                                .remote_view
                                .selected_mirror()
                                .map(|m| m.name.as_str())
                                .unwrap_or("?");
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("rename mirror ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    old.to_string(),
                                    Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" → ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    app.remote_view.new_name.clone(),
                                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled("█", Style::default().fg(bc)),
                            ])
                        }
                        RemoteConfirm::MirrorAddPlatform => Line::from(vec![
                            Span::raw(" "),
                            Span::styled(
                                "mirror platform (github/gitlab/…): ",
                                Style::default().fg(C_SUBTLE),
                            ),
                            Span::styled(
                                app.remote_view.new_mirror_platform.clone(),
                                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::MirrorAddAccount => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("account: ", Style::default().fg(C_SUBTLE)),
                            Span::styled(
                                app.remote_view.new_mirror_account.clone(),
                                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::MirrorAddRepo => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("repo name: ", Style::default().fg(C_SUBTLE)),
                            Span::styled(
                                app.remote_view.new_mirror_repo.clone(),
                                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::MirrorAddType => {
                            let (replica_style, primary_style) =
                                if app.remote_view.new_mirror_type == 0 {
                                    (
                                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                                        Style::default().fg(C_SUBTLE),
                                    )
                                } else {
                                    (
                                        Style::default().fg(C_SUBTLE),
                                        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                                    )
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
                            let name = app
                                .remote_view
                                .remotes
                                .get(app.remote_view.idx)
                                .map(|r| r.name.as_str())
                                .unwrap_or("?");
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("remove remote ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    name.to_string(),
                                    Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled("?  ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    "[y]",
                                    Style::default().fg(bc).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                                Span::styled(
                                    "[any]",
                                    Style::default().fg(bc).add_modifier(Modifier::BOLD),
                                ),
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
            }
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
            }
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
            }
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
                            Span::styled(
                                "[y]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                            Span::styled(
                                "[any]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" cancel", Style::default().fg(C_DIM)),
                        ]),
                        WorkspaceConfirm::RemoveRepo => Line::from(vec![
                            Span::raw(" "),
                            Span::styled(
                                "remove repo from workspace?  ",
                                Style::default().fg(C_SUBTLE),
                            ),
                            Span::styled(
                                "[y]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" confirm  ", Style::default().fg(C_DIM)),
                            Span::styled(
                                "[any]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" cancel", Style::default().fg(C_DIM)),
                        ]),
                        WorkspaceConfirm::SaveMessage => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("commit message: ", Style::default().fg(C_SUBTLE)),
                            Span::styled(
                                app.workspace_view.input.clone(),
                                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        WorkspaceConfirm::AddRepoPath => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("repo path: ", Style::default().fg(C_SUBTLE)),
                            Span::styled(
                                app.workspace_view.input.clone(),
                                Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        WorkspaceConfirm::RenameWorkspace => {
                            let old = app
                                .workspace_view
                                .workspaces
                                .get(app.workspace_view.ws_idx)
                                .map(|ws| ws.name.as_str())
                                .unwrap_or("?");
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("rename ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    old.to_string(),
                                    Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" → ", Style::default().fg(C_SUBTLE)),
                                Span::styled(
                                    app.workspace_view.input.clone(),
                                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                                ),
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
            }
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
            }
            View::Settings => Line::from(vec![
                Span::raw(" "),
                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[Enter]", Style::default().fg(bc)),
                Span::styled(" toggle/edit  ", Style::default().fg(C_SUBTLE)),
                Span::styled("[s]", Style::default().fg(bc)),
                Span::styled(" save", Style::default().fg(C_SUBTLE)),
            ]),
            View::Submodule => {
                use crate::tui::app::SubmoduleFocus;
                match app.submodule_view.focus {
                    SubmoduleFocus::OpsDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(C_SUBTLE)),
                    ]),
                    SubmoduleFocus::InputArgs => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[type]", Style::default().fg(bc)),
                        Span::styled(" input  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" next/run  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    SubmoduleFocus::ConfirmRemove => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" yes  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[n/Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    SubmoduleFocus::List => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" sidebar", Style::default().fg(C_SUBTLE)),
                    ]),
                }
            }
            View::Worktree => {
                use crate::tui::app::WorktreeFocus;
                match app.worktree_view.focus {
                    WorktreeFocus::OpsDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(C_SUBTLE)),
                    ]),
                    WorktreeFocus::InputArgs => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[type]", Style::default().fg(bc)),
                        Span::styled(" input  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    WorktreeFocus::ConfirmRemove | WorktreeFocus::ConfirmPrune => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" yes  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[n/Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    WorktreeFocus::List => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" sidebar", Style::default().fg(C_SUBTLE)),
                    ]),
                }
            }
            View::Bisect => {
                use crate::tui::app::BisectFocus;
                match app.bisect_view.focus {
                    BisectFocus::OpsDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(C_SUBTLE)),
                    ]),
                    BisectFocus::InputArgs => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[type]", Style::default().fg(bc)),
                        Span::styled(" input  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    BisectFocus::RefPicker => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[type]", Style::default().fg(bc)),
                        Span::styled(" filter  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Tab]", Style::default().fg(bc)),
                        Span::styled(" bad/good  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Space]", Style::default().fg(bc)),
                        Span::styled(" toggle good  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run", Style::default().fg(C_SUBTLE)),
                    ]),
                    BisectFocus::ConfirmReset => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" reset  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[n/Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    BisectFocus::List => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" sidebar", Style::default().fg(C_SUBTLE)),
                    ]),
                }
            }
            View::Auth => {
                use crate::tui::app::AuthFocus;
                match app.auth_view.focus {
                    AuthFocus::OpsDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(C_SUBTLE)),
                    ]),
                    AuthFocus::InputToken => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[type/paste]", Style::default().fg(bc)),
                        Span::styled(" token  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" save  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    AuthFocus::ConfirmRemove => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" remove  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[n/Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(C_SUBTLE)),
                    ]),
                    AuthFocus::OauthFlow => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("any-key", Style::default().fg(bc)),
                        Span::styled(" close (when done)", Style::default().fg(C_SUBTLE)),
                    ]),
                    AuthFocus::List => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" sidebar", Style::default().fg(C_SUBTLE)),
                    ]),
                }
            }
            View::Platform => {
                use crate::tui::app::PlatformFocus;
                match app.platform_view.focus {
                    // Inside a dropdown / popup: navigate + enter to apply.
                    PlatformFocus::RemotePopup
                    | PlatformFocus::OpsDropdown
                    | PlatformFocus::FilterDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" apply  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(C_SUBTLE)),
                    ]),
                    // Inside the job log scrollback.
                    PlatformFocus::JobLog => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" scroll  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[End]", Style::default().fg(bc)),
                        Span::styled(" follow  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[p]", Style::default().fg(bc)),
                        Span::styled(" live  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" pager  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" back", Style::default().fg(C_SUBTLE)),
                    ]),
                    // Browsing a sub-tab list.
                    PlatformFocus::List | PlatformFocus::JobsOfPipeline => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[f]", Style::default().fg(bc)),
                        Span::styled(" filter  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[r]", Style::default().fg(bc)),
                        Span::styled(" remote  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[p]", Style::default().fg(bc)),
                        Span::styled(" live  ", Style::default().fg(C_SUBTLE)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" drill", Style::default().fg(C_SUBTLE)),
                    ]),
                }
            }
            _ => Line::from(""),
        }
    };

    // The event log and the repo picker hang off the right edge.
    let events_label = if app.show_event_log {
        " events ✓"
    } else {
        " events"
    };
    let has_siblings = app.workspace_has_siblings();
    let right_str = if has_siblings {
        format!("W  repos  e  {}  ", events_label.trim())
    } else {
        format!("e  {}  ", events_label.trim())
    };
    // The per-view arms above still write their keys as `[Enter]`; the site
    // sets them bare, so they are unwrapped here rather than in two thousand
    // lines of match arm.
    let mut spans: Vec<Span> = line.spans.into_iter().map(theme::unbracket).collect();

    // Arrow glyphs are three bytes each, so the gap has to be measured in
    // columns or the right-hand pair walks off the edge.
    let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_len: usize = right_str.chars().count();
    let pad = (area.width as usize).saturating_sub(left_len + right_len);

    spans.push(Span::raw(" ".repeat(pad)));
    if has_siblings {
        spans.extend(theme::key_hint(app, "W", "repos"));
    }
    spans.extend(theme::key_hint(app, "e", events_label.trim()));
    spans.push(Span::raw(" "));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The sidebar: a list of names against a hairline, not a box. Focus is shown
/// by the caret and the ink, the way the site's listbox shows it, rather than
/// by repainting a border white.
fn render_sidebar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let spine = theme::divider_right();
    let inner_area = spine.inner(area);
    f.render_widget(spine, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // tabs
            Constraint::Length(2), // help + quit
        ])
        .split(inner_area);

    let accent = theme::accent(app);

    let tab_items: Vec<ListItem> = TABS
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let is_current_view = app.view == tab.view;
            let is_sidebar_sel = app.sidebar_focused && i == app.sidebar_idx;

            // The open view keeps full ink; the sidebar cursor, when the
            // sidebar has focus, is the one carrying the caret.
            let label_style = if is_current_view {
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD)
            } else if is_sidebar_sel {
                Style::default().fg(theme::INK)
            } else {
                Style::default().fg(theme::INK_FAINT)
            };

            let label: &str = if tab.view == View::Pr {
                "pr/mr"
            } else {
                tab.label
            };

            let mut item = ListItem::new(Line::from(vec![
                theme::caret(
                    app,
                    is_sidebar_sel || (!app.sidebar_focused && is_current_view),
                ),
                Span::styled(label.to_string(), label_style),
            ]));
            if is_current_view || is_sidebar_sel {
                item = item.style(Style::default().bg(theme::selection(app)));
            }
            item
        })
        .collect();

    f.render_widget(
        List::new(tab_items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        rows[0],
    );

    // Help and quit sit on the key line's row, so the foot of the window reads
    // as one strip across both columns.
    // The key sits in the caret's column so these two line up with the
    // entries above them rather than starting a column of their own.
    let bottom = List::new(vec![
        ListItem::new(Line::from(vec![
            Span::styled("?", Style::default().fg(accent)),
            Span::styled(" help", Style::default().fg(theme::INK_FAINT)),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("q", Style::default().fg(accent)),
            Span::styled(" quit", Style::default().fg(theme::INK_FAINT)),
        ])),
    ]);
    f.render_widget(
        bottom.block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        rows[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    /// Render the chrome once and hand back the screen as text.
    fn screen(width: u16, height: u16) -> Vec<String> {
        screen_of(View::Dashboard, width, height)
    }

    /// The same, for a named view with its focus in the view rather than the
    /// sidebar — which is how a view is actually looked at.
    fn screen_of(view: View, width: u16, height: u16) -> Vec<String> {
        let mut app = App::new().expect("a repository to look at");
        if view != View::Dashboard {
            app.go_to(view);
            app.sidebar_focused = false;
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// The window is one box: a border around the edge, and inside it only
    /// rules. A second `┌` anywhere means a view has grown its own box again.
    #[test]
    fn chrome_draws_a_single_window() {
        let lines = screen(100, 30);
        let corners = lines
            .iter()
            .flat_map(|l| l.chars())
            .filter(|c| matches!(c, '┌' | '┐' | '└' | '┘' | '╭' | '╮' | '╰' | '╯'))
            .count();
        assert_eq!(
            corners,
            4,
            "expected one window, got:\n{}",
            lines.join("\n")
        );
    }

    /// Every horizontal rule has to meet the frame on both sides, or it reads
    /// as a line dropped inside a box rather than part of it.
    #[test]
    fn rules_tie_into_the_frame() {
        let lines = screen(100, 30);
        let rules: Vec<&String> = lines.iter().filter(|l| l.contains("───")).collect();
        assert!(rules.len() >= 3, "expected header, foot and content rules");
        for line in rules {
            if line.starts_with('╭')
                || line.starts_with('┌')
                || line.starts_with('╰')
                || line.starts_with('└')
            {
                continue; // the window's own top and bottom edges
            }
            // A rule inside the body starts after the sidebar, so what matters
            // is that both of its ends land on something: the junction it
            // begins at, and the window's right border.
            assert!(
                line.contains('├') && line.ends_with('┤'),
                "rule floats free: {line}"
            );
        }
    }

    /// The sidebar's rule runs the height of the body and meets both rules.
    #[test]
    fn sidebar_rule_meets_both_rules() {
        let lines = screen(100, 30);
        let spine = SIDEBAR_WIDTH as usize; // frame column + sidebar width - 1
        let at = |row: &String, i: usize| row.chars().nth(i).unwrap_or(' ');
        let header_rule = lines.iter().position(|l| l.starts_with('├')).unwrap();
        assert_eq!(at(&lines[header_rule], spine), '┬');
        assert_eq!(at(&lines[header_rule + 1], spine), '│');
    }

    /// Not an assertion — `cargo test -- --nocapture look_at_it` prints the
    /// screen, which is the only way to review a change of this kind.
    #[test]
    fn look_at_it() {
        for view in CONVERTED {
            println!("\n── {} ──", view_label(&view));
            println!("{}", screen_of(view, 100, 24).join("\n"));
        }
    }

    /// No rule may cross another without a junction. A `│` sitting directly
    /// on a `─` is a column that starts out of thin air, which is what the
    /// grid looked like before the views tied themselves into the chrome.
    /// The views converted to the window chrome so far. A view joins this
    /// list when it stops drawing its own boxes.
    const CONVERTED: [View; 15] = [
        View::Dashboard,
        View::Log,
        View::Branch,
        View::Commit,
        View::Sync,
        View::Snapshot,
        View::Tag,
        View::Remote,
        View::Workspace,
        View::Worktree,
        View::Submodule,
        View::Auth,
        View::Bisect,
        View::Config,
        View::Issue,
    ];

    #[test]
    fn rules_never_cross_without_a_junction() {
        for view in CONVERTED {
            check_junctions(&screen_of(view, 100, 30));
        }
    }

    fn check_junctions(lines: &[String]) {
        let at = |row: usize, col: usize| lines[row].chars().nth(col).unwrap_or(' ');
        for row in 1..lines.len() {
            for col in 0..lines[row].chars().count() {
                let here = at(row, col);
                let above = at(row - 1, col);
                assert!(
                    !(here == '│' && above == '─'),
                    "column at {col} starts on a rule at row {row}:\n{}",
                    lines.join("\n")
                );
                assert!(
                    !(here == '─' && above == '│'),
                    "column at {col} ends on a rule at row {row}:\n{}",
                    lines.join("\n")
                );
            }
        }
    }

    #[test]
    fn brackets_are_stripped_from_key_hints() {
        let lines = screen(100, 30);
        let foot = lines.last().unwrap();
        assert!(!foot.contains('['), "key hints still bracketed: {foot}");
    }
}
