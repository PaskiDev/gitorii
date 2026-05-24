// Unified Platform view (0.7.12) — CI/CD surface for the active remote.
//
// Four sub-tabs (Pipelines / Jobs / Releases / Packages) over a single
// remote. Drill-down: Enter on a pipeline → Jobs tab populated with its
// jobs; Enter on a job → its log/trace in a scrollable panel. Esc backs
// out of drill-downs. `r` opens a centred popup to switch remote.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::tui::app::{App, PlatformFocus, PlatformSubTab};
use super::super::ui::{C_WHITE, C_SUBTLE, C_DIM, C_GREEN, C_RED, C_YELLOW, C_CYAN};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header: remote + sub-tabs
            Constraint::Min(1),    // body
        ])
        .split(area);

    render_header(f, app, rows[0]);

    // Drill-down: job log takes the whole body
    if app.platform_view.focus == PlatformFocus::JobLog {
        render_job_log(f, app, rows[1]);
    } else {
        // Body = list (60%) + detail (40%)
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(60),
                Constraint::Percentage(40),
            ])
            .split(rows[1]);
        render_list(f, app, cols[0]);
        render_detail(f, app, cols[1]);
    }

    if app.platform_view.focus == PlatformFocus::RemotePopup {
        render_remote_popup(f, app, area);
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let pv = &app.platform_view;

    let remote_label = format!(" remote: {} ▾ ", pv.remote);
    let platform_label = if pv.platform.is_empty() {
        " — ".to_string()
    } else {
        format!(" {} {}/{} ", pv.platform, pv.owner, pv.repo_name)
    };

    let mut tabs: Vec<Span> = Vec::new();
    for (i, (st, label)) in [
        (PlatformSubTab::Pipelines, "[1] pipelines"),
        (PlatformSubTab::Jobs,      "[2] jobs"),
        (PlatformSubTab::Releases,  "[3] releases"),
        (PlatformSubTab::Packages,  "[4] packages"),
    ].iter().enumerate() {
        if i > 0 { tabs.push(Span::raw("  ")); }
        let active = pv.sub_tab == *st;
        let style = if active {
            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(bc)
        };
        tabs.push(Span::styled(*label, style));
    }

    let mut line: Vec<Span> = vec![
        Span::raw(" "),
        Span::styled(remote_label, Style::default().fg(C_YELLOW).add_modifier(Modifier::BOLD)),
        Span::styled(platform_label, Style::default().fg(C_DIM)),
        Span::raw(" "),
    ];
    line.extend(tabs);

    f.render_widget(
        Paragraph::new(Line::from(line)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(bc))
                .title(Span::styled(" platform ", Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)))
        ),
        area,
    );
}

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let focused = !app.sidebar_focused;
    let pv = &app.platform_view;

    let border = if focused && pv.focus == PlatformFocus::List {
        Style::default().fg(C_WHITE)
    } else {
        Style::default().fg(bc)
    };

    let (title, items, selected): (String, Vec<ListItem>, usize) = if pv.loading {
        (
            list_title(pv),
            vec![ListItem::new(Line::from(Span::styled("  loading...", Style::default().fg(C_SUBTLE))))],
            0,
        )
    } else if let Some(err) = &pv.error {
        (
            list_title(pv),
            wrap_error(err),
            0,
        )
    } else {
        match pv.sub_tab {
            PlatformSubTab::Pipelines => render_pipelines_items(app),
            PlatformSubTab::Jobs      => render_jobs_items(app),
            PlatformSubTab::Releases  => render_releases_items(app),
            PlatformSubTab::Packages  => render_packages_items(app),
        }
    };

    let mut state = ListState::default();
    if !items.is_empty() && pv.error.is_none() && !pv.loading {
        state.select(Some(selected));
    }

    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(title, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(border)
        ),
        area,
        &mut state,
    );
}

fn list_title(pv: &crate::tui::app::PlatformState) -> String {
    match pv.sub_tab {
        PlatformSubTab::Pipelines => format!(" pipelines ({}) ", pv.pipelines.len()),
        PlatformSubTab::Jobs => {
            if let Some(pid) = pv.active_pipeline_id {
                format!(" jobs of #{} ({}) ", pid, pv.jobs.len())
            } else {
                format!(" jobs ({}) ", pv.jobs.len())
            }
        }
        PlatformSubTab::Releases => format!(" releases ({}) ", pv.releases.len()),
        PlatformSubTab::Packages => format!(" packages ({}) ", pv.packages.len()),
    }
}

fn render_pipelines_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv.pipelines.iter().enumerate().map(|(i, p)| {
        let is_sel = i == pv.pipelines_idx;
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        let prefix = if is_sel { "█ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(app.brand_color())),
            Span::styled(format!("#{:<10}", p.id), Style::default().fg(C_CYAN)),
            Span::styled(format!("{:<10}", p.status), Style::default().fg(status_color(&p.status))),
            Span::styled(format!("{:<18}", truncate(&p.branch, 17)), Style::default().fg(C_WHITE)),
            Span::styled(format!("{:<20}", short_time(&p.created_at)), Style::default().fg(C_DIM)),
        ])).style(style)
    }).collect();
    (list_title(pv), items, pv.pipelines_idx)
}

fn render_jobs_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv.jobs.iter().enumerate().map(|(i, j)| {
        let is_sel = i == pv.jobs_idx;
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        let prefix = if is_sel { "█ " } else { "  " };
        let dur = j.duration_seconds.map(|s| format!("{}s", s as u64)).unwrap_or_default();
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(app.brand_color())),
            Span::styled(format!("#{:<10}", j.id), Style::default().fg(C_CYAN)),
            Span::styled(format!("{:<10}", j.status), Style::default().fg(status_color(&j.status))),
            Span::styled(format!("{:<10}", truncate(&j.stage, 9)), Style::default().fg(C_YELLOW)),
            Span::styled(format!("{:<24}", truncate(&j.name, 23)), Style::default().fg(C_WHITE)),
            Span::styled(format!("{:>8}", dur), Style::default().fg(C_DIM)),
        ])).style(style)
    }).collect();
    (list_title(pv), items, pv.jobs_idx)
}

fn render_releases_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv.releases.iter().enumerate().map(|(i, r)| {
        let is_sel = i == pv.releases_idx;
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        let prefix = if is_sel { "█ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(app.brand_color())),
            Span::styled(format!("{:<16}", truncate(&r.tag, 15)), Style::default().fg(C_GREEN)),
            Span::styled(format!("{:<28}", truncate(&r.name, 27)), Style::default().fg(C_WHITE)),
            Span::styled(format!("{:<20}", short_time(&r.created_at)), Style::default().fg(C_DIM)),
        ])).style(style)
    }).collect();
    (list_title(pv), items, pv.releases_idx)
}

fn render_packages_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv.packages.iter().enumerate().map(|(i, p)| {
        let is_sel = i == pv.packages_idx;
        let style = if is_sel {
            Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
        } else { Style::default() };
        let prefix = if is_sel { "█ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(app.brand_color())),
            Span::styled(format!("{:<22}", truncate(&p.name, 21)), Style::default().fg(C_WHITE)),
            Span::styled(format!("{:<14}", truncate(&p.version, 13)), Style::default().fg(C_GREEN)),
            Span::styled(format!("{:<10}", truncate(&p.package_type, 9)), Style::default().fg(C_YELLOW)),
            Span::styled(format!("{:<20}", short_time(&p.created_at)), Style::default().fg(C_DIM)),
        ])).style(style)
    }).collect();
    (list_title(pv), items, pv.packages_idx)
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let pv = &app.platform_view;

    let body: Vec<Line> = match pv.sub_tab {
        PlatformSubTab::Pipelines => pv.pipelines.get(pv.pipelines_idx).map(|p| vec![
            line_kv("id",        &format!("#{}", p.id), C_CYAN),
            line_kv("status",    &p.raw_status,        status_color(&p.status)),
            line_kv("branch",    &p.branch,            C_WHITE),
            line_kv("sha",       &short_sha(&p.sha),   C_YELLOW),
            line_kv("created",   &p.created_at,        C_DIM),
            line_kv("updated",   &p.updated_at,        C_DIM),
            Line::from(""),
            Line::from(Span::styled(p.web_url.clone(), Style::default().fg(C_CYAN))),
            Line::from(""),
            Line::from(Span::styled("Enter → drill into jobs", Style::default().fg(C_SUBTLE))),
        ]).unwrap_or_else(|| vec![Line::from(Span::styled("no selection", Style::default().fg(C_DIM)))]),

        PlatformSubTab::Jobs => pv.jobs.get(pv.jobs_idx).map(|j| vec![
            line_kv("id",        &format!("#{}", j.id), C_CYAN),
            line_kv("pipeline",  &format!("#{}", j.pipeline_id), C_CYAN),
            line_kv("status",    &j.raw_status,        status_color(&j.status)),
            line_kv("stage",     &j.stage,             C_YELLOW),
            line_kv("name",      &j.name,              C_WHITE),
            line_kv("duration",  &j.duration_seconds.map(|s| format!("{}s", s as u64)).unwrap_or_default(), C_DIM),
            Line::from(""),
            Line::from(Span::styled(j.web_url.clone(), Style::default().fg(C_CYAN))),
            Line::from(""),
            Line::from(Span::styled("Enter → fetch log", Style::default().fg(C_SUBTLE))),
        ]).unwrap_or_else(|| vec![Line::from(Span::styled("no selection", Style::default().fg(C_DIM)))]),

        PlatformSubTab::Releases => pv.releases.get(pv.releases_idx).map(|r| vec![
            line_kv("tag",       &r.tag,                                C_GREEN),
            line_kv("name",      &r.name,                                C_WHITE),
            line_kv("created",   &r.created_at,                         C_DIM),
            Line::from(""),
            Line::from(Span::styled(r.web_url.clone(), Style::default().fg(C_CYAN))),
        ]).unwrap_or_else(|| vec![Line::from(Span::styled("no selection", Style::default().fg(C_DIM)))]),

        PlatformSubTab::Packages => pv.packages.get(pv.packages_idx).map(|p| vec![
            line_kv("name",     &p.name,           C_WHITE),
            line_kv("version",  &p.version,        C_GREEN),
            line_kv("type",     &p.package_type,   C_YELLOW),
            line_kv("created",  &p.created_at,     C_DIM),
        ]).unwrap_or_else(|| vec![Line::from(Span::styled("no selection", Style::default().fg(C_DIM)))]),
    };

    f.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(Span::styled(" detail ", Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(bc))
        ),
        area,
    );
}

fn render_job_log(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let pv = &app.platform_view;
    let log = pv.job_log.as_deref().unwrap_or(if pv.loading { "loading log..." } else { "(no log)" });

    f.render_widget(
        Paragraph::new(log)
            .scroll((pv.job_log_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title(Span::styled(" job log — Esc to go back ", Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_type(app.border_type())
                    .border_style(Style::default().fg(bc))
            ),
        area,
    );
}

fn render_remote_popup(f: &mut Frame, app: &App, area: Rect) {
    let pv = &app.platform_view;
    let bc = app.brand_color();

    // Centred popup ~30 cols wide, height grows with list (max 14).
    let w: u16 = 36;
    let n = pv.remotes.len().max(1) as u16;
    let h: u16 = (n + 4).min(14);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    };

    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = if pv.remotes.is_empty() {
        vec![ListItem::new(Line::from(Span::styled("  (no remotes)", Style::default().fg(C_DIM))))]
    } else {
        pv.remotes.iter().enumerate().map(|(i, name)| {
            let is_sel = i == pv.remote_popup_idx;
            let is_cur = name == &pv.remote;
            let style = if is_sel {
                Style::default().bg(app.selected_bg()).add_modifier(Modifier::BOLD)
            } else { Style::default() };
            let marker = if is_cur { "●" } else { " " };
            ListItem::new(Line::from(vec![
                Span::styled(if is_sel { "▶ " } else { "  " }, Style::default().fg(bc)),
                Span::styled(format!("{} ", marker), Style::default().fg(C_GREEN)),
                Span::styled(name.clone(), Style::default().fg(if is_sel { C_WHITE } else { C_SUBTLE })),
            ])).style(style)
        }).collect()
    };

    let mut state = ListState::default();
    state.select(Some(pv.remote_popup_idx.min(pv.remotes.len().saturating_sub(1))));

    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(" select remote ", Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE))
        ),
        popup,
        &mut state,
    );
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn status_color(s: &str) -> ratatui::style::Color {
    match s {
        "success" => C_GREEN,
        "failed"  => C_RED,
        "running" => C_CYAN,
        "canceled" => C_DIM,
        "pending" => C_YELLOW,
        _ => C_SUBTLE,
    }
}

fn line_kv<'a>(k: &'a str, v: &str, vc: ratatui::style::Color) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {:<10}", k), Style::default().fg(C_SUBTLE)),
        Span::styled(v.to_string(), Style::default().fg(vc)),
    ])
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { return s.to_string(); }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut)
}

fn short_time(s: &str) -> String {
    // ISO 8601 → "YYYY-MM-DD HH:MM" if possible, else truncate.
    if s.len() >= 16 && s.as_bytes().get(10) == Some(&b'T') {
        format!("{} {}", &s[..10], &s[11..16])
    } else {
        truncate(s, 19)
    }
}

fn short_sha(s: &str) -> String {
    s.chars().take(8).collect()
}

fn wrap_error(err: &str) -> Vec<ListItem<'static>> {
    let mut items = vec![ListItem::new(Line::from(vec![
        Span::styled("  ✗ ", Style::default().fg(C_RED)),
        Span::styled("error", Style::default().fg(C_RED).add_modifier(Modifier::BOLD)),
    ]))];
    for chunk in err.chars().collect::<Vec<_>>().chunks(50) {
        let s: String = chunk.iter().collect();
        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("  {}", s), Style::default().fg(C_SUBTLE)),
        ])));
    }
    items
}
