use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn render(f: &mut Frame, app: &crate::tui::app::App) {
    let bc = app.brand_color();
    let bt = app.border_type();
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    // Header
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " ⛩  gitorii",
                Style::default().fg(bc).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  keybindings", Style::default().fg(Color::Gray)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(bt)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        chunks[0],
    );

    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "navigation",
            &[
                ("Tab", "focus sidebar / cycle panels"),
                ("j / ↓", "next item in sidebar"),
                ("k / ↑", "previous item in sidebar"),
                ("Enter", "open selected view"),
                ("Esc", "back / cancel"),
                ("?", "this help screen"),
                ("q / Ctrl+c", "quit"),
            ],
        ),
        (
            "files view",
            &[
                ("Tab / Shift+Tab", "move between panels"),
                ("j / ↓", "move down"),
                ("k / ↑", "move up"),
                ("space", "stage / unstage file"),
                ("d", "open diff for file"),
                ("t", "tree of HEAD — same key, or Esc, comes back"),
                (
                    "Enter / space / h l / - +",
                    "open or fold a folder in the tree",
                ),
                ("click", "select a row — second click opens a folder"),
                ("r", "refresh"),
            ],
        ),
        (
            "log view",
            &[
                ("j / ↓", "move down"),
                ("k / ↑", "move up"),
                ("d", "diff selected commit"),
                ("o", "ops menu (diff / copy hash / search)"),
                ("space", "pick commits for an op"),
                ("/", "search commits — author:<who> narrows to one"),
                ("A", "authors menu — pick one to filter the log by"),
                ("Esc", "back to files"),
            ],
        ),
        (
            "diff view",
            &[
                ("j / ↓", "scroll down"),
                ("k / ↑", "scroll up"),
                ("Esc / q", "back"),
            ],
        ),
        (
            "commit view",
            &[
                ("type", "write commit message"),
                ("← →", "move cursor"),
                ("Enter", "confirm commit"),
                ("Esc", "cancel"),
            ],
        ),
        (
            "clone view",
            &[
                ("j / ↓ k / ↑", "move through the repositories"),
                ("p", "host menu — which account to list"),
                ("/", "filter by name or description"),
                ("u", "type or paste a URL instead"),
                ("d", "edit where the clone lands"),
                ("t", "ssh / https"),
                ("r", "ask the host again"),
                ("Enter", "clone it, then open it"),
                ("Esc", "stop a clone that is running"),
            ],
        ),
        (
            "sync view",
            &[
                ("j / ↓", "select operation"),
                ("k / ↑", "select operation"),
                ("Enter", "run selected operation"),
                ("Esc", "cancel"),
            ],
        ),
        (
            "issues view",
            &[
                ("j / ↓ k / ↑", "move through the issues"),
                ("p", "switch which forge the list is from"),
                ("o", "ops — create, comment, browser, close"),
                ("^r", "reload"),
            ],
        ),
    ];

    let mut items: Vec<ListItem> = vec![];
    for (section, binds) in sections {
        items.push(ListItem::new(Line::from(Span::styled(
            format!("  {}", section.to_uppercase()),
            Style::default().fg(bc).add_modifier(Modifier::BOLD),
        ))));
        for (key, desc) in *binds {
            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!("    {:18}", key),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*desc, Style::default().fg(Color::Gray)),
            ])));
        }
        items.push(ListItem::new(Line::from("")));
    }

    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(bt)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[Esc / ?]", Style::default().fg(bc)),
            Span::raw(" close"),
        ])),
        chunks[2],
    );
}
