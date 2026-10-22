use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use super::app::{App, EventKind, View};
use super::theme;
use super::views;

#[allow(dead_code)]
pub const SELECTED_BG: Color = Color::Rgb(40, 40, 60);

// Paleta viva.
//
// The tones with no consumer are silenced deliberately, the way SELECTED_BG
// and C_BORDER already were: this is a palette, and it is kept whole so the
// next view picks a tone from here instead of inventing one inline. C_WHITE,
// C_CYAN and C_YELLOW lost their only caller when views/mirror.rs was retired
// in 0.15.0; dropping them would leave four of seven and the next view that
// needed white would just redefine it.
#[allow(dead_code)]
pub const C_WHITE: Color = Color::Rgb(220, 220, 220);
pub const C_SUBTLE: Color = Color::Rgb(140, 140, 160);
pub const C_DIM: Color = Color::Rgb(80, 80, 100);
#[allow(dead_code)]
pub const C_CYAN: Color = Color::Rgb(80, 220, 200);
#[allow(dead_code)]
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
// Clone leads: getting a repository is how you start. Files is still
// what the TUI opens on, because you launch it inside one. Keep this
// aligned with `view_for_idx` + the sidebar_idx maps in `go_to` /
// `go_back` (src/tui/app.rs).
const TABS: &[Tab] = &[
    // entry
    Tab {
        key: "o",
        label: "clone",
        view: View::Clone,
    },
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
        key: "x",
        label: "safety",
        view: View::Safety,
    },
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
    // admin / look at the repo from a distance
    Tab {
        key: "z",
        label: "stats",
        view: View::Stats,
    },
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
    // The map describes the frame being drawn, so it starts empty every time.
    app.hits.borrow_mut().clear();

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
        // 0.16.0 — browse an account's repositories and clone one.
        View::Clone => views::clone::render(f, app, content_rows[0]),
        // Config absorbs Settings via tabs since 0.7.2.
        View::Config | View::Settings => views::config::render(f, app, content_rows[0]),
        View::Safety => views::safety::render(f, app, content_rows[0]),
        View::Stats => views::stats::render(f, app, content_rows[0]),
        View::Diff | View::Help => {}
    }

    render_hint(f, app, rows[4]);

    if app.show_event_log {
        render_event_log(f, app, area);
    }

    if app.repo_picker_open {
        render_repo_picker(f, app, rows[0]);
    }

    // Last, so it sits over everything: the palette is the way out when a
    // binding is wrong, forgotten, or was never made.
    if app.palette.open {
        render_palette(f, app, area);
    }

    // Over even the palette: the operation is stopped until it is answered.
    if let Some(p) = &app.conflict {
        render_conflict(f, app, p, area);
    }

    // A half-typed sequence says so, or `g` looks like a key that did nothing.
    if !app.pending_chords.is_empty() {
        render_pending_chords(f, app, rows[4]);
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
    let paths = app.workspace_repo_paths();
    if paths.is_empty() {
        return;
    }

    let name_of = |p: &String| {
        std::path::Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| p.clone())
    };
    // caret + name + "  current" + the box's two borders and padding.
    let dropdown_w: u16 = paths
        .iter()
        .map(|p| name_of(p).chars().count() + 15)
        .max()
        .unwrap_or(20)
        .min(48) as u16;
    let dropdown_h = paths.len() as u16 + 2;

    // Under the repository name in the chrome bar, which starts after " ⛩ ".
    let x = header_area.x + 2;
    let y = header_area.y + header_area.height;
    let drop_area = Rect::new(x, y, dropdown_w, dropdown_h.min(header_area.height + 10));

    let current = std::fs::canonicalize(&app.repo_path).ok();
    let items: Vec<ListItem> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_current = std::fs::canonicalize(p).ok() == current;
            let is_sel = i == app.repo_picker_idx;
            let mut spans = vec![
                theme::caret(app, is_sel),
                Span::styled(
                    name_of(p),
                    Style::default().fg(if is_current && !is_sel {
                        theme::OK
                    } else if is_sel {
                        theme::INK
                    } else {
                        theme::INK_DIM
                    }),
                ),
            ];
            if is_current {
                spans.push(Span::styled(
                    "  current",
                    Style::default().fg(theme::INK_FAINT),
                ));
            }
            ListItem::new(Line::from(spans)).style(if is_sel {
                Style::default()
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        })
        .collect();

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.repo_picker_idx));

    // A popup is a window, so it keeps its box — in the rule colour, like the
    // palette's.
    let block = Block::default()
        .title(Span::styled(
            " switch repo ",
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE));

    let drop_area = theme::clear(f, drop_area);
    f.render_stateful_widget(List::new(items).block(block), drop_area, &mut state);
}

/// Word-wrap `s` with a hanging indent: the first line gets `first`
/// columns (what is left after the timestamp and the symbol), every line
/// after it gets `rest`.
///
/// A word longer than the line — a hash, a path, an identity — is broken
/// rather than left to run off the edge, because running off the edge is
/// exactly what a `Line` in a `List` does: ratatui clips, it does not
/// wrap.
fn wrap_message(s: &str, first: usize, rest: usize) -> Vec<String> {
    let first = first.max(1);
    let rest = rest.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in s.split_whitespace() {
        let mut word = word;
        loop {
            let width = if lines.is_empty() { first } else { rest };
            let room = if current.is_empty() {
                width
            } else {
                width.saturating_sub(current.chars().count() + 1)
            };
            if word.chars().count() <= room {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
                break;
            }
            if current.is_empty() {
                let head: String = word.chars().take(width).collect();
                let cut = head.len();
                lines.push(head);
                word = &word[cut..];
            } else {
                lines.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn render_event_log(f: &mut Frame, app: &App, area: Rect) {
    // The preferred size, then what the window can actually spare. A minimum
    // wider than the terminal is not a minimum, it is an overflow: asking for
    // 28 columns inside 20 used to put the panel outside the buffer.
    let panel_w = (area.width / 3).clamp(28, 55).min(area.width);
    let panel_h = (area.height / 2).clamp(6, 24).min(area.height);
    let x = (area.x + area.width).saturating_sub(panel_w + 1);
    let y = (area.y + area.height).saturating_sub(panel_h + 1);
    let panel_area = theme::fit(f, Rect::new(x, y, panel_w, panel_h));

    // A popup keeps its box — it is a window — but the box is a rule, the
    // title is ink, and the keys read like the strip at the foot.
    let mut hint = vec![Span::raw(" ")];
    hint.extend(theme::key_hint(app, "e", "close"));
    hint.extend(theme::key_hint(app, "c", "clear"));
    let block = Block::default()
        .title(Span::styled(
            format!(" events ({}) ", app.event_log.len()),
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(hint))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE));

    let inner = block.inner(panel_area);
    let panel_area = theme::clear(f, panel_area);
    f.render_widget(block, panel_area);

    // What one line of the list can hold, less the block's own padding.
    // An event here is a whole sentence — a rewrite's before and after, a
    // scan's finding, the reason a commit was refused — and the panel is
    // 55 columns at its widest, so on one line the panel was showing a
    // timestamp and the first few words of everything.
    let text_w = inner.width.saturating_sub(2) as usize;
    const INDENT: usize = 2;

    let items: Vec<ListItem> = app
        .event_log
        .iter()
        .map(|e| {
            let (sym, color) = match e.kind {
                EventKind::Error => ("✗", theme::BAD),
                EventKind::Success => ("✓", theme::OK),
                EventKind::Info => ("·", theme::INK_FAINT),
            };
            // timestamp + space + symbol + space
            let prefix_w = e.timestamp.chars().count() + 3;
            let wrapped = wrap_message(
                &e.message,
                text_w.saturating_sub(prefix_w),
                text_w.saturating_sub(INDENT),
            );

            let mut rows = wrapped.into_iter();
            let head = rows.next().unwrap_or_default();
            let mut lines = vec![Line::from(vec![
                Span::styled(
                    format!("{} ", e.timestamp),
                    Style::default().fg(theme::INK_FAINT),
                ),
                Span::styled(sym, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(head, Style::default().fg(theme::INK_DIM)),
            ])];
            // The continuation hangs under the symbol rather than under
            // the message: aligning it there would cost eleven of the
            // panel's columns on every line.
            lines.extend(rows.map(|tail| {
                Line::from(vec![
                    Span::raw(" ".repeat(INDENT)),
                    Span::styled(tail, Style::default().fg(theme::INK_DIM)),
                ])
            }));
            ListItem::new(lines)
        })
        .collect();

    f.render_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        inner,
    );
}

/// The conflict prompt: which side wins, everywhere or file by file. Same
/// words and keys as the CLI prompt.
fn render_conflict(f: &mut Frame, app: &App, p: &super::app::ConflictPrompt, area: Rect) {
    use crate::cli::conflict_picker::{op_name, sanitize, FileState};

    let w = 72u16.min(area.width.saturating_sub(4));
    let h = match &p.picker {
        None => 9u16,
        Some(st) => st.files.len() as u16 + 4,
    }
    .min(area.height.saturating_sub(4));
    let overlay = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    let overlay = theme::clear(f, overlay);

    let remote_label = sanitize(p.job.target());
    let local_label = sanitize(&p.local_label);
    let mut hint = vec![Span::raw(" ")];
    if p.picker.is_some() {
        hint.extend(theme::key_hint(app, "space", "remote/local"));
        hint.extend(theme::key_hint(app, "backspace", "conflict"));
        hint.extend(theme::key_hint(app, "enter", "apply"));
        hint.extend(theme::key_hint(app, "esc", "cancel"));
    } else {
        hint.extend(theme::key_hint(app, "l", "local"));
        hint.extend(theme::key_hint(app, "r", "remote"));
        hint.extend(theme::key_hint(app, "f", "file by file"));
        hint.extend(theme::key_hint(app, "m", "manual"));
    }
    let n = p.files.len();
    let block = Block::default()
        .title(Span::styled(
            format!(" {} conflict{} ", n, if n == 1 { "" } else { "s" }),
            Style::default()
                .fg(theme::WARN)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(hint))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let Some(st) = &p.picker else {
        let faint = Style::default().fg(theme::INK_FAINT);
        let ink = Style::default().fg(theme::INK).add_modifier(Modifier::BOLD);
        let key = Style::default().fg(theme::accent(app));
        let lines = vec![
            Line::from(vec![
                Span::styled(format!("{}: ", op_name(p.job.op())), faint),
                Span::styled(remote_label.clone(), ink),
                Span::styled(" into ", faint),
                Span::styled(local_label.clone(), ink),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("local   ", Style::default().fg(theme::OK)),
                Span::styled(local_label, ink),
                Span::styled("  your work", faint),
            ]),
            Line::from(vec![
                Span::styled("remote  ", Style::default().fg(theme::INK_DIM)),
                Span::styled(remote_label, ink),
                Span::styled("  incoming", faint),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("l", key),
                Span::styled(" local wins everywhere   ", faint),
                Span::styled("r", key),
                Span::styled(" remote wins everywhere", faint),
            ]),
            Line::from(vec![
                Span::styled("f", key),
                Span::styled(" choose file by file     ", faint),
                Span::styled("m", key),
                Span::styled(" leave markers, resolve by hand", faint),
            ]),
        ];
        f.render_widget(
            Paragraph::new(lines).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
            inner,
        );
        return;
    };

    let items: Vec<ListItem> = st
        .files
        .iter()
        .enumerate()
        .map(|(i, (path, fs))| {
            let is_sel = i == st.cursor;
            let (label, color) = match fs {
                FileState::Conflict => ("conflict", theme::WARN),
                FileState::Remote => ("remote  ", theme::INK),
                FileState::Local => ("local   ", theme::OK),
            };
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(label, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(
                    sanitize(path),
                    Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                ),
            ]))
            .style(theme::row_style(app, is_sel))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(st.cursor));
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        inner,
        &mut state,
    );
}

/// The action palette: every action, filtered by what is typed.
fn render_palette(f: &mut Frame, app: &App, area: Rect) {
    let w = 64u16.min(area.width.saturating_sub(4));
    let h = 16u16.min(area.height.saturating_sub(4));
    let overlay = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    let overlay = theme::clear(f, overlay);

    let block = Block::default()
        .title(Span::styled(
            app.palette_title(),
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" > ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                app.palette.query.clone(),
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme::accent(app))),
        ])),
        rows[0],
    );

    let matches = app.palette_matches();
    let items: Vec<ListItem> = if matches.is_empty() {
        vec![ListItem::new(Span::styled(
            "no action matches",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        matches
            .iter()
            .enumerate()
            .map(|(i, action)| {
                let is_sel = i == app.palette.idx;
                // The row carries its own right-hand text: a binding for an
                // action, `current` for the branch you are on, a path for a
                // repo. The palette teaches the keys instead of replacing them.
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        format!("{:<30}", action.label),
                        Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                    ),
                    Span::styled(action.hint.clone(), Style::default().fg(theme::INK_FAINT)),
                ]))
                .style(if is_sel {
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
    state.select(Some(app.palette.idx.min(matches.len().saturating_sub(1))));
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        rows[1],
        &mut state,
    );
    // Drawn last, so these rows win the cells they cover — a click inside the
    // palette must never reach the view behind it.
    if !matches.is_empty() {
        let first = state.offset();
        app.hits.borrow_mut().rows(
            rows[1],
            "palette",
            first,
            matches.len().saturating_sub(first),
        );
    }
}

/// What has been pressed of a sequence, on the key line.
fn render_pending_chords(f: &mut Frame, app: &App, area: Rect) {
    let text: Vec<String> = app.pending_chords.iter().map(|c| c.to_string()).collect();
    let label = format!(" {} … ", text.join(" "));
    let w = label.chars().count() as u16;
    let spot = Rect::new(area.x, area.y, w.min(area.width), 1);
    let spot = theme::clear(f, spot);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label,
            Style::default()
                .fg(theme::INK)
                .bg(theme::selection(app))
                .add_modifier(Modifier::BOLD),
        ))),
        spot,
    );
}

/// The tail of a reauthor prompt: the format the prompt wants, or the
/// reason the identity just typed was refused.
///
/// Both halves of the pair are asked for in full — `"Name <email>"` —
/// and the old one is asked for in full precisely because it already
/// exists: it is copied off the commits being fixed, not invented. A
/// bare name or a bare email would also parse, and would also match
/// commits nobody asked to touch.
fn identity_hint<'a>(app: &'a App, hint: &'a str) -> Span<'a> {
    match &app.history_view.identity_error {
        Some(why) => Span::styled(format!("  {}", why), Style::default().fg(theme::BAD)),
        None => Span::styled(
            format!("  ({})", hint),
            Style::default().fg(theme::INK_FAINT),
        ),
    }
}

fn files_hint(app: &App, bc: Color) -> Line<'static> {
    if app.dashboard.browse {
        return Line::from(vec![
            Span::raw(" "),
            Span::styled("[t]", Style::default().fg(bc)),
            Span::styled(" / ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" status  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
            Span::styled(" move  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[Enter]", Style::default().fg(bc)),
            Span::styled(" / ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[space]", Style::default().fg(bc)),
            Span::styled(" fold", Style::default().fg(theme::INK_FAINT)),
        ]);
    }
    use crate::tui::app::Panel;
    match app.dashboard.selected_panel {
        Panel::Staged => Line::from(vec![
            Span::raw(" "),
            Span::styled("[space]", Style::default().fg(bc)),
            Span::styled(" unstage  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[d]", Style::default().fg(bc)),
            Span::styled(" diff  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[t]", Style::default().fg(bc)),
            Span::styled(" tree", Style::default().fg(theme::INK_FAINT)),
        ]),
        Panel::Unstaged => Line::from(vec![
            Span::raw(" "),
            Span::styled("[space]", Style::default().fg(bc)),
            Span::styled(" stage  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[d]", Style::default().fg(bc)),
            Span::styled(" diff  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[t]", Style::default().fg(bc)),
            Span::styled(" tree", Style::default().fg(theme::INK_FAINT)),
        ]),
        Panel::Untracked => Line::from(vec![
            Span::raw(" "),
            Span::styled("[space]", Style::default().fg(bc)),
            Span::styled(" stage  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[t]", Style::default().fg(bc)),
            Span::styled(" tree", Style::default().fg(theme::INK_FAINT)),
        ]),
        Panel::Log => Line::from(vec![
            Span::raw(" "),
            Span::styled("[d]", Style::default().fg(bc)),
            Span::styled(" diff  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[l]", Style::default().fg(bc)),
            Span::styled(" expand  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[t]", Style::default().fg(bc)),
            Span::styled(" tree", Style::default().fg(theme::INK_FAINT)),
        ]),
    }
}

/// The key strip for the clone view.
///
/// Lifted out of `render_hint` rather than written inside it: that
/// function is one `match` of nested `Line::from(vec![…])` arms over a
/// thousand lines long, and every arm added to it is another storey on
/// an expression tree the compiler has to walk in one go. This one has
/// states of its own on top of the view's, so it stands apart — and the
/// in-flight run is a third function again, so a stop asked mid-clone
/// does not land back on that tree.
fn clone_hint(app: &App, bc: Color) -> Line<'static> {
    match app.clone_view.run.as_ref() {
        Some(run) => clone_run_hint(run, bc),
        None => clone_idle_hint(app, bc),
    }
}

fn clone_run_hint(run: &crate::tui::app::CloneRun, bc: Color) -> Line<'static> {
    use crate::tui::app::CloneStatus;
    if matches!(&run.status, CloneStatus::Starting | CloneStatus::Working(_)) && run.cancel_asked()
    {
        return Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "stopping — waiting for the transfer to answer",
                Style::default().fg(theme::WARN),
            ),
        ]);
    }
    match &run.status {
        CloneStatus::Starting => Line::from(vec![
            Span::raw(" "),
            Span::styled("connecting…  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" stop", Style::default().fg(theme::INK_FAINT)),
        ]),
        CloneStatus::Working(p) => Line::from(vec![
            Span::raw(" "),
            Span::styled(
                if p.resolving_deltas() {
                    "resolving deltas "
                } else {
                    "receiving objects "
                },
                Style::default().fg(theme::INK_FAINT),
            ),
            Span::styled(
                format!("{}%", p.percent()),
                Style::default().fg(bc).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" stop", Style::default().fg(theme::INK_FAINT)),
        ]),
        CloneStatus::Done(_) | CloneStatus::Cancelled | CloneStatus::Error(_) => Line::from(vec![
            Span::raw(" "),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" dismiss", Style::default().fg(theme::INK_FAINT)),
        ]),
    }
}

fn clone_idle_hint(app: &App, bc: Color) -> Line<'static> {
    use crate::tui::app::CloneFocus;
    match app.clone_view.focus {
        CloneFocus::Platforms => Line::from(vec![
            Span::raw(" "),
            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
            Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[Enter]", Style::default().fg(bc)),
            Span::styled(" switch  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
        ]),
        CloneFocus::Filter => Line::from(vec![
            Span::raw(" "),
            Span::styled("filter: ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                app.clone_view.filter.clone(),
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(bc)),
        ]),
        CloneFocus::Url => Line::from(vec![
            Span::raw(" "),
            Span::styled("url: ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                app.clone_view.url_input.clone(),
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(bc)),
        ]),
        CloneFocus::Dest => Line::from(vec![
            Span::raw(" "),
            Span::styled("into: ", Style::default().fg(theme::INK_FAINT)),
            Span::styled(
                app.clone_view.dest_input.clone(),
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(bc)),
        ]),
        CloneFocus::List => Line::from(vec![
            Span::raw(" "),
            Span::styled("[Enter]", Style::default().fg(bc)),
            Span::styled(" clone  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[p]", Style::default().fg(bc)),
            Span::styled(" platform  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[/]", Style::default().fg(bc)),
            Span::styled(" filter  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[u]", Style::default().fg(bc)),
            Span::styled(" url  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[d]", Style::default().fg(bc)),
            Span::styled(" into  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[t]", Style::default().fg(bc)),
            Span::styled(
                if app.clone_view.ssh { " ssh" } else { " https" },
                Style::default().fg(theme::INK_FAINT),
            ),
        ]),
    }
}

fn render_hint(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let bc = app.brand_color();
    // The sidebar's own keys, and then every view's, land in the same strip:
    // one place that pads, unbrackets and hangs the right-hand pair.
    let line = if app.sidebar_focused {
        Line::from(vec![
            Span::raw(" "),
            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
            Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[Enter]", Style::default().fg(bc)),
            Span::styled(" open  ", Style::default().fg(theme::INK_FAINT)),
            Span::styled("[Esc]", Style::default().fg(bc)),
            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
        ])
    } else {
        match app.view {
            View::Dashboard => files_hint(app, bc),
            View::Commit => {
                let amend_style = if app.commit_view.amend {
                    Style::default().fg(bc).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::INK_FAINT)
                };
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" save  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[←→]", Style::default().fg(bc)),
                    Span::styled(" cursor  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[a]", Style::default().fg(bc)),
                    Span::styled(" amend ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled(
                        if app.commit_view.amend {
                            "[amend ✓]"
                        } else {
                            ""
                        },
                        amend_style,
                    ),
                    Span::styled("  [Esc]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                ])
            }
            View::Sync => Line::from(vec![
                Span::raw(" "),
                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                Span::styled("[Enter]", Style::default().fg(bc)),
                Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                Span::styled("[Esc]", Style::default().fg(bc)),
                Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
            ]),
            View::Log => {
                if app.log.search_mode {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel search", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else if app.log.ops_mode {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else if !app.log.selected.is_empty() {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            format!("{} picked ", app.log.selected.len()),
                            Style::default().fg(theme::OK),
                        ),
                        Span::styled(
                            app.log
                                .selection_identity
                                .as_deref()
                                .unwrap_or("")
                                .to_string(),
                            Style::default().fg(theme::INK_FAINT),
                        ),
                        Span::styled("  [space]", Style::default().fg(bc)),
                        Span::styled(" pick  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" operations  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" clear", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[space]", Style::default().fg(bc)),
                        Span::styled(" pick  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" operations  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[/]", Style::default().fg(bc)),
                        Span::styled(" search  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[A]", Style::default().fg(bc)),
                        Span::styled(" authors", Style::default().fg(theme::INK_FAINT)),
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
                            Span::styled("delete ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                name.to_string(),
                                Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("?  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                "[y]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                "[any]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ])
                    }
                    BranchConfirm::NewBranch => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("new branch: ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled(
                            app.branch_view.new_name.clone(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    BranchConfirm::None => {
                        if app.branch_view.search_mode {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("search: ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    app.branch_view.search_query.clone(),
                                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled("█  ", Style::default().fg(bc)),
                                Span::styled("[Enter]", Style::default().fg(bc)),
                                Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Esc]", Style::default().fg(bc)),
                                Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                            ])
                        } else if app.branch_view.ops_mode {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Enter]", Style::default().fg(bc)),
                                Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Esc]", Style::default().fg(bc)),
                                Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                            ])
                        } else if let Some(s) = &app.branch_view.status {
                            let color = if s.starts_with("checkout:")
                                || s.starts_with("created")
                                || s.starts_with("pushed")
                                || s.starts_with("deleted")
                            {
                                theme::OK
                            } else if s.contains("failed") || s.contains("cannot") {
                                theme::BAD
                            } else {
                                theme::WARN
                            };
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled(s.clone(), Style::default().fg(color)),
                            ])
                        } else {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[o]", Style::default().fg(bc)),
                                Span::styled(
                                    " operations  ",
                                    Style::default().fg(theme::INK_FAINT),
                                ),
                                Span::styled("[/]", Style::default().fg(bc)),
                                Span::styled(" search", Style::default().fg(theme::INK_FAINT)),
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
                        Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel search", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else if app.snapshot_view.ops_mode
                    && app.snapshot_view.focus == SnapshotFocus::List
                {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else if app.snapshot_view.focus == SnapshotFocus::Create {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("snapshot name: ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled(
                            app.snapshot_view.create_name.clone(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ])
                } else if app.snapshot_view.focus == SnapshotFocus::AutoCustom {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("interval  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled(
                            app.snapshot_view.auto_custom_input.clone(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                        Span::styled("  30m / 1h / 2h", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else if app.snapshot_view.focus == SnapshotFocus::AutoConfig {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" set  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" back", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" operations  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[/]", Style::default().fg(bc)),
                        Span::styled(" search  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[d]", Style::default().fg(bc)),
                        Span::styled(" diff  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[a]", Style::default().fg(bc)),
                        Span::styled(" auto-config", Style::default().fg(theme::INK_FAINT)),
                    ])
                }
            }
            View::Tag => {
                use crate::tui::app::TagConfirm;
                match &app.tag_view.confirm {
                    TagConfirm::Delete => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("delete tag?  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[y]", Style::default().fg(bc).add_modifier(Modifier::BOLD)),
                        Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled(
                            "[any]",
                            Style::default().fg(bc).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    TagConfirm::CreateName => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("tag name: ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled(
                            app.tag_view.new_name.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    TagConfirm::CreateMessage => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("message: ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled(
                            app.tag_view.new_message.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    TagConfirm::None => {
                        if app.tag_view.search_mode {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("search: ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    app.tag_view.search_query.clone(),
                                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled("█  ", Style::default().fg(bc)),
                                Span::styled("[Enter]", Style::default().fg(bc)),
                                Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Esc]", Style::default().fg(bc)),
                                Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                            ])
                        } else if app.tag_view.ops_mode {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Enter]", Style::default().fg(bc)),
                                Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Esc]", Style::default().fg(bc)),
                                Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                            ])
                        } else {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[o]", Style::default().fg(bc)),
                                Span::styled(
                                    " operations  ",
                                    Style::default().fg(theme::INK_FAINT),
                                ),
                                Span::styled("[/]", Style::default().fg(bc)),
                                Span::styled(" search", Style::default().fg(theme::INK_FAINT)),
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
                        Span::styled(
                            "cherry-pick commit?  ",
                            Style::default().fg(theme::INK_FAINT),
                        ),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[any]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    HistoryConfirm::Clean => Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            "clean history & GC?  ",
                            Style::default().fg(theme::INK_FAINT),
                        ),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[any]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    HistoryConfirm::Rebase => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("rebase onto: ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::RemoveFile => Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            "remove file from history: ",
                            Style::default().fg(theme::BAD),
                        ),
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::RewriteStart => Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            "rewrite start date (YYYY-MM-DD HH:MM): ",
                            Style::default().fg(theme::INK_FAINT),
                        ),
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::RewriteEnd => Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            "rewrite end date (YYYY-MM-DD HH:MM): ",
                            Style::default().fg(theme::INK_FAINT),
                        ),
                        Span::styled(
                            app.history_view.input2.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::ReauthorOld => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("reauthor — old identity: ", Style::default().fg(theme::BAD)),
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                        identity_hint(
                            app,
                            "e.g. Ada Lovelace <ada@example.com> — as it stands in the commits",
                        ),
                    ]),
                    HistoryConfirm::ReauthorNew => Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            match app.log.selected.len() {
                                0 => "reauthor — new identity: ".to_string(),
                                n => format!("reauthor {} picked — new identity: ", n),
                            },
                            Style::default().fg(theme::BAD),
                        ),
                        Span::styled(
                            app.history_view.input2.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                        identity_hint(
                            app,
                            "e.g. Ada Lovelace <ada@example.com> — both halves required",
                        ),
                    ]),
                    HistoryConfirm::Blame => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("blame file: ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled(
                            app.history_view.input.as_str(),
                            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("█", Style::default().fg(bc)),
                    ]),
                    HistoryConfirm::Scan => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[f]", Style::default().fg(bc)),
                        Span::styled(" toggle mode  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run scan  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    HistoryConfirm::None => {
                        if app.history_view.ops_mode {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Enter]", Style::default().fg(bc)),
                                Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Esc]", Style::default().fg(bc)),
                                Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                            ])
                        } else {
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[o]", Style::default().fg(bc)),
                                Span::styled(" operations", Style::default().fg(theme::INK_FAINT)),
                            ])
                        }
                    }
                }
            }
            View::Clone => clone_hint(app, bc),
            View::Remote => {
                use crate::tui::app::RemoteConfirm;
                if app.remote_view.ops_mode {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else {
                    match &app.remote_view.confirm {
                        RemoteConfirm::AddName => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("remote name: ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                app.remote_view.new_name.clone(),
                                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::AddUrl => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("remote url: ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                app.remote_view.new_url.clone(),
                                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
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
                                Span::styled("rename ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    old.to_string(),
                                    Style::default()
                                        .fg(theme::WARN)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" → ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    app.remote_view.new_name.clone(),
                                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
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
                                Span::styled(
                                    "edit url for ",
                                    Style::default().fg(theme::INK_FAINT),
                                ),
                                Span::styled(
                                    name.to_string(),
                                    Style::default()
                                        .fg(theme::WARN)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(": ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    app.remote_view.new_url.clone(),
                                    Style::default().fg(theme::INK),
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
                                Span::styled(
                                    "rename mirror ",
                                    Style::default().fg(theme::INK_FAINT),
                                ),
                                Span::styled(
                                    old.to_string(),
                                    Style::default()
                                        .fg(theme::WARN)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" → ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    app.remote_view.new_name.clone(),
                                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled("█", Style::default().fg(bc)),
                            ])
                        }
                        RemoteConfirm::MirrorAddPlatform => Line::from(vec![
                            Span::raw(" "),
                            Span::styled(
                                "mirror platform (github/gitlab/…): ",
                                Style::default().fg(theme::INK_FAINT),
                            ),
                            Span::styled(
                                app.remote_view.new_mirror_platform.clone(),
                                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::MirrorAddAccount => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("account: ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                app.remote_view.new_mirror_account.clone(),
                                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::MirrorAddRepo => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("repo name: ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                app.remote_view.new_mirror_repo.clone(),
                                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        RemoteConfirm::MirrorAddType => {
                            let (replica_style, primary_style) = if app.remote_view.new_mirror_type
                                == 0
                            {
                                (
                                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                                    Style::default().fg(theme::INK_FAINT),
                                )
                            } else {
                                (
                                    Style::default().fg(theme::INK_FAINT),
                                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                                )
                            };
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled("type: ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("replica", replica_style),
                                Span::styled(" / ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("primary", primary_style),
                                Span::styled("  [←→]", Style::default().fg(bc)),
                                Span::styled(" toggle  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled("[Enter]", Style::default().fg(bc)),
                                Span::styled(" confirm", Style::default().fg(theme::INK_FAINT)),
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
                                Span::styled(
                                    "remove remote ",
                                    Style::default().fg(theme::INK_FAINT),
                                ),
                                Span::styled(
                                    name.to_string(),
                                    Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled("?  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    "[y]",
                                    Style::default().fg(bc).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    "[any]",
                                    Style::default().fg(bc).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                            ])
                        }
                        RemoteConfirm::None => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[o]", Style::default().fg(bc)),
                            Span::styled(" operations", Style::default().fg(theme::INK_FAINT)),
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
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else {
                    match &app.issue_view.confirm {
                        IssueConfirm::CreateTitle => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("title  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" next  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        IssueConfirm::CreateDesc => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("description  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" create  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        IssueConfirm::Comment => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("comment  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" send  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        IssueConfirm::Close => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[y]", Style::default().fg(bc)),
                            Span::styled(" confirm close  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[any]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        IssueConfirm::SwitchPlatform => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" switch  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        IssueConfirm::None => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[o]", Style::default().fg(bc)),
                            Span::styled(" operations  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[p]", Style::default().fg(bc)),
                            Span::styled(" platform  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[^r]", Style::default().fg(bc)),
                            Span::styled(" refresh", Style::default().fg(theme::INK_FAINT)),
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
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else {
                    match &app.pr_view.confirm {
                        PrConfirm::Merge => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[←→]", Style::default().fg(bc)),
                            Span::styled(" method  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" merge  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::Close => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[y]", Style::default().fg(bc)),
                            Span::styled(" confirm close  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[any]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::CreateTitle => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" next  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::CreateHead => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(
                                " select source branch  ",
                                Style::default().fg(theme::INK_FAINT),
                            ),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" next  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::CreateBase => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" select base  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" next  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::CreateDesc => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" new line  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[^S]", Style::default().fg(bc)),
                            Span::styled(" next  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Tab]", Style::default().fg(bc)),
                            Span::styled(" draft  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::CreatePlatforms => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[Space]", Style::default().fg(bc)),
                            Span::styled(" toggle  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[a]", Style::default().fg(bc)),
                            Span::styled(" all  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" create  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::SwitchPlatform => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" switch  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::EditTitle => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" next  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::EditDesc => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" new line  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[^S]", Style::default().fg(bc)),
                            Span::styled(" next  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::EditBase => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" select base  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Enter]", Style::default().fg(bc)),
                            Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Esc]", Style::default().fg(bc)),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        PrConfirm::None => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                            Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[o]", Style::default().fg(bc)),
                            Span::styled(" operations  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[Tab]", Style::default().fg(bc)),
                            Span::styled(" filter  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled("[^r]", Style::default().fg(bc)),
                            Span::styled(" refresh", Style::default().fg(theme::INK_FAINT)),
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
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else {
                    match &app.workspace_view.confirm {
                        WorkspaceConfirm::DeleteWorkspace => Line::from(vec![
                            Span::raw(" "),
                            Span::styled(
                                "delete workspace?  ",
                                Style::default().fg(theme::INK_FAINT),
                            ),
                            Span::styled(
                                "[y]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                "[any]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        WorkspaceConfirm::RemoveRepo => Line::from(vec![
                            Span::raw(" "),
                            Span::styled(
                                "remove repo from workspace?  ",
                                Style::default().fg(theme::INK_FAINT),
                            ),
                            Span::styled(
                                "[y]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" confirm  ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                "[any]",
                                Style::default().fg(bc).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                        ]),
                        WorkspaceConfirm::SaveMessage => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("commit message: ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                app.workspace_view.input.clone(),
                                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("█", Style::default().fg(bc)),
                        ]),
                        WorkspaceConfirm::AddRepoPath => Line::from(vec![
                            Span::raw(" "),
                            Span::styled("repo path: ", Style::default().fg(theme::INK_FAINT)),
                            Span::styled(
                                app.workspace_view.input.clone(),
                                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
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
                                Span::styled("rename ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    old.to_string(),
                                    Style::default()
                                        .fg(theme::WARN)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" → ", Style::default().fg(theme::INK_FAINT)),
                                Span::styled(
                                    app.workspace_view.input.clone(),
                                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled("█", Style::default().fg(bc)),
                            ])
                        }
                        WorkspaceConfirm::None => {
                            if app.workspace_view.focus == WorkspaceFocus::Workspaces {
                                Line::from(vec![
                                    Span::raw(" "),
                                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                    Span::styled(
                                        " navigate  ",
                                        Style::default().fg(theme::INK_FAINT),
                                    ),
                                    Span::styled("[→/l]", Style::default().fg(bc)),
                                    Span::styled(" repos  ", Style::default().fg(theme::INK_FAINT)),
                                    Span::styled("[o]", Style::default().fg(bc)),
                                    Span::styled(
                                        " operations",
                                        Style::default().fg(theme::INK_FAINT),
                                    ),
                                ])
                            } else {
                                Line::from(vec![
                                    Span::raw(" "),
                                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                                    Span::styled(
                                        " navigate  ",
                                        Style::default().fg(theme::INK_FAINT),
                                    ),
                                    Span::styled("[Enter]", Style::default().fg(bc)),
                                    Span::styled(" open  ", Style::default().fg(theme::INK_FAINT)),
                                    Span::styled("[o]", Style::default().fg(bc)),
                                    Span::styled(
                                        " operations  ",
                                        Style::default().fg(theme::INK_FAINT),
                                    ),
                                    Span::styled("[←/h]", Style::default().fg(bc)),
                                    Span::styled(
                                        " workspaces",
                                        Style::default().fg(theme::INK_FAINT),
                                    ),
                                ])
                            }
                        }
                    }
                }
            }
            View::Safety if app.ignore_view.focus == crate::tui::app::IgnoreFocus::SettingInput => {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[^T]", Style::default().fg(bc)),
                    Span::styled(" target  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" save  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                ])
            }
            View::Safety if app.ignore_view.tab == crate::tui::app::SafetyTab::Scanner => {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" set  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[d]", Style::default().fg(bc)),
                    Span::styled(" unset  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[a]", Style::default().fg(bc)),
                    Span::styled(" new pattern  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[1]", Style::default().fg(bc)),
                    Span::styled(" rules", Style::default().fg(theme::INK_FAINT)),
                ])
            }
            View::Safety => match app.ignore_view.focus {
                crate::tui::app::IgnoreFocus::Input => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[Tab]", Style::default().fg(bc)),
                    Span::styled(" kind  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[^T]", Style::default().fg(bc)),
                    Span::styled(" target  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" add  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                ]),
                crate::tui::app::IgnoreFocus::ConfirmDelete => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[y]", Style::default().fg(bc)),
                    Span::styled(" remove  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[n]", Style::default().fg(bc)),
                    Span::styled(" keep", Style::default().fg(theme::INK_FAINT)),
                ]),
                crate::tui::app::IgnoreFocus::SettingInput => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" save  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[Esc]", Style::default().fg(bc)),
                    Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                ]),
                crate::tui::app::IgnoreFocus::List => Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[a]", Style::default().fg(bc)),
                    Span::styled(" add  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[d]", Style::default().fg(bc)),
                    Span::styled(" remove  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[t]", Style::default().fg(bc)),
                    Span::styled(" path/secret  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[2]", Style::default().fg(bc)),
                    Span::styled(" scanner", Style::default().fg(theme::INK_FAINT)),
                ]),
            },
            View::Stats => Line::from(vec![
                Span::raw(" "),
                Span::styled("[1]", Style::default().fg(bc)),
                Span::styled(" repo  ", Style::default().fg(theme::INK_FAINT)),
                Span::styled("[2]", Style::default().fg(bc)),
                Span::styled(" workspace  ", Style::default().fg(theme::INK_FAINT)),
                Span::styled("[3]", Style::default().fg(bc)),
                Span::styled(" people  ", Style::default().fg(theme::INK_FAINT)),
                Span::styled("[r]", Style::default().fg(bc)),
                Span::styled(" measure again", Style::default().fg(theme::INK_FAINT)),
            ]),
            View::Config if app.config_view.tab == crate::tui::app::ConfigTab::Tui => {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                    Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[Enter]", Style::default().fg(bc)),
                    Span::styled(" change  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled("[1]", Style::default().fg(bc)),
                    Span::styled(" values", Style::default().fg(theme::INK_FAINT)),
                ])
            }
            View::Config if app.config_view.tab == crate::tui::app::ConfigTab::Keys => {
                if app.config_view.capturing.is_some() {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[any key]", Style::default().fg(bc)),
                        Span::styled(" record  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" save  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" bind  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[d]", Style::default().fg(bc)),
                        Span::styled(" unbind  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[1]", Style::default().fg(bc)),
                        Span::styled(" values", Style::default().fg(theme::INK_FAINT)),
                    ])
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
                        Span::styled(" save  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" edit  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Tab]", Style::default().fg(bc)),
                        Span::styled(" toggle scope", Style::default().fg(theme::INK_FAINT)),
                    ])
                }
            }
            View::Settings => Line::from(vec![
                Span::raw(" "),
                Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                Span::styled("[Enter]", Style::default().fg(bc)),
                Span::styled(" toggle/edit  ", Style::default().fg(theme::INK_FAINT)),
                Span::styled("[s]", Style::default().fg(bc)),
                Span::styled(" save", Style::default().fg(theme::INK_FAINT)),
            ]),
            View::Submodule => {
                use crate::tui::app::SubmoduleFocus;
                match app.submodule_view.focus {
                    SubmoduleFocus::OpsDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    SubmoduleFocus::InputArgs => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[type]", Style::default().fg(bc)),
                        Span::styled(" input  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" next/run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    SubmoduleFocus::ConfirmRemove => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" yes  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[n/Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    SubmoduleFocus::List => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" sidebar", Style::default().fg(theme::INK_FAINT)),
                    ]),
                }
            }
            View::Worktree => {
                use crate::tui::app::WorktreeFocus;
                match app.worktree_view.focus {
                    WorktreeFocus::OpsDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    WorktreeFocus::InputArgs => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[type]", Style::default().fg(bc)),
                        Span::styled(" input  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    WorktreeFocus::ConfirmRemove | WorktreeFocus::ConfirmPrune => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" yes  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[n/Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    WorktreeFocus::List => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" sidebar", Style::default().fg(theme::INK_FAINT)),
                    ]),
                }
            }
            View::Bisect => {
                use crate::tui::app::BisectFocus;
                match app.bisect_view.focus {
                    BisectFocus::OpsDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    BisectFocus::InputArgs => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[type]", Style::default().fg(bc)),
                        Span::styled(" input  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    BisectFocus::RefPicker => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[type]", Style::default().fg(bc)),
                        Span::styled(" filter  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Tab]", Style::default().fg(bc)),
                        Span::styled(" bad/good  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Space]", Style::default().fg(bc)),
                        Span::styled(" toggle good  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    BisectFocus::ConfirmReset => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" reset  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[n/Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    BisectFocus::List => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" sidebar", Style::default().fg(theme::INK_FAINT)),
                    ]),
                }
            }
            View::Auth => {
                use crate::tui::app::AuthFocus;
                match app.auth_view.focus {
                    AuthFocus::OpsDropdown => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" run  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    AuthFocus::InputToken => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[type/paste]", Style::default().fg(bc)),
                        Span::styled(" token  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" save  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    AuthFocus::ConfirmRemove => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[y]", Style::default().fg(bc)),
                        Span::styled(" remove  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[n/Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    AuthFocus::OauthFlow => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" cancel  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("any-key", Style::default().fg(bc)),
                        Span::styled(" close (when done)", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    AuthFocus::List => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" select  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" sidebar", Style::default().fg(theme::INK_FAINT)),
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
                        Span::styled(" navigate  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" apply  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" close", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    // Inside the job log scrollback.
                    PlatformFocus::JobLog => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[↑↓/jk]", Style::default().fg(bc)),
                        Span::styled(" scroll  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[End]", Style::default().fg(bc)),
                        Span::styled(" follow  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[p]", Style::default().fg(bc)),
                        Span::styled(" live  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" pager  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Esc]", Style::default().fg(bc)),
                        Span::styled(" back", Style::default().fg(theme::INK_FAINT)),
                    ]),
                    // Browsing a sub-tab list.
                    PlatformFocus::List | PlatformFocus::JobsOfPipeline => Line::from(vec![
                        Span::raw(" "),
                        Span::styled("[o]", Style::default().fg(bc)),
                        Span::styled(" ops  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[f]", Style::default().fg(bc)),
                        Span::styled(" filter  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[r]", Style::default().fg(bc)),
                        Span::styled(" remote  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[p]", Style::default().fg(bc)),
                        Span::styled(" live  ", Style::default().fg(theme::INK_FAINT)),
                        Span::styled("[Enter]", Style::default().fg(bc)),
                        Span::styled(" drill", Style::default().fg(theme::INK_FAINT)),
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

    // The key spans are registered as their own zones, so the foot of the
    // window works as a row of buttons. A key is recognised by its style —
    // the accent is only ever used for keys on this line — which keeps the
    // registration out of two thousand lines of match arm.
    {
        let accent = theme::accent(app);
        let mut hits = app.hits.borrow_mut();
        let mut x = area.x;
        for span in &spans {
            let width = span.content.chars().count() as u16;
            if span.style.fg == Some(accent) && !span.content.trim().is_empty() {
                hits.push(
                    ratatui::layout::Rect::new(x, area.y, width, 1),
                    crate::tui::hit::Zone::Key(span.content.trim().to_string()),
                );
            }
            x = x.saturating_add(width);
        }
    }

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

    // The list outgrew the shortest terminals, so it scrolls: whichever row
    // is in play — the sidebar cursor, or the open view — stays on screen.
    // One zone per visible tab, so a click lands on the view it names.
    {
        let mut hits = app.hits.borrow_mut();
        for (i, _) in TABS.iter().enumerate() {
            let y = rows[0].y + i as u16;
            if y >= rows[0].bottom() {
                break;
            }
            hits.push(
                ratatui::layout::Rect::new(rows[0].x, y, rows[0].width, 1),
                crate::tui::hit::Zone::Sidebar(i),
            );
        }
    }

    let mut state = ListState::default();
    let focus_row = if app.sidebar_focused {
        app.sidebar_idx
    } else {
        TABS.iter().position(|t| t.view == app.view).unwrap_or(0)
    };
    state.select(Some(focus_row.min(TABS.len().saturating_sub(1))));
    f.render_stateful_widget(
        List::new(tab_items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        rows[0],
        &mut state,
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

    /// The sidebar order lives in four places — `TABS` here, `view_for_idx`,
    /// `go_to` and `go_back` in the app — and nothing but this test makes them
    /// agree. Moving one entry and forgetting another sends a key to the wrong
    /// view, or leaves the cursor on a row that is not the open one.
    #[test]
    fn the_sidebar_order_agrees_with_itself() {
        let mut app = App::new().expect("a repository to look at");

        // Clone leads the list; the TUI still opens on files.
        assert_eq!(app.view, View::Dashboard);
        assert_eq!(
            app.sidebar_idx,
            TABS.iter().position(|t| t.view == View::Dashboard).unwrap()
        );
        assert_eq!(TABS[0].view, View::Clone);

        for (i, tab) in TABS.iter().enumerate() {
            // Walking the sidebar to row `i` must open the view TABS shows there.
            app.sidebar_idx = i;
            app.sidebar_enter();
            assert_eq!(
                app.view, tab.view,
                "row {i} shows `{}` but opens another view",
                tab.label
            );

            // …and going to that view by any other route must put the cursor
            // back on row `i`.
            app.go_to(View::Dashboard);
            app.go_to(tab.view.clone());
            assert_eq!(
                app.sidebar_idx, i,
                "opening `{}` leaves the cursor somewhere else",
                tab.label
            );
        }
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

    /// The renderer is handed `filtered` straight from state. If a checkout
    /// ever leaves an index behind again, the frame must degrade, not panic.
    #[test]
    fn the_log_survives_an_index_past_the_end() {
        let mut app = App::new().expect("a repository to look at");
        app.go_to(View::Log);
        app.sidebar_focused = false;
        app.log.search_query = "feat".to_string();
        app.log.filtered = vec![0, app.commits.len() + 500];
        app.log.idx = app.commits.len() + 500;

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render(f, &app)).expect("a frame");
    }

    /// The heading names the filter for what it is. Left as
    /// `matching "author:paski"` it reads as a search for that literal
    /// string, which is the one thing it is not.
    #[test]
    fn an_author_filter_says_so_in_the_heading() {
        let mut app = App::new().expect("a repository to look at");
        app.go_to(View::Log);
        app.sidebar_focused = false;
        app.log.search_query = "author:paski".to_string();
        app.log_update_filter();

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render(f, &app)).expect("a frame");
        let buffer = terminal.backend().buffer().clone();
        let screen: String = (0..30)
            .map(|y| {
                (0..100)
                    .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            screen.contains("by \"paski\""),
            "the heading should name the identity it filtered to:\n{}",
            screen
        );
        // The footer writes its keys bare: `unbracket` turns the `[A]`
        // the match arm is written with into `A ` — bracket gone, a
        // space in its place — and the label brings its own leading
        // space, so two land between them on the screen.
        assert!(
            screen.contains("A  authors"),
            "and the footer should offer the key that opens the menu:\n{}",
            screen
        );
    }

    /// The dropdown draws over the commit list with its own title and
    /// the clear row at the top. `render_author_menu` walks the same
    /// rows the key handler applies from, so what is drawn here is what
    /// Enter would act on.
    #[test]
    fn the_authors_menu_draws_over_the_log() {
        let mut app = App::new().expect("a repository to look at");
        app.go_to(View::Log);
        app.sidebar_focused = false;
        assert!(
            app.log_open_author_menu(),
            "this repository has commits to group"
        );

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render(f, &app)).expect("a frame");
        let buffer = terminal.backend().buffer().clone();
        let screen: String = (0..30)
            .map(|y| {
                (0..100)
                    .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            screen.contains("authors — Enter filter"),
            "the dropdown should name what Enter does:\n{}",
            screen
        );
        assert!(
            screen.contains("all authors"),
            "and carry the row that lifts the filter:\n{}",
            screen
        );
    }

    /// The events popup is the one overlay every view can raise, so it has to
    /// speak the window's language: a rule for a border, ink for the title,
    /// and keys in the accent — not the brand red used as a box colour.
    #[test]
    fn the_events_popup_wears_the_window_chrome() {
        let mut app = App::new().expect("a repository to look at");
        app.log_event("something happened", EventKind::Success);
        app.show_event_log = true;

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();

        let mut border_colours = std::collections::HashSet::new();
        for y in 0..30 {
            for x in 0..100 {
                let cell = buffer.cell((x, y)).unwrap();
                if matches!(
                    cell.symbol(),
                    "│" | "─" | "╭" | "╮" | "╰" | "╯" | "┌" | "┐" | "└" | "┘"
                ) {
                    border_colours.insert(format!("{:?}", cell.fg));
                }
            }
        }
        assert_eq!(
            border_colours,
            std::collections::HashSet::from([format!("{:?}", theme::RULE)]),
            "every line on screen, popup included, is drawn in the rule's colour"
        );
    }

    /// The ignore view over a repo that actually has rules — the version
    /// `look_at_it` cannot show, since the crate directory has no
    /// `.toriignore` of its own.
    #[test]
    fn look_at_the_safety_view() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".toriignore"),
            "build/\ntarget/\n*.log\n\n[secrets]\ndeny: AKIA[0-9A-Z]{16}  # AWS\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".toriignore.local"),
            "internal/billing/\n\n[secrets]\ndeny: xkeysib-[a-z0-9]{20,}  # Brevo\n",
        )
        .unwrap();

        let mut app = App::new().expect("a repository to look at");
        app.repo_path = tmp.path().to_string_lossy().into_owned();
        app.go_to(View::Safety);
        app.sidebar_focused = false;
        app.load_safety();
        // A private rule, so the pane has to name the file it came from.
        app.ignore_view.idx = app
            .ignore_view
            .rules
            .iter()
            .position(|r| r.origin == crate::ignore_rules::Origin::Local)
            .expect("a rule from the private file");

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let screen = dump(terminal.backend().buffer(), 100, 24);
        println!("{screen}");

        assert!(screen.contains("rules (6)"), "{screen}");
        assert!(screen.contains("local"), "the private rules must be marked");
        assert!(
            screen.contains(".toriignore.local"),
            "the detail names the file the rule lives in"
        );

        // The scanner tab: the machinery behind those rules.
        app.ignore_view.tab = crate::tui::app::SafetyTab::Scanner;
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let screen = dump(terminal.backend().buffer(), 100, 24);
        println!("{screen}");

        assert!(screen.contains("built in"), "{screen}");
        assert!(
            screen.contains("blocked") && screen.contains("--yes"),
            "the screen must say what a hit does, and how to override: {screen}"
        );
        assert!(
            screen.contains("block above") && screen.contains("before save"),
            "the settings are rows you can act on: {screen}"
        );
        assert!(
            screen.contains(&crate::scanner::builtin_pattern_count().to_string()),
            "the built-in count is shown: {screen}"
        );
    }

    /// The keys tab and the palette, drawn once so they can be reviewed.
    #[test]
    fn look_at_the_keys() {
        let mut app = App::new().expect("a repository to look at");
        app.keymap = crate::tui::keys::Keymap::from_text(
            "\"ctrl+g\" = \"goto:log\"\n\"g s\" = \"goto:sync\"\n\"alt+i\" = \"goto:ignore\"\n",
        );
        app.go_to(View::Config);
        app.sidebar_focused = false;
        app.config_view.tab = crate::tui::app::ConfigTab::Keys;
        app.config_view.keys_idx = 5;

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        println!("{}", dump(terminal.backend().buffer(), 100, 24));

        app.palette.open = true;
        app.palette.query = "sw".into();
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        println!("{}", dump(terminal.backend().buffer(), 100, 24));
    }

    fn dump(buffer: &ratatui::buffer::Buffer, w: u16, h: u16) -> String {
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The selected row and the caret must be the same colour family: the
    /// caret is the accent, so the row behind it is the accent washed down.
    #[test]
    fn the_selected_row_wears_the_accent() {
        let mut app = App::new().expect("a repository to look at");
        app.go_to(View::Log);
        app.sidebar_focused = false;

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();

        // Find the caret cell, then read the background beside it.
        let mut found = None;
        for y in 0..24u16 {
            for x in 0..100u16 {
                let cell = buffer.cell((x, y)).unwrap();
                if cell.symbol() == "›" && cell.fg == theme::accent(&app) {
                    found = Some(buffer.cell((x + 2, y)).unwrap().bg);
                }
            }
        }
        let bg = found.expect("a selected row somewhere on screen");
        match bg {
            ratatui::style::Color::Rgb(r, g, b) => {
                assert!(r > g && r > b, "the selection is red-ish: {r},{g},{b}");
            }
            other => panic!("expected an rgb background, got {other:?}"),
        }
    }

    /// The stats screen, both modes, over this very repo.
    #[test]
    fn look_at_the_stats() {
        let mut app = App::new().expect("a repository to look at");
        app.go_to(View::Stats);
        app.sidebar_focused = false;
        app.load_stats();
        // The worker would normally answer a moment later; wait for it so the
        // printed screen shows the finished thing.
        for _ in 0..200 {
            app.poll_stats_worker();
            if app.stats_view.churn.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        println!("{}", dump(terminal.backend().buffer(), 100, 28));

        app.stats_view.mode = crate::tui::app::StatsMode::People;
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        println!("{}", dump(terminal.backend().buffer(), 100, 20));

        app.stats_view.mode = crate::tui::app::StatsMode::Workspace;
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        println!("{}", dump(terminal.backend().buffer(), 100, 16));
    }

    /// The people screen holds names and addresses, and neither may be cut:
    /// half an address is not an address.
    #[test]
    fn the_people_screen_never_cuts_a_name_or_an_address() {
        for width in [100u16, 84, 70] {
            let mut app = App::new().expect("a repository to look at");
            app.go_to(View::Stats);
            app.sidebar_focused = false;
            app.stats_view.mode = crate::tui::app::StatsMode::People;
            app.stats_view.people = vec![crate::stats::Person {
                name: "Someone With A Very Long Name Indeed".into(),
                email: "someone.with.a.long.address@example.com".into(),
                other_emails: vec!["second.address@example.com".into()],
                commits: 12,
                signed: 12,
                sig_kinds: vec![crate::stats::SigKind::Pgp],
                first: Some(1_700_000_000),
                last: Some(1_700_000_000),
                ..Default::default()
            }];

            let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
            terminal.draw(|f| render(f, &app)).unwrap();
            let screen = dump(terminal.backend().buffer(), width, 30);

            // A wrapped value spans two lines, so the pane is read as a
            // column and glued back together: what must survive is the text,
            // not the line it happens to sit on.
            // Box characters are multi-byte, so the column is counted in
            // characters, not bytes.
            let column = screen
                .lines()
                .find_map(|l| {
                    l.chars()
                        .collect::<Vec<_>>()
                        .windows(8)
                        .position(|w| w.iter().collect::<String>() == "identity")
                })
                .expect("the identity pane");
            let pane: String = screen
                .lines()
                .map(|l| l.chars().skip(column).collect::<String>())
                .map(|l| l.trim_end_matches(['│', ' ']).trim_end().to_string())
                .collect::<Vec<_>>()
                .join("")
                .split_whitespace()
                .collect();

            assert!(
                !screen.contains('…'),
                "at {width} something was cut short:\n{screen}"
            );
            for whole in [
                "SomeoneWithAVeryLongNameIndeed",
                "someone.with.a.long.address@example.com",
                "second.address@example.com",
            ] {
                assert!(
                    pane.contains(whole),
                    "at {width} `{whole}` did not survive:\n{screen}"
                );
            }
            assert!(
                screen.contains("pgp"),
                "the signature format shows: {screen}"
            );
        }
    }

    /// The pointer resolves against what was actually drawn: render a frame,
    /// then ask the map what is under a cell.
    #[test]
    fn a_click_on_the_sidebar_finds_the_row_it_names() {
        let mut app = App::new().expect("a repository to look at");
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();

        // The third row of the sidebar, inside its column.
        let zone = app.hits.borrow().at(3, 5).cloned();
        let Some(crate::tui::hit::Zone::Sidebar(index)) = zone else {
            panic!("expected a sidebar row, got {zone:?}");
        };
        // Row 0 is the first tab drawn, and the header sits above it.
        assert!(index < TABS.len());

        // And clicking it goes to the view that row names.
        app.sidebar_idx = index;
        app.sidebar_enter();
        assert_eq!(app.view, TABS[index].view);
    }

    /// The keys on the foot of the window are buttons: the accent is only
    /// used there for keys, which is what makes them findable.
    #[test]
    fn the_hint_keys_are_clickable() {
        let app = App::new().expect("a repository to look at");
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();

        let hits = app.hits.borrow();
        let mut found = Vec::new();
        for x in 0..100u16 {
            if let Some(crate::tui::hit::Zone::Key(key)) = hits.at(x, 28) {
                if !found.contains(key) {
                    found.push(key.clone());
                }
            }
        }
        assert!(
            found.iter().any(|k| k == "e"),
            "the events key must be clickable: {found:?}"
        );
    }

    /// Rows are registered where they were drawn, so a click on a person in
    /// the stats screen resolves to that person.
    #[test]
    fn a_click_on_a_list_row_resolves_to_that_row() {
        let mut app = App::new().expect("a repository to look at");
        app.go_to(View::Stats);
        app.sidebar_focused = false;
        app.stats_view.mode = crate::tui::app::StatsMode::People;
        app.stats_view.people = (0..4)
            .map(|i| crate::stats::Person {
                name: format!("Person {i}"),
                email: format!("p{i}@example.com"),
                commits: 10 - i,
                ..Default::default()
            })
            .collect();

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();

        // Find the row zone for the third person and check the index matches.
        let hits = app.hits.borrow();
        let mut seen = None;
        for y in 0..24u16 {
            if let Some(crate::tui::hit::Zone::Row { list, index }) = hits.at(20, y) {
                if list == "stats.people" && *index == 2 {
                    seen = Some(y);
                }
            }
        }
        assert!(seen.is_some(), "the third person must have a row on screen");
    }

    /// The pointer switch says what it costs, and turning it off is a
    /// keypress away — a captured pointer takes the terminal's own text
    /// selection with it.
    #[test]
    fn the_tui_tab_carries_the_mouse_switch() {
        let mut app = App::new().expect("a repository to look at");
        app.go_to(View::Config);
        app.sidebar_focused = false;
        app.config_view.tab = crate::tui::app::ConfigTab::Tui;

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let screen = dump(terminal.backend().buffer(), 100, 24);

        assert!(screen.contains("mouse"), "{screen}");
        assert!(
            screen.contains("text selection"),
            "the cost is stated where the switch is: {screen}"
        );

        // Flipping it reports the change so the loop can tell the terminal.
        // The toggle persists, so the real settings file is put back: a test
        // has no business editing the machine it runs on.
        let path = dirs::home_dir().map(|h| h.join(".torii/tui-settings.toml"));
        let saved = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok());

        let before = app.settings.mouse;
        app.config_view.tui_idx = 0;
        assert_eq!(app.tui_setting_toggle(), Some(!before));
        assert_eq!(app.settings.mouse, !before);

        if let Some(path) = path {
            match saved {
                Some(text) => std::fs::write(&path, text).unwrap(),
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    /// Every view with a list has to answer a click on its rows. A view that
    /// forgets to register loses the pointer silently, which is exactly the
    /// kind of gap nobody notices until they try it.
    #[test]
    fn every_list_view_registers_its_rows() {
        // view, the list name it must register, and what has to be loaded for
        // there to be anything to click on.
        let cases: &[(View, &str)] = &[
            (View::Log, "log"),
            (View::Branch, "branch"),
            (View::Tag, "tag"),
            (View::Auth, "auth"),
            (View::Config, "config"),
            (View::Safety, "safety.rules"),
        ];

        for (view, list) in cases {
            let mut app = App::new().expect("a repository to look at");
            app.go_to(view.clone());
            app.sidebar_focused = false;
            if *view == View::Safety {
                // The rules list is empty in a repo with no .toriignore, and
                // an empty list has nothing to register.
                app.ignore_view.rules = vec![crate::ignore_rules::Rule {
                    origin: crate::ignore_rules::Origin::Public,
                    kind: crate::ignore_rules::Kind::Path,
                    line_no: 1,
                    raw: "build/".into(),
                    pattern: "build/".into(),
                    name: None,
                }];
            }

            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal.draw(|f| render(f, &app)).unwrap();

            let hits = app.hits.borrow();
            let mut found = false;
            for y in 0..40u16 {
                for x in 0..120u16 {
                    if let Some(crate::tui::hit::Zone::Row { list: name, .. }) = hits.at(x, y) {
                        if name == list {
                            found = true;
                        }
                    }
                }
            }
            assert!(found, "{view:?} draws rows but registers none for `{list}`");
        }
    }

    /// A row's index is the item's, not the screen line's: the branch view
    /// draws group headers between them.
    #[test]
    fn a_group_header_does_not_shift_the_rows_under_it() {
        let mut app = App::new().expect("a repository to look at");
        app.go_to(View::Branch);
        app.sidebar_focused = false;

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();

        // The first branch row on screen must be branch 0, even though a
        // header was drawn above it.
        let hits = app.hits.borrow();
        let mut first_row = None;
        for y in 0..40u16 {
            if let Some(crate::tui::hit::Zone::Row { list, index }) = hits.at(20, y) {
                if list == "branch" && first_row.is_none() {
                    first_row = Some(*index);
                }
            }
        }
        assert_eq!(first_row, Some(0), "the first branch row is branch 0");
    }

    /// Every list a pointer is meant to reach has to register its rows, or a
    /// click there does nothing and the view looks broken. This walks the
    /// views that have one and asks the map what it registered.
    #[test]
    fn the_lists_register_the_rows_a_click_can_reach() {
        // (view, the list name it registers, what to set up first)
        let cases: &[(View, &str)] = &[
            (View::Log, "log"),
            (View::Branch, "branch"),
            (View::Tag, "tag"),
            (View::Auth, "auth"),
            (View::Config, "config"),
            (View::Safety, "safety.rules"),
        ];

        for (view, list) in cases {
            let mut app = App::new().expect("a repository to look at");
            app.go_to(view.clone());
            app.sidebar_focused = false;
            if *view == View::Safety {
                // The rules list is empty in a repo with no .toriignore, and
                // an empty list has nothing to register.
                app.ignore_view.rules = vec![crate::ignore_rules::Rule {
                    origin: crate::ignore_rules::Origin::Public,
                    kind: crate::ignore_rules::Kind::Path,
                    line_no: 1,
                    raw: "build/".into(),
                    pattern: "build/".into(),
                    name: None,
                }];
            }

            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal.draw(|f| render(f, &app)).unwrap();

            let hits = app.hits.borrow();
            let mut found = false;
            for y in 0..40u16 {
                for x in 0..120u16 {
                    if let Some(crate::tui::hit::Zone::Row { list: name, .. }) = hits.at(x, y) {
                        if name == list {
                            found = true;
                        }
                    }
                }
            }
            assert!(found, "`{list}` registers no rows for a click");
        }
    }

    /// Nothing in the keys screen may be cut: the action id is what has to be
    /// typed into `keys.toml`, and half an id is worse than none. Narrow
    /// terminals get a second line, not an ellipsis.
    ///
    /// A row below the fold is fine — that is scrolling. What must never
    /// appear is a *partial* id, so every id-shaped token on screen has to be
    /// one the catalogue knows.
    #[test]
    fn no_action_id_is_ever_clipped() {
        let known: Vec<&str> = crate::tui::keys::ACTIONS.iter().map(|a| a.id).collect();

        for width in [100u16, 84, 72, 60] {
            let mut app = App::new().expect("a repository to look at");
            app.go_to(View::Config);
            app.sidebar_focused = false;
            app.config_view.tab = crate::tui::app::ConfigTab::Keys;

            let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
            terminal.draw(|f| render(f, &app)).unwrap();
            let screen = dump(terminal.backend().buffer(), width, 40);

            for token in screen
                .split(|c: char| c.is_whitespace() || c == '│' || c == '┤')
                .filter(|t| {
                    t.contains(':')
                        && t.chars()
                            .all(|c| c.is_ascii_lowercase() || ":-".contains(c))
                })
            {
                assert!(
                    known.contains(&token),
                    "at {width} columns `{token}` is a cut id:\n{screen}"
                );
            }
            // And at least the first rows must actually be there.
            assert!(screen.contains("goto:files"), "at {width}:\n{screen}");
        }
    }

    /// The same for the palette, whose rows carry a hint on the right.
    #[test]
    fn the_palette_keeps_whole_labels() {
        let mut app = App::new().expect("a repository to look at");
        app.palette.open = true;
        app.palette.query = "switch".into();

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let screen = dump(terminal.backend().buffer(), 100, 24);

        assert!(screen.contains("Switch branch"), "{screen}");
        assert!(screen.contains("Switch workspace repo"), "{screen}");
        assert!(
            screen.contains("ctrl+o"),
            "the binding stays visible: {screen}"
        );
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
    const CONVERTED: [View; 19] = [
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
        View::Pr,
        View::Platform,
        View::Safety,
        View::Clone,
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

    /// The panel is 55 columns at its widest and the messages are
    /// sentences. On one `Line` ratatui clipped them: everything past
    /// the first few words was simply not on screen.
    #[test]
    fn event_messages_wrap_instead_of_being_clipped() {
        let lines = wrap_message(
            "reauthored author and committer on 3 picked commits",
            12,
            18,
        );

        assert!(lines.len() > 1, "still one line: {lines:?}");
        assert!(lines[0].chars().count() <= 12, "first line: {:?}", lines[0]);
        for line in &lines[1..] {
            assert!(line.chars().count() <= 18, "wrapped line: {line:?}");
        }
        assert_eq!(
            lines.join(" "),
            "reauthored author and committer on 3 picked commits",
            "wrapping lost or duplicated words"
        );
    }

    /// A hash, a path or an identity has no space to break at, and a word
    /// wider than the panel would clip exactly like before.
    #[test]
    fn a_word_wider_than_the_panel_is_broken() {
        let lines = wrap_message("44322bdc0ffee1234567890abcdef", 10, 10);

        assert!(lines.len() >= 3);
        for line in &lines {
            assert!(line.chars().count() <= 10, "unbroken: {line:?}");
        }
        assert_eq!(lines.concat(), "44322bdc0ffee1234567890abcdef");
    }

    /// Empty in, one empty line out — never an empty item that renders as
    /// nothing at all.
    #[test]
    fn an_empty_message_still_yields_a_line() {
        assert_eq!(wrap_message("", 10, 10), vec![String::new()]);
    }

    #[test]
    fn brackets_are_stripped_from_key_hints() {
        let lines = screen(100, 30);
        let foot = lines.last().unwrap();
        assert!(!foot.contains('['), "key hints still bracketed: {foot}");
    }
}
