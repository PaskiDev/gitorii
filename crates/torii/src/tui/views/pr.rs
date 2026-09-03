use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{App, PrConfirm, PrStateFilter};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let focused = !app.sidebar_focused;
    let pr = &app.pr_view;

    // list (60%) | detail (40%)
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    // The overlays below anchor on the list column, borders and all.
    let cols = panes.clone();

    // The list carries the rule; the detail sits the other side of it.
    let divider = theme::divider_right();
    let list_pane = divider.inner(panes[0]);
    f.render_widget(divider, panes[0]);
    let spine = [panes[0].right().saturating_sub(1)];
    theme::tie_above(f, area, &spine);
    theme::tie_below(f, area, &spine);

    let [list_heading, list_body] = theme::heading_and_body(list_pane);
    let [detail_heading, detail_body] = theme::heading_and_body(panes[1]);

    // ── The list ──────────────────────────────────────────────────────────────
    let items: Vec<ListItem> = if pr.loading {
        vec![ListItem::new(Span::styled(
            "loading…",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else if let Some(err) = &pr.error {
        let is_token = err.to_lowercase().contains("token");
        let mut err_items = vec![ListItem::new(Line::from(vec![
            Span::styled("✗ ", Style::default().fg(theme::BAD)),
            Span::styled(
                if is_token {
                    "authentication required"
                } else {
                    "error"
                },
                Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
            ),
        ]))];
        for chunk in err.chars().collect::<Vec<_>>().chunks(50) {
            let s: String = chunk.iter().collect();
            err_items.push(ListItem::new(Span::styled(
                s,
                Style::default().fg(theme::INK_FAINT),
            )));
        }
        err_items
    } else if pr.prs.is_empty() {
        vec![ListItem::new(Span::styled(
            "no pull requests",
            Style::default().fg(theme::INK_FAINT),
        ))]
    } else {
        pr.prs
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let is_sel = focused && i == pr.idx;
                ListItem::new(Line::from(vec![
                    theme::caret(app, is_sel),
                    Span::styled(
                        format!("#{}", p.number),
                        Style::default().fg(state_color(&p.state)),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        p.title.clone(),
                        Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                    ),
                    Span::styled(
                        if p.draft { " draft" } else { "" },
                        Style::default().fg(theme::INK_FAINT),
                    ),
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

    let mut list_state = ListState::default();
    if !pr.prs.is_empty() {
        list_state.select(Some(pr.idx));
    }

    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(
        if pr.platform == "gitlab" {
            "merge requests"
        } else {
            "pull requests"
        },
        if pr.loading { None } else { Some(pr.prs.len()) },
        focused,
    ));
    heading.push(Span::styled(
        match pr.filter {
            PrStateFilter::Open => "  open",
            PrStateFilter::Closed => "  closed",
            PrStateFilter::All => "  all",
        },
        Style::default().fg(theme::INK_FAINT),
    ));
    f.render_widget(Paragraph::new(Line::from(heading)), list_heading);
    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        list_body,
        &mut list_state,
    );
    // Rows the pointer can address, mapped from where the list actually
    // scrolled to.
    app.hits.borrow_mut().rows(
        list_body,
        "pr",
        list_state.offset(),
        pr.prs.len().saturating_sub(list_state.offset()),
    );

    // ── The detail ────────────────────────────────────────────────────────────
    let mut detail_title = vec![Span::raw(" ")];
    detail_title.extend(theme::panel_title("detail", None, false));
    f.render_widget(Paragraph::new(Line::from(detail_title)), detail_heading);

    let detail_lines: Vec<Line> = match pr.prs.get(pr.idx) {
        Some(p) => {
            let sc = state_color(&p.state);
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("#", Style::default().fg(theme::INK_FAINT)),
                    Span::styled(
                        p.number.to_string(),
                        Style::default().fg(sc).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(p.state.clone(), Style::default().fg(sc)),
                    if p.draft {
                        Span::styled("  draft", Style::default().fg(theme::INK_FAINT))
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    p.title.clone(),
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("by  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled(p.author.clone(), Style::default().fg(theme::INK_DIM)),
                ]),
                Line::from(vec![
                    Span::styled(p.head.clone(), Style::default().fg(theme::WARN)),
                    Span::styled(" → ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled(p.base.clone(), Style::default().fg(theme::INK_DIM)),
                ]),
                Line::from(match p.mergeable {
                    Some(true) => Span::styled("✓ mergeable", Style::default().fg(theme::OK)),
                    Some(false) => Span::styled("✗ conflicts", Style::default().fg(theme::BAD)),
                    None => Span::styled("~ unknown", Style::default().fg(theme::INK_FAINT)),
                }),
                Line::from(vec![
                    Span::styled("created  ", Style::default().fg(theme::INK_FAINT)),
                    Span::styled(p.created_at.clone(), Style::default().fg(theme::INK_DIM)),
                ]),
            ];

            if let Some(body) = &p.body {
                if !body.trim().is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "description",
                        Style::default().fg(theme::INK_FAINT),
                    )));
                    for l in body.lines().take(12) {
                        lines.push(Line::from(Span::styled(
                            l.to_string(),
                            Style::default().fg(theme::INK_DIM),
                        )));
                    }
                }
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "nothing selected",
            Style::default().fg(theme::INK_FAINT),
        ))],
    };
    f.render_widget(
        Paragraph::new(detail_lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        detail_body,
    );

    // ── Confirm overlay ───────────────────────────────────────────────────────
    if pr.confirm == PrConfirm::Close {
        let overlay = Rect::new(
            cols[0].x + 2,
            cols[0].y + 2 + pr.idx.min(cols[0].height as usize - 5) as u16,
            28,
            3,
        );
        f.render_widget(Clear, overlay);
        let p = Paragraph::new(Line::from(vec![
            Span::styled("  close PR? ", Style::default().fg(theme::BAD)),
            Span::styled(
                "[y]",
                Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" / any", Style::default().fg(theme::INK_DIM)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::BAD))
                .border_type(app.border_type()),
        );
        f.render_widget(p, overlay);
    }

    if pr.confirm == PrConfirm::Merge {
        let methods = ["merge", "squash", "rebase"];
        let head_branch = pr.prs.get(pr.idx).map(|p| p.head.as_str()).unwrap_or("?");
        let overlay = Rect::new(
            cols[0].x + 2,
            cols[0].y + 2 + pr.idx.min(cols[0].height as usize - 8) as u16,
            34,
            6,
        );
        f.render_widget(Clear, overlay);
        let method_spans: Vec<Span> = methods
            .iter()
            .enumerate()
            .map(|(i, m)| {
                if i == pr.merge_method {
                    Span::styled(
                        format!(" [{}] ", m),
                        Style::default()
                            .fg(theme::INK)
                            .add_modifier(Modifier::BOLD)
                            .bg(theme::selection(app)),
                    )
                } else {
                    Span::styled(format!("  {}  ", m), Style::default().fg(theme::INK_DIM))
                }
            })
            .collect();
        let lines = vec![
            Line::from(vec![Span::styled(
                "  merge method:",
                Style::default().fg(theme::INK_DIM),
            )]),
            Line::from(method_spans),
            Line::from(vec![
                Span::styled("  branch '", Style::default().fg(theme::INK_DIM)),
                Span::styled(head_branch.to_string(), Style::default().fg(theme::WARN)),
                Span::styled("' will be deleted", Style::default().fg(theme::INK_DIM)),
            ]),
            Line::from(vec![
                Span::styled("  [←→]", Style::default().fg(theme::accent(app))),
                Span::styled(" select  ", Style::default().fg(theme::INK_DIM)),
                Span::styled("[Enter]", Style::default().fg(theme::accent(app))),
                Span::styled(" confirm", Style::default().fg(theme::INK_DIM)),
            ]),
        ];
        let p = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::RULE))
                .border_type(app.border_type()),
        );
        f.render_widget(p, overlay);
    }

    // ── Create overlays ───────────────────────────────────────────────────────
    let pr_label = if pr.platform == "gitlab" { "MR" } else { "PR" };
    if pr.confirm == PrConfirm::CreateTitle {
        const TITLE_MAX: usize = 255;
        let ow = (area.width.saturating_sub(6)).clamp(52, 80);
        let oh = 6u16;
        let ox = area.x + area.width.saturating_sub(ow) / 2;
        let oy = area.y + area.height.saturating_sub(oh) / 2;
        let overlay = Rect::new(ox, oy, ow, oh);
        let len = pr.create_input.chars().count();
        let inner_w = (ow as usize).saturating_sub(4);
        // show last inner_w chars so cursor is always visible
        let display: String = pr
            .create_input
            .chars()
            .rev()
            .take(inner_w.saturating_sub(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let at_limit = len >= TITLE_MAX;
        let counter_color = if len > 230 {
            theme::BAD
        } else {
            theme::INK_FAINT
        };
        let lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("  create {} — step 1/5: ", pr_label),
                    Style::default().fg(theme::INK_DIM),
                ),
                Span::styled(
                    "title",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![Span::styled(
                format!("  {}█", display),
                Style::default().fg(theme::INK_DIM),
            )]),
            Line::from(vec![
                Span::styled(
                    format!("  {}/{}", len, TITLE_MAX),
                    Style::default().fg(counter_color),
                ),
                if at_limit {
                    Span::styled("  limit reached", Style::default().fg(theme::BAD))
                } else {
                    Span::raw("")
                },
            ]),
            Line::from(vec![
                Span::styled("  [Enter]", Style::default().fg(theme::accent(app))),
                Span::styled(" next  ", Style::default().fg(theme::INK_DIM)),
                Span::styled("[Esc]", Style::default().fg(theme::accent(app))),
                Span::styled(" cancel", Style::default().fg(theme::INK_DIM)),
            ]),
        ];
        f.render_widget(Clear, overlay);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::RULE))
                    .border_type(app.border_type()),
            ),
            overlay,
        );
    }

    if pr.confirm == PrConfirm::CreateHead {
        let dw = 32u16;
        let dh = (pr.branches.len().min(10) + 2) as u16;
        let ox = area.x + area.width.saturating_sub(dw) / 2;
        let oy = area.y + area.height.saturating_sub(dh) / 2;
        let drop_area = Rect::new(ox, oy, dw, dh);
        let drop_items: Vec<ListItem> = pr
            .branches
            .iter()
            .enumerate()
            .map(|(i, branch)| {
                let is_sel = i == pr.branch_idx;
                let color = if is_sel { theme::INK } else { theme::INK_DIM };
                let prefix = if is_sel { "▶ " } else { "  " };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::accent(app))),
                    Span::styled(branch.clone(), Style::default().fg(color)),
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
        let mut drop_state = ListState::default();
        drop_state.select(Some(pr.branch_idx));
        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(drop_items).block(
                Block::default()
                    .title(Span::styled(
                        format!(" create {} — step 2/5: source branch ", pr_label),
                        Style::default().fg(theme::INK_DIM),
                    ))
                    .borders(Borders::ALL)
                    .border_type(app.border_type())
                    .border_style(Style::default().fg(theme::RULE)),
            ),
            drop_area,
            &mut drop_state,
        );
    }

    if pr.confirm == PrConfirm::CreateBase {
        let dw = 32u16;
        let dh = (pr.branches.len().min(10) + 2) as u16;
        let ox = area.x + area.width.saturating_sub(dw) / 2;
        let oy = area.y + area.height.saturating_sub(dh) / 2;
        let drop_area = Rect::new(ox, oy, dw, dh);
        let drop_items: Vec<ListItem> = pr
            .branches
            .iter()
            .enumerate()
            .map(|(i, branch)| {
                let is_sel = i == pr.branch_idx;
                let color = if is_sel { theme::INK } else { theme::INK_DIM };
                let prefix = if is_sel { "▶ " } else { "  " };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::accent(app))),
                    Span::styled(branch.clone(), Style::default().fg(color)),
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
        let mut drop_state = ListState::default();
        drop_state.select(Some(pr.branch_idx));
        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(drop_items).block(
                Block::default()
                    .title(Span::styled(
                        format!(" create {} — step 3/5: base branch ", pr_label),
                        Style::default().fg(theme::INK_DIM),
                    ))
                    .borders(Borders::ALL)
                    .border_type(app.border_type())
                    .border_style(Style::default().fg(theme::RULE)),
            ),
            drop_area,
            &mut drop_state,
        );
    }

    if pr.confirm == PrConfirm::CreateDesc {
        let ow = 74u16;
        let oh = 14u16;
        // centre within the content area (excludes sidebar)
        let ox = area.x + area.width.saturating_sub(ow) / 2;
        let oy = area.y + area.height.saturating_sub(oh) / 2;
        let overlay = Rect::new(ox, oy, ow, oh);

        let draft_hint = if pr.create_draft {
            "  [draft ✓]"
        } else {
            "  [Tab] draft"
        };
        let mut lines = vec![Line::from(vec![
            Span::styled(
                format!("  create {} — step 4/5: ", pr_label),
                Style::default().fg(theme::INK_DIM),
            ),
            Span::styled(
                "description",
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" (optional)", Style::default().fg(theme::INK_FAINT)),
        ])];
        // accumulated lines
        for l in pr.create_desc.lines() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(l.to_string(), Style::default().fg(theme::INK_DIM)),
            ]));
        }
        // current input line with cursor
        lines.push(Line::from(vec![Span::styled(
            format!("  {}█", pr.create_input),
            Style::default().fg(theme::INK_DIM),
        )]));
        // fill remaining space — saturating_sub protects against tiny
        // overlay heights (oh < 3) that would otherwise underflow usize
        // and spin forever pushing empty lines.
        let target = (oh as usize).saturating_sub(3);
        while lines.len() < target {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(vec![
            Span::styled("  [Enter]", Style::default().fg(theme::accent(app))),
            Span::styled(" new line  ", Style::default().fg(theme::INK_DIM)),
            Span::styled("[^S]", Style::default().fg(theme::accent(app))),
            Span::styled(" create  ", Style::default().fg(theme::INK_DIM)),
            Span::styled("[Esc]", Style::default().fg(theme::accent(app))),
            Span::styled(" cancel  ", Style::default().fg(theme::INK_DIM)),
            Span::styled(draft_hint, Style::default().fg(theme::WARN)),
        ]));

        f.render_widget(Clear, overlay);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::RULE))
                    .border_type(app.border_type()),
            ),
            overlay,
        );
    }

    // ── Edit overlays ────────────────────────────────────────────────────────
    let edit_label = if pr.platform == "gitlab" { "MR" } else { "PR" };

    if pr.confirm == PrConfirm::EditTitle {
        let ow = 60u16;
        let oh = 5u16;
        let ox = area.x + area.width.saturating_sub(ow) / 2;
        let oy = area.y + area.height.saturating_sub(oh) / 2;
        let overlay = Rect::new(ox, oy, ow, oh);
        let lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("  edit {} — step 1/3: ", edit_label),
                    Style::default().fg(theme::INK_DIM),
                ),
                Span::styled(
                    "title",
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![Span::styled(
                format!("  {}█", pr.edit_input),
                Style::default().fg(theme::INK_DIM),
            )]),
            Line::from(vec![
                Span::styled("  [Enter]", Style::default().fg(theme::accent(app))),
                Span::styled(" next  ", Style::default().fg(theme::INK_DIM)),
                Span::styled("[Esc]", Style::default().fg(theme::accent(app))),
                Span::styled(" cancel", Style::default().fg(theme::INK_DIM)),
            ]),
        ];
        f.render_widget(Clear, overlay);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::RULE))
                    .border_type(app.border_type()),
            ),
            overlay,
        );
    }

    if pr.confirm == PrConfirm::EditDesc {
        let ow = 74u16;
        let oh = 14u16;
        let ox = area.x + area.width.saturating_sub(ow) / 2;
        let oy = area.y + area.height.saturating_sub(oh) / 2;
        let overlay = Rect::new(ox, oy, ow, oh);
        let mut lines = vec![Line::from(vec![
            Span::styled(
                format!("  edit {} — step 2/3: ", edit_label),
                Style::default().fg(theme::INK_DIM),
            ),
            Span::styled(
                "description",
                Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
            ),
        ])];
        for l in pr.edit_desc.lines() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(l.to_string(), Style::default().fg(theme::INK_DIM)),
            ]));
        }
        lines.push(Line::from(vec![Span::styled(
            "  █",
            Style::default().fg(theme::INK_DIM),
        )]));
        while lines.len() < (oh as usize - 3) {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(vec![
            Span::styled("  [Enter]", Style::default().fg(theme::accent(app))),
            Span::styled(" new line  ", Style::default().fg(theme::INK_DIM)),
            Span::styled("[^S]", Style::default().fg(theme::accent(app))),
            Span::styled(" next  ", Style::default().fg(theme::INK_DIM)),
            Span::styled("[Esc]", Style::default().fg(theme::accent(app))),
            Span::styled(" cancel", Style::default().fg(theme::INK_DIM)),
        ]));
        f.render_widget(Clear, overlay);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::RULE))
                    .border_type(app.border_type()),
            ),
            overlay,
        );
    }

    if pr.confirm == PrConfirm::EditBase {
        let dw = 30u16;
        let dh = (pr.branches.len().min(10) + 2) as u16;
        let ox = area.x + area.width.saturating_sub(dw) / 2;
        let oy = area.y + area.height.saturating_sub(dh) / 2;
        let drop_area = Rect::new(ox, oy, dw, dh);

        let drop_items: Vec<ListItem> = pr
            .branches
            .iter()
            .enumerate()
            .map(|(i, branch)| {
                let is_sel = i == pr.branch_idx;
                let color = if is_sel { theme::INK } else { theme::INK_DIM };
                let prefix = if is_sel { "▶ " } else { "  " };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::accent(app))),
                    Span::styled(branch.clone(), Style::default().fg(color)),
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

        let mut drop_state = ListState::default();
        drop_state.select(Some(pr.branch_idx));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(drop_items).block(
                Block::default()
                    .title(Span::styled(
                        " step 4/4: base branch (edit) ",
                        Style::default().fg(theme::INK_DIM),
                    ))
                    .borders(Borders::ALL)
                    .border_type(app.border_type())
                    .border_style(Style::default().fg(theme::RULE)),
            ),
            drop_area,
            &mut drop_state,
        );
    }

    // ── Create — platform multi-select ───────────────────────────────────────
    if pr.confirm == PrConfirm::CreatePlatforms {
        let entries = &pr.available_platforms;
        let max_label = entries.iter().map(|e| e.label.len()).max().unwrap_or(20);
        let dw = (max_label + 10)
            .max(62)
            .min(area.width.saturating_sub(4) as usize) as u16;
        let dh = (entries.len() + 4).min(14) as u16;
        let ox = area.x + area.width.saturating_sub(dw) / 2;
        let oy = area.y + area.height.saturating_sub(dh) / 2;
        let drop_area = Rect::new(ox, oy, dw, dh);

        let drop_items: Vec<ListItem> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let is_sel = i == pr.create_platform_idx;
                let checked = pr.create_platform_selected.get(i).copied().unwrap_or(false);
                let checkbox = if checked { "[✓] " } else { "[ ] " };
                let color = if is_sel { theme::INK } else { theme::INK_DIM };
                let prefix = if is_sel { "▶ " } else { "  " };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::accent(app))),
                    Span::styled(
                        checkbox,
                        Style::default().fg(if checked { theme::OK } else { theme::INK_FAINT }),
                    ),
                    Span::styled(entry.label.clone(), Style::default().fg(color)),
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

        let mut drop_state = ListState::default();
        drop_state.select(Some(pr.create_platform_idx));

        let all_selected = !pr.create_platform_selected.is_empty()
            && pr.create_platform_selected.iter().all(|&s| s);
        let hint_a = if all_selected {
            "[a] deselect all"
        } else {
            "[a] select all"
        };

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(drop_items).block(
                Block::default()
                    .title(Span::styled(
                        " select platforms ",
                        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(app.border_type())
                    .border_style(Style::default().fg(theme::RULE)),
            ),
            Rect::new(
                drop_area.x,
                drop_area.y,
                drop_area.width,
                drop_area.height - 1,
            ),
            &mut drop_state,
        );
        // hint line at bottom
        let hint_area = Rect::new(
            drop_area.x + 1,
            drop_area.y + drop_area.height - 2,
            drop_area.width - 2,
            1,
        );
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("[Space]", Style::default().fg(theme::accent(app))),
                Span::styled(" toggle  ", Style::default().fg(theme::INK_DIM)),
                Span::styled(hint_a, Style::default().fg(theme::accent(app))),
                Span::styled("  [Enter]", Style::default().fg(theme::accent(app))),
                Span::styled(" create", Style::default().fg(theme::INK_DIM)),
            ])),
            hint_area,
        );
    }

    // ── Switch platform dropdown ──────────────────────────────────────────────
    if pr.confirm == PrConfirm::SwitchPlatform {
        let entries = &pr.available_platforms;
        let max_label = entries.iter().map(|e| e.label.len()).max().unwrap_or(20);
        let dw = (max_label + 6).min(50) as u16;
        let dh = (entries.len() + 2).min(12) as u16;
        let ox = area.x + area.width.saturating_sub(dw) / 2;
        let oy = area.y + area.height.saturating_sub(dh) / 2;
        let drop_area = Rect::new(ox, oy, dw, dh);

        let drop_items: Vec<ListItem> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let is_sel = i == pr.platform_idx;
                let is_active = entry.platform == pr.platform && entry.owner == pr.owner;
                let color = if is_sel { theme::INK } else { theme::INK_DIM };
                let prefix = if is_sel { "▶ " } else { "  " };
                let active_marker = if is_active { " ✓" } else { "" };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::accent(app))),
                    Span::styled(
                        format!("{}{}", entry.label, active_marker),
                        Style::default().fg(color),
                    ),
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

        let mut drop_state = ListState::default();
        drop_state.select(Some(pr.platform_idx));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(drop_items).block(
                Block::default()
                    .title(Span::styled(
                        " switch platform ",
                        Style::default().fg(theme::INK_DIM),
                    ))
                    .borders(Borders::ALL)
                    .border_type(app.border_type())
                    .border_style(Style::default().fg(theme::RULE)),
            ),
            drop_area,
            &mut drop_state,
        );
    }

    // ── Ops dropdown ──────────────────────────────────────────────────────────
    if pr.ops_mode {
        let current_state = pr
            .prs
            .get(pr.idx)
            .map(|p| p.state.as_str())
            .unwrap_or("open");
        let create_label = if pr.platform == "gitlab" {
            "create MR"
        } else {
            "create PR"
        };
        let ops: &[(&str, bool)] = &[
            (create_label, false),
            ("edit", false),
            ("merge", false),
            ("close ⚠", true),
            ("checkout", false),
            ("open browser", false),
            ("switch platform", false),
        ];

        let dropdown_w = 22u16;
        let dropdown_h = ops.len() as u16 + 2;
        let entry_y = cols[0].y + 1 + pr.idx as u16 + 1;
        let drop_y = if entry_y + dropdown_h < cols[0].y + cols[0].height {
            entry_y
        } else {
            cols[0].y + cols[0].height - dropdown_h
        };
        let drop_area = Rect::new(cols[0].x + 3, drop_y, dropdown_w, dropdown_h);

        let drop_items: Vec<ListItem> = ops
            .iter()
            .enumerate()
            .map(|(i, (label, danger))| {
                let is_sel = i == pr.ops_idx;
                let dimmed = i == 2 && current_state != "open" && current_state != "opened";
                let color = if dimmed {
                    theme::INK_FAINT
                } else if *danger {
                    theme::BAD
                } else if is_sel {
                    theme::INK
                } else {
                    theme::INK_DIM
                };
                let prefix = if is_sel { "▶ " } else { "  " };
                let style = if is_sel && !dimmed {
                    Style::default()
                        .bg(theme::selection(app))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::accent(app))),
                    Span::styled(*label, Style::default().fg(color)),
                ]))
                .style(style)
            })
            .collect();

        let mut drop_state = ListState::default();
        drop_state.select(Some(pr.ops_idx));

        let drop_block = Block::default()
            .borders(Borders::ALL)
            .border_type(app.border_type())
            .border_style(Style::default().fg(theme::RULE));

        f.render_widget(Clear, drop_area);
        f.render_stateful_widget(
            List::new(drop_items).block(drop_block),
            drop_area,
            &mut drop_state,
        );
    }
}

/// Open is settled and green; anything else is closed, and closed is quiet.
fn state_color(state: &str) -> ratatui::style::Color {
    if state == "open" {
        theme::OK
    } else {
        theme::INK_DIM
    }
}
