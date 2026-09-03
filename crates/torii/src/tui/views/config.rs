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

use crate::tui::app::{App, ConfigScope, ConfigTab};
use crate::tui::keys;
use crate::tui::theme;

const SECTIONS: &[&str] = &["user", "auth", "git", "mirror", "snapshot", "ui"];

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    // Two tabs share this screen: the git values, and the key bindings.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);
    render_tab_strip(f, app, rows[0]);
    theme::hrule_content(f, rows[1], &[]);
    match app.config_view.tab {
        ConfigTab::Values => render_values(f, app, rows[2]),
        ConfigTab::Keys => render_keys(f, app, rows[2]),
        ConfigTab::Tui => render_tui(f, app, rows[2]),
    }
}

/// The two tabs, as words rather than a boxed widget.
fn render_tab_strip(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;
    let tab = app.config_view.tab;
    let style = |mine: ConfigTab| {
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
            Span::styled("1 values", style(ConfigTab::Values)),
            Span::styled(" · ", Style::default().fg(theme::RULE)),
            Span::styled("2 keys", style(ConfigTab::Keys)),
            Span::styled(" · ", Style::default().fg(theme::RULE)),
            Span::styled("3 tui", style(ConfigTab::Tui)),
        ])),
        area,
    );
    // The strip is clickable, and a tab is the digit printed on it.
    let mut hits = app.hits.borrow_mut();
    let mut x = area.x + 1;
    for (index, label) in ["1 values", "2 keys", "3 tui"].iter().enumerate() {
        let width = label.chars().count() as u16;
        hits.push(
            Rect::new(x, area.y, width, 1),
            crate::tui::hit::Zone::Tab {
                strip: "config".into(),
                index,
            },
        );
        x += width + 3;
    }
}

/// The TUI's own settings: the ones that live in `tui-settings.toml` rather
/// than in git config, which is what the values tab shows.
fn render_tui(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let focused = !app.sidebar_focused;
    let rows = app.tui_settings_rows();

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("tui", Some(rows.len()), focused));
    heading.push(Span::styled(
        "  Enter changes · saved on the spot",
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, setting)| {
            let is_sel = focused && i == app.config_view.tui_idx;
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(
                    format!("{:<10}", setting.label),
                    Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                ),
                Span::styled(
                    format!("{:<10}", setting.value),
                    Style::default().fg(theme::INK),
                ),
                Span::styled(setting.note, Style::default().fg(theme::INK_FAINT)),
            ]))
            .style(if is_sel {
                Style::default()
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.config_view.tui_idx));
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
    let first = state.offset();
    app.hits
        .borrow_mut()
        .rows(body, "config.tui", first, rows.len().saturating_sub(first));

    if let Some(status) = &app.config_view.status {
        let line = Rect::new(body.x, body.bottom().saturating_sub(1), body.width, 1);
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(" {status}"),
                Style::default().fg(theme::INK_FAINT),
            )),
            line,
        );
    }
}

/// The bindings: every action, and what it is on.
fn render_keys(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(1)])
        .split(area);

    // Left: the groups the catalogue declares, as a legend.
    let divider = theme::divider_right();
    let groups_pane = divider.inner(cols[0]);
    f.render_widget(divider, cols[0]);
    let spine = [cols[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    let [groups_heading, groups_body] = theme::heading_and_body(groups_pane);
    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("groups", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), groups_heading);

    let selected_group = keys::ACTIONS
        .get(app.config_view.keys_idx)
        .map(|a| a.group)
        .unwrap_or("");
    let mut seen: Vec<&str> = Vec::new();
    for a in keys::ACTIONS {
        if !seen.contains(&a.group) {
            seen.push(a.group);
        }
    }
    let group_items: Vec<ListItem> = seen
        .iter()
        .map(|g| {
            let is_current = *g == selected_group;
            ListItem::new(Line::from(vec![
                theme::caret(app, is_current),
                Span::styled(
                    *g,
                    Style::default().fg(if is_current {
                        theme::INK
                    } else {
                        theme::INK_DIM
                    }),
                ),
            ]))
        })
        .collect();
    f.render_widget(
        List::new(group_items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        groups_body,
    );

    // Right: action → binding.
    let [heading_row, body] = theme::heading_and_body(cols[1]);
    let focused = !app.sidebar_focused;
    let bound = app.keymap.bindings.len();

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("keys", Some(bound), focused));
    heading.push(Span::styled(
        format!("  palette {}", app.keymap.leader),
        Style::default().fg(theme::INK_FAINT),
    ));
    if let Some(action) = &app.config_view.capturing {
        heading.push(Span::styled(
            format!("  recording {action} — Esc cancels"),
            Style::default().fg(theme::accent(app)),
        ));
    }
    // A binding that can never fire is worth saying out loud, right where it
    // was made.
    if let Some(first) = app.keymap.conflicts().first() {
        heading.push(Span::styled(
            format!("  ⚠ {first}"),
            Style::default().fg(theme::WARN),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    // Columns are measured, not guessed: a label that outgrew a fixed width
    // used to push the id off the edge of the pane.
    let label_w = keys::ACTIONS
        .iter()
        .map(|a| a.label.chars().count())
        .max()
        .unwrap_or(20);
    let binding_w = keys::ACTIONS
        .iter()
        .filter_map(|a| app.keymap.binding_for(a.id))
        .map(|b| b.to_string().chars().count())
        .max()
        .unwrap_or(6)
        .max("press keys…".chars().count());
    // caret + label + gap + binding + gap + id
    let id_w = keys::ACTIONS
        .iter()
        .map(|a| a.id.chars().count())
        .max()
        .unwrap_or(10);
    let full_w = 2 + label_w + 2 + binding_w + 2 + id_w;
    let fits = (body.width as usize).saturating_sub(2) >= full_w;

    let items: Vec<ListItem> = keys::ACTIONS
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let is_sel = focused && i == app.config_view.keys_idx;
            let capturing = app.config_view.capturing.as_deref() == Some(action.id);
            let binding = if capturing {
                // Show the chords as they land, so a three-chord binding can
                // be seen taking shape.
                let so_far: Vec<String> = app
                    .config_view
                    .captured
                    .iter()
                    .map(|c| c.to_string())
                    .collect();
                if so_far.is_empty() {
                    "press keys…".to_string()
                } else {
                    format!("{}  ↵ to save", so_far.join(" "))
                }
            } else {
                app.keymap
                    .binding_for(action.id)
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "—".to_string())
            };
            let binding_colour = if capturing {
                theme::accent(app)
            } else if app.keymap.binding_for(action.id).is_some() {
                theme::INK
            } else {
                theme::INK_FAINT
            };
            let label_style = Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM });
            let lines = if fits {
                vec![Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(format!("{:<label_w$}", action.label), label_style),
                    Span::styled(
                        format!("  {binding:<binding_w$}"),
                        Style::default().fg(binding_colour),
                    ),
                    Span::styled(
                        format!("  {}", action.id),
                        Style::default().fg(theme::INK_FAINT),
                    ),
                ])]
            } else {
                // Too narrow for one row: the id moves under the label rather
                // than being cut off — the id is the thing that has to be
                // typed into keys.toml, so it is the last thing to lose.
                vec![
                    Line::from(vec![
                        theme::caret(app, is_sel),
                        Span::styled(action.label, label_style),
                        Span::styled(format!("  {binding}"), Style::default().fg(binding_colour)),
                    ]),
                    Line::from(vec![
                        Span::raw("    "),
                        Span::styled(action.id, Style::default().fg(theme::INK_FAINT)),
                    ]),
                ]
            };
            ListItem::new(lines).style(if is_sel {
                Style::default()
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.config_view.keys_idx));
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
    let first = state.offset();
    app.hits.borrow_mut().rows(
        body,
        "config.keys",
        first,
        keys::ACTIONS.len().saturating_sub(first),
    );
}

fn render_values(f: &mut Frame, app: &App, area: Rect) {
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
    app.hits.borrow_mut().rows(
        body,
        "config",
        state.offset(),
        app.config_view.entries.len().saturating_sub(state.offset()),
    );
}

// `render_status` was removed in 0.7.5; hints moved to the global hint
// bar in `ui.rs::render_hint` (where every other view's legend lives).
// Removed altogether rather than #[allow(dead_code)] because it would
// become a maintenance trap — drifting copy-paste of the hint strings
// that already exist elsewhere.
