//! The safety view: what keeps a secret out of this repo.
//!
//! Two tabs. **Rules** is the `.toriignore` pair, editable, with each rule
//! showing which of the two files it lives in — the difference between a rule
//! everyone with the repo can read and one that never leaves this machine.
//! **Scanner** is what reads those rules: the built-in patterns, the size
//! gate, and the hooks that run around a save.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

use crate::ignore_rules::{Kind, Origin};
use crate::tui::app::{App, IgnoreFocus, SafetyTab};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    render_tab_strip(f, app, rows[0]);
    match app.ignore_view.tab {
        SafetyTab::Rules => {
            render_rules_tab(f, app, rows[1], rows[2]);
        }
        SafetyTab::Scanner => {
            theme::hrule_content(f, rows[1], &[]);
            render_scanner(f, app, rows[2]);
        }
    }

    match app.ignore_view.focus {
        IgnoreFocus::Input => render_input(f, app, area),
        IgnoreFocus::SettingInput => render_setting_input(f, app, area),
        IgnoreFocus::ConfirmDelete => render_confirm(f, app, area),
        IgnoreFocus::List => {}
    }
}

fn render_tab_strip(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;
    let tab = app.ignore_view.tab;
    let style = |mine: SafetyTab| {
        if tab == mine && focused {
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD)
        } else if tab == mine {
            Style::default()
                .fg(theme::INK_DIM)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::INK_FAINT)
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled("1 rules", style(SafetyTab::Rules)),
            Span::styled(" · ", Style::default().fg(theme::RULE)),
            Span::styled("2 scanner", style(SafetyTab::Scanner)),
        ])),
        area,
    );
}

fn render_rules_tab(f: &mut Frame, app: &App, rule_row: Rect, area: Rect) {
    let focused = !app.sidebar_focused;

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    let divider = theme::divider_right();
    let list_pane = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = panes[0].right().saturating_sub(1);
    theme::hrule_content(f, rule_row, &[(spine, theme::Tick::Down)]);
    theme::tie_below(f, area, &[spine]);

    let [list_heading, list_body] = theme::heading_and_body(list_pane);
    let [detail_heading, detail_body] = theme::heading_and_body(panes[1]);

    render_list(f, app, list_heading, list_body, focused);
    render_detail(f, app, detail_heading, detail_body);
}

fn render_list(f: &mut Frame, app: &App, heading_row: Rect, body: Rect, focused: bool) {
    let iv = &app.ignore_view;
    let active = focused && iv.focus == IgnoreFocus::List;

    let public = iv
        .rules
        .iter()
        .filter(|r| r.origin == Origin::Public)
        .count();
    let local = iv.rules.len() - public;

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("rules", Some(iv.rules.len()), active));
    heading.push(Span::styled(
        format!("  {public} public  {local} local"),
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let items: Vec<ListItem> = if iv.rules.is_empty() {
        vec![ListItem::new(Span::styled(
            "no rules — press a to add one",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        let mut items = Vec::new();
        let mut last_kind: Option<Kind> = None;
        for (i, rule) in iv.rules.iter().enumerate() {
            // A group header whenever the kind changes, so paths and secret
            // patterns never read as one list.
            if last_kind != Some(rule.kind) {
                items.push(ListItem::new(Line::from(Span::styled(
                    rule.kind.label().to_string(),
                    Style::default()
                        .fg(theme::INK_FAINT)
                        .add_modifier(Modifier::BOLD),
                ))));
                last_kind = Some(rule.kind);
            }
            let is_sel = active && i == iv.idx;
            items.push(
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        truncate(&rule.pattern, 34),
                        Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                    ),
                    Span::styled(
                        format!("  {}", rule.origin.label()),
                        Style::default().fg(origin_colour(rule.origin)),
                    ),
                ]))
                .style(if is_sel {
                    Style::default()
                        .bg(theme::selection(app))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
            );
        }
        items
    };

    // The list carries group headers, so the selected row is not at `idx`.
    let mut state = ListState::default();
    if !iv.rules.is_empty() {
        state.select(Some(row_of(app, iv.idx)));
    }
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
}

/// What the scanner is made of, and what it enforces.
///
/// The left column is the machinery — built-in patterns and the user's own
/// deny rules — and the right one is everything else the same two files
/// control: the size gate and the hooks that run around a save. Nothing here
/// is decoration: every line changes whether a `torii save` goes through.
fn render_scanner(f: &mut Frame, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(area);

    let divider = theme::divider_right();
    let left = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = panes[0].right().saturating_sub(1);
    theme::tie_above(f, area, &[spine]);
    theme::tie_below(f, area, &[spine]);

    render_patterns(f, app, left);
    render_enforcement(f, app, panes[1]);
}

fn render_patterns(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let builtin = crate::scanner::builtin_pattern_names();
    let own: Vec<&crate::ignore_rules::Rule> = app
        .ignore_view
        .rules
        .iter()
        .filter(|r| r.kind == Kind::Secret)
        .collect();

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        "patterns",
        Some(builtin.len() + own.len()),
        !app.sidebar_focused,
    ));
    heading.push(Span::styled(
        format!("  {} built in · {} yours", builtin.len(), own.len()),
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let mut items: Vec<ListItem> = Vec::new();
    // The user's own rules first: they are the ones that can be wrong, and
    // the ones the rules tab can change.
    if !own.is_empty() {
        items.push(group("yours — edit them in the rules tab"));
        for rule in &own {
            items.push(ListItem::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    rule.name.clone().unwrap_or_else(|| rule.pattern.clone()),
                    Style::default().fg(theme::INK),
                ),
                Span::styled(
                    format!("  {}", rule.origin.label()),
                    Style::default().fg(if rule.origin == Origin::Local {
                        theme::WARN
                    } else {
                        theme::INK_FAINT
                    }),
                ),
            ])));
        }
    }
    items.push(group("built in — always on"));
    let room = (body.width as usize).saturating_sub(4);
    for name in builtin {
        items.push(ListItem::new(
            theme::wrap(name, room.max(8))
                .into_iter()
                .map(|part| {
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(part, Style::default().fg(theme::INK_DIM)),
                    ])
                })
                .collect::<Vec<_>>(),
        ));
    }

    let mut state = ListState::default();
    state.select(Some(app.ignore_view.scanner_idx));
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
}

fn render_enforcement(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let iv = &app.ignore_view;
    let active = !app.sidebar_focused && iv.tab == SafetyTab::Scanner;

    let set = iv.settings.iter().filter(|s| s.value.is_some()).count();
    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("enforcement", Some(set), active));
    heading.push(Span::styled(
        "  Enter sets · d unsets",
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let room = (body.width as usize).saturating_sub(16);
    let mut items: Vec<ListItem> = Vec::new();
    let mut last_section = "";
    for (i, setting) in iv.settings.iter().enumerate() {
        if setting.section != last_section {
            items.push(group(match setting.section {
                "size" => "size gate — what a save refuses to carry",
                _ => "hooks — what runs around a save",
            }));
            last_section = setting.section;
        }
        let is_sel = active && i == iv.scanner_idx;
        let (value, value_style) = match &setting.value {
            Some(v) => (v.clone(), Style::default().fg(theme::INK)),
            None => ("not set".to_string(), Style::default().fg(theme::INK_FAINT)),
        };
        // The file a setting lives in matters as much as its value: a hook
        // naming internal tooling belongs in the private file.
        let origin = setting
            .origin
            .map(|o| format!("  {}", o.label()))
            .unwrap_or_default();

        let mut lines = vec![Line::from(vec![
            theme::caret(app, is_sel),
            Span::styled(
                format!("{:<12}", setting.label),
                Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
            ),
        ])];
        for (n, part) in theme::wrap(&value, room.max(8)).into_iter().enumerate() {
            if n == 0 {
                if let Some(line) = lines.last_mut() {
                    line.spans.push(Span::styled(part, value_style));
                    line.spans.push(Span::styled(
                        origin.clone(),
                        Style::default().fg(match setting.origin {
                            Some(Origin::Local) => theme::WARN,
                            _ => theme::INK_FAINT,
                        }),
                    ));
                }
            } else {
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(14)),
                    Span::styled(part, value_style),
                ]));
            }
        }

        items.push(ListItem::new(lines).style(if is_sel {
            Style::default()
                .bg(theme::selection(app))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        }));
    }

    // What the scanner does on a hit is not a setting — it is the contract,
    // and it belongs on screen so nobody is surprised by it.
    items.push(ListItem::new(Line::from("")));
    items.push(group("on a hit"));
    for line in theme::wrap(
        "the save is blocked and you are asked; --yes overrides",
        room.max(8) + 12,
    ) {
        items.push(ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(line, Style::default().fg(theme::INK_DIM)),
        ])));
    }

    let mut state = ListState::default();
    state.select(Some(iv.scanner_idx.min(items.len().saturating_sub(1))));
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
}

/// Typing the value of a setting, with the file it will land in on show.
fn render_setting_input(f: &mut Frame, app: &App, area: Rect) {
    let Some(setting) = app.safety_selected_setting() else {
        return;
    };
    let iv = &app.ignore_view;
    let w = 64u16.min(area.width.saturating_sub(4));
    let h = 5u16;
    let overlay = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, overlay);

    let cursor = iv.cursor.min(iv.input.len());
    let (before, after) = iv.input.split_at(cursor);
    let hint = match (setting.section, setting.key) {
        ("size", "exclude") => "a glob, e.g. *.bin",
        ("size", _) => "a size, e.g. 10MB",
        _ => "a shell command",
    };

    let body = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} → ", setting.label),
                Style::default().fg(theme::INK_FAINT),
            ),
            Span::styled(
                iv.new_origin.file_name(),
                Style::default()
                    .fg(match iv.new_origin {
                        Origin::Local => theme::WARN,
                        Origin::Public => theme::INK_FAINT,
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("   {hint}"), Style::default().fg(theme::INK_FAINT)),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(before.to_string(), Style::default().fg(theme::INK)),
            Span::styled("█", Style::default().fg(theme::accent(app))),
            Span::styled(after.to_string(), Style::default().fg(theme::INK)),
        ]),
        Line::from(match &iv.status {
            Some(status) => vec![
                Span::raw("  "),
                Span::styled(status.clone(), Style::default().fg(theme::BAD)),
            ],
            None => {
                let mut spans = vec![Span::raw(" ")];
                spans.extend(theme::key_hint(app, "^T", "target"));
                spans.extend(theme::key_hint(app, "Enter", "save"));
                spans.extend(theme::key_hint(app, "Esc", "cancel"));
                spans
            }
        }),
    ];

    f.render_widget(Paragraph::new(body).block(popup(app)), overlay);
}

/// A group header inside a list.
fn group(label: &str) -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled(
        label.to_string(),
        Style::default()
            .fg(theme::INK_FAINT)
            .add_modifier(Modifier::BOLD),
    )))
}

/// Where rule `idx` sits once the group headers are counted in.
fn row_of(app: &App, idx: usize) -> usize {
    let mut row = 0;
    let mut last_kind = None;
    for (i, rule) in app.ignore_view.rules.iter().enumerate() {
        if last_kind != Some(rule.kind) {
            row += 1;
            last_kind = Some(rule.kind);
        }
        if i == idx {
            return row;
        }
        row += 1;
    }
    row
}

fn render_detail(f: &mut Frame, app: &App, heading_row: Rect, body: Rect) {
    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("detail", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let iv = &app.ignore_view;
    let mut lines: Vec<Line> = match app.ignore_selected() {
        Some(rule) => vec![
            field(
                "file",
                Span::styled(
                    rule.origin.file_name().to_string(),
                    Style::default()
                        .fg(origin_colour(rule.origin))
                        .add_modifier(Modifier::BOLD),
                ),
            ),
            field(
                "scope",
                Span::styled(
                    match rule.origin {
                        Origin::Public => "committed, public",
                        Origin::Local => "private, not committed",
                    },
                    Style::default().fg(theme::INK_FAINT),
                ),
            ),
            field(
                "kind",
                Span::styled(rule.kind.label(), Style::default().fg(theme::INK_DIM)),
            ),
            field(
                "line",
                Span::styled(
                    rule.line_no.to_string(),
                    Style::default().fg(theme::INK_FAINT),
                ),
            ),
            Line::from(""),
            field(
                "pattern",
                Span::styled(rule.pattern.clone(), Style::default().fg(theme::INK)),
            ),
        ],
        None => vec![Line::from(Span::styled(
            "no rule selected",
            Style::default().fg(theme::INK_FAINT),
        ))],
    };

    if let Some(rule) = app.ignore_selected() {
        if let Some(name) = &rule.name {
            lines.push(field(
                "name",
                Span::styled(name.clone(), Style::default().fg(theme::INK_DIM)),
            ));
        }
    }

    // What the next `a` would write, so the target is known before typing.
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("new rule ", Style::default().fg(theme::INK_FAINT)),
        Span::styled(
            iv.new_kind.label(),
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" → ", Style::default().fg(theme::INK_FAINT)),
        Span::styled(
            iv.new_origin.file_name(),
            Style::default().fg(origin_colour(iv.new_origin)),
        ),
    ]));

    if let Some(status) = &iv.status {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            status.clone(),
            Style::default().fg(theme::BAD),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
    );
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let iv = &app.ignore_view;
    let w = 64u16.min(area.width.saturating_sub(4));
    let h = 5u16;
    let overlay = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, overlay);

    let msg = &iv.input;
    let cursor = iv.cursor.min(msg.len());
    let (before, after) = msg.split_at(cursor);

    let body = vec![
        Line::from(vec![
            Span::styled(
                format!(" new {} rule → ", iv.new_kind.label()),
                Style::default().fg(theme::INK_FAINT),
            ),
            Span::styled(
                iv.new_origin.file_name(),
                Style::default()
                    .fg(origin_colour(iv.new_origin))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(before.to_string(), Style::default().fg(theme::INK)),
            Span::styled("█", Style::default().fg(theme::accent(app))),
            Span::styled(after.to_string(), Style::default().fg(theme::INK)),
        ]),
        Line::from({
            let mut spans = vec![Span::raw(" ")];
            spans.extend(theme::key_hint(app, "Tab", "kind"));
            spans.extend(theme::key_hint(app, "^T", "target"));
            spans.extend(theme::key_hint(app, "Enter", "add"));
            spans.extend(theme::key_hint(app, "Esc", "cancel"));
            spans
        }),
    ];

    f.render_widget(Paragraph::new(body).block(popup(app)), overlay);
}

fn render_confirm(f: &mut Frame, app: &App, area: Rect) {
    let Some(rule) = app.ignore_selected() else {
        return;
    };
    let w = 62u16.min(area.width.saturating_sub(4));
    let h = 5u16;
    let overlay = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, overlay);

    let body = vec![
        Line::from(vec![
            Span::styled("  remove ", Style::default().fg(theme::INK)),
            Span::styled(
                truncate(&rule.pattern, 30),
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" from {}?", rule.origin.file_name()),
                Style::default().fg(theme::INK),
            ),
        ]),
        // Removing a rule loosens the scanner: say so, do not just ask.
        Line::from(Span::styled(
            match rule.kind {
                Kind::Secret => "  the scanner will stop allowing what it matched",
                _ => "  the path will be scanned and committed again",
            },
            Style::default().fg(theme::INK_FAINT),
        )),
        Line::from({
            let mut spans = vec![Span::raw(" ")];
            spans.extend(theme::key_hint(app, "y", "remove"));
            spans.extend(theme::key_hint(app, "n", "keep"));
            spans
        }),
    ];

    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::BAD)),
        ),
        overlay,
    );
}

/// A popup keeps its box: it is a window, not a column.
fn popup(app: &App) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type())
        .border_style(Style::default().fg(theme::RULE))
}

/// Private rules are the ones worth spotting at a glance.
fn origin_colour(origin: Origin) -> ratatui::style::Color {
    match origin {
        Origin::Public => theme::INK_FAINT,
        Origin::Local => theme::WARN,
    }
}

fn field(label: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<9}", label),
            Style::default().fg(theme::INK_FAINT),
        ),
        value,
    ])
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}
