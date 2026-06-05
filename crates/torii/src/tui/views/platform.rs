// Unified Platform view — CI/CD surface for the active remote.
//
// Layout (0.7.26 rework):
//
//   ┌───────────────────────────────────────────────┐
//   │ header: remote popup trigger + Tabs widget    │  3 rows
//   ├───────────────────────────────────────────────┤
//   │ list (60%)             │ detail (40%)         │
//   │                        │                      │  flexible
//   │                        │                      │
//   ├───────────────────────────────────────────────┤
//   │ footer: hints + filters + action result       │  2 rows
//   └───────────────────────────────────────────────┘
//
// Five sub-tabs: Pipelines / Jobs / Releases / Packages / Runners.
// Drill-down: Enter on a pipeline → Jobs of that pipeline; Enter on a
// job → log/trace in a scrollable panel that takes the full body.
// Esc backs out of drill-downs.
//
// Interaction lives in three dropdowns triggered by single keys:
//   r  → remote-selector popup
//   o  → contextual ops (cancel / retry / pause / etc., per sub-tab)
//   f  → list filters  (status cycle + branch-only toggle)
// This replaces the per-action keys (c/x/a/t/d/s/b) we shipped in
// 0.7.24/0.7.25 — those collided across sub-tabs (c meant cancel in
// Pipelines but pause in Runners) and weren't discoverable.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame,
};

use super::super::ui::{C_DIM, C_GREEN, C_RED, C_SUBTLE, C_WHITE, C_YELLOW};
use crate::tui::app::{App, PlatformFocus, PlatformSubTab};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header: Tabs
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
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(rows[1]);
        render_list(f, app, cols[0]);
        render_detail(f, app, cols[1]);
    }

    // Overlays — drawn last so they sit on top of body content.
    // (Bottom-of-screen hints are handled by `render_hint` in ui.rs,
    // matching every other view; we don't add our own footer here.)
    match app.platform_view.focus {
        PlatformFocus::RemotePopup => render_remote_popup(f, app, area),
        PlatformFocus::OpsDropdown => render_ops_dropdown(f, app, area),
        PlatformFocus::FilterDropdown => render_filter_dropdown(f, app, area),
        _ => {}
    }
}

/// Width-aware column formatter. `format!("{:<10}", s)` only pads
/// when `s` is *shorter* than 10 — a 14-char GitHub workflow_run id
/// would overflow and visually concatenate with the next column. This
/// helper truncates with an ellipsis so the column boundary is
/// preserved no matter the input length.
fn col(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n > width {
        let cut: String = s.chars().take(width.saturating_sub(1)).collect();
        format!("{}… ", cut)
    } else {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(width.saturating_sub(n)));
        out.push(' ');
        out
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let pv = &app.platform_view;
    let focused = !app.sidebar_focused;

    // Title carries the active remote + resolved platform/owner/repo
    // so it doesn't compete with the Tabs widget for horizontal space.
    // Trailing markers show the active filter / live state in the same
    // line so the user has them in eye-shot without dragging attention
    // away from the body.
    let mut title_text = if pv.platform.is_empty() {
        format!(" platform · {} ", pv.remote)
    } else {
        format!(" platform · {} → {}/{} ", pv.remote, pv.owner, pv.repo_name)
    };
    if let Some(s) = &pv.filter_status {
        title_text.push_str(&format!("· status:{} ", s));
    }
    if pv.filter_branch_only {
        title_text.push_str("· branch-only ");
    }
    if pv.auto_refresh {
        title_text.push_str("· ⟳ live ");
    }

    let titles: Vec<&'static str> = vec![
        " 1 pipelines ",
        " 2 jobs ",
        " 3 releases ",
        " 4 packages ",
        " 5 runners ",
    ];
    let active_idx = match pv.sub_tab {
        PlatformSubTab::Pipelines => 0,
        PlatformSubTab::Jobs => 1,
        PlatformSubTab::Releases => 2,
        PlatformSubTab::Packages => 3,
        PlatformSubTab::Runners => 4,
    };

    // Match the per-view title color convention used in log.rs / branch.rs:
    // C_WHITE when focused, bc otherwise. Same for the border.
    let title_color = if focused { C_WHITE } else { bc };
    let border_color = if focused { C_WHITE } else { bc };

    let tabs = Tabs::new(titles)
        .select(active_idx)
        .style(Style::default().fg(bc))
        .highlight_style(
            Style::default()
                .fg(C_WHITE)
                .bg(app.selected_bg())
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled("·", Style::default().fg(C_DIM)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(
                    title_text,
                    Style::default()
                        .fg(title_color)
                        .add_modifier(Modifier::BOLD),
                )),
        );

    f.render_widget(tabs, area);
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
            vec![ListItem::new(Line::from(Span::styled(
                "  loading...",
                Style::default().fg(C_SUBTLE),
            )))],
            0,
        )
    } else if let Some(err) = &pv.error {
        (list_title(pv), wrap_error(err), 0)
    } else {
        match pv.sub_tab {
            PlatformSubTab::Pipelines => render_pipelines_items(app),
            PlatformSubTab::Jobs => render_jobs_items(app),
            PlatformSubTab::Releases => render_releases_items(app),
            PlatformSubTab::Packages => render_packages_items(app),
            PlatformSubTab::Runners => render_runners_items(app),
        }
    };

    let mut state = ListState::default();
    if !items.is_empty() && pv.error.is_none() && !pv.loading {
        state.select(Some(selected));
    }

    let title_color = if focused && pv.focus == PlatformFocus::List {
        C_WHITE
    } else {
        bc
    };
    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    title,
                    Style::default()
                        .fg(title_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(border),
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
        PlatformSubTab::Runners => format!(" runners ({}) ", pv.runners.len()),
    }
}

fn render_pipelines_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv
        .pipelines
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_sel = i == pv.pipelines_idx;
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            let id = format!("#{}", p.id);
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(app.brand_color())),
                Span::styled(col(&id, 13), Style::default().fg(app.brand_color())),
                Span::styled(
                    col(&p.status, 10),
                    Style::default().fg(status_color(&p.status)),
                ),
                Span::styled(col(&p.branch, 18), Style::default().fg(C_WHITE)),
                Span::styled(
                    col(&short_time(&p.created_at), 18),
                    Style::default().fg(C_DIM),
                ),
            ]))
            .style(style)
        })
        .collect();
    (list_title(pv), items, pv.pipelines_idx)
}

fn render_jobs_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv
        .jobs
        .iter()
        .enumerate()
        .map(|(i, j)| {
            let is_sel = i == pv.jobs_idx;
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            let dur = j
                .duration_seconds
                .map(|s| format!("{}s", s as u64))
                .unwrap_or_else(|| "—".into());
            let id = format!("#{}", j.id);
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(app.brand_color())),
                Span::styled(col(&id, 13), Style::default().fg(app.brand_color())),
                Span::styled(
                    col(&j.status, 10),
                    Style::default().fg(status_color(&j.status)),
                ),
                Span::styled(col(&j.stage, 10), Style::default().fg(C_DIM)),
                Span::styled(col(&j.name, 24), Style::default().fg(C_WHITE)),
                Span::styled(col(&dur, 8), Style::default().fg(C_DIM)),
            ]))
            .style(style)
        })
        .collect();
    (list_title(pv), items, pv.jobs_idx)
}

fn render_releases_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv
        .releases
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let is_sel = i == pv.releases_idx;
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(app.brand_color())),
                Span::styled(col(&r.tag, 16), Style::default().fg(C_GREEN)),
                Span::styled(col(&r.name, 28), Style::default().fg(C_WHITE)),
                Span::styled(
                    col(&short_time(&r.created_at), 18),
                    Style::default().fg(C_DIM),
                ),
            ]))
            .style(style)
        })
        .collect();
    (list_title(pv), items, pv.releases_idx)
}

fn render_packages_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv
        .packages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_sel = i == pv.packages_idx;
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(app.brand_color())),
                Span::styled(col(&p.name, 22), Style::default().fg(C_WHITE)),
                Span::styled(col(&p.version, 14), Style::default().fg(C_GREEN)),
                Span::styled(col(&p.package_type, 10), Style::default().fg(C_DIM)),
                Span::styled(
                    col(&short_time(&p.created_at), 18),
                    Style::default().fg(C_DIM),
                ),
            ]))
            .style(style)
        })
        .collect();
    (list_title(pv), items, pv.packages_idx)
}

fn render_runners_items(app: &App) -> (String, Vec<ListItem<'static>>, usize) {
    let pv = &app.platform_view;
    let items: Vec<ListItem> = pv
        .runners
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let is_sel = i == pv.runners_idx;
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "█ " } else { "  " };
            let status_color = match r.status.as_str() {
                "online" | "active" => C_GREEN,
                "offline" | "stale" => C_DIM,
                "paused" => C_DIM,
                _ => C_SUBTLE,
            };
            // 0.8.1 — distinguish online platform runners from torii-
            // spawned Docker containers on this host. `🌐` for online,
            // `🐳` for local-docker.
            let scope_glyph = if r.runner_type == "local-docker" {
                "🐳"
            } else {
                "🌐"
            };
            let scope_label = if r.runner_type == "local-docker" {
                "local"
            } else {
                "online"
            };
            let scope_color = if r.runner_type == "local-docker" {
                C_YELLOW
            } else {
                app.brand_color()
            };
            let tags_str = if r.tags.is_empty() {
                "—".to_string()
            } else {
                r.tags.join(",")
            };
            // Local containers use their container name as the id; the
            // platform's runners use the numeric id from the API.
            let id_disp = if r.runner_type == "local-docker" {
                r.id.clone()
            } else {
                format!("#{}", r.id)
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(app.brand_color())),
                Span::styled(
                    format!("{} ", scope_glyph),
                    Style::default().fg(scope_color),
                ),
                Span::styled(col(scope_label, 7), Style::default().fg(scope_color)),
                Span::styled(col(&id_disp, 18), Style::default().fg(app.brand_color())),
                Span::styled(col(&r.status, 10), Style::default().fg(status_color)),
                Span::styled(col(&r.description, 22), Style::default().fg(C_WHITE)),
                Span::styled(col(&tags_str, 24), Style::default().fg(C_DIM)),
            ]))
            .style(style)
        })
        .collect();
    (list_title(pv), items, pv.runners_idx)
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let pv = &app.platform_view;

    // Value column width = panel inner width − 2 side borders − key
    // prefix. Clamp to a minimum so very narrow terminals still wrap
    // sanely instead of producing 1-char-wide chunks.
    let inner_w = (area.width as usize).saturating_sub(2);
    let value_w = inner_w.saturating_sub(KV_PREFIX_W).max(20);

    let mut body: Vec<Line<'static>> = Vec::new();

    match pv.sub_tab {
        PlatformSubTab::Pipelines => {
            if let Some(p) = pv.pipelines.get(pv.pipelines_idx) {
                kv(&mut body, "id", &format!("#{}", p.id), bc, value_w);
                kv(
                    &mut body,
                    "status",
                    &p.raw_status,
                    status_color(&p.status),
                    value_w,
                );
                kv(&mut body, "branch", &p.branch, C_WHITE, value_w);
                kv(&mut body, "sha", &short_sha(&p.sha), C_DIM, value_w);
                kv(&mut body, "created", &p.created_at, C_DIM, value_w);
                kv(&mut body, "updated", &p.updated_at, C_DIM, value_w);
                body.push(Line::from(""));
                kv(&mut body, "url", &p.web_url, bc, value_w);
            }
        }
        PlatformSubTab::Jobs => {
            if let Some(j) = pv.jobs.get(pv.jobs_idx) {
                let dur = j
                    .duration_seconds
                    .map(|s| format!("{}s", s as u64))
                    .unwrap_or_default();
                kv(&mut body, "id", &format!("#{}", j.id), bc, value_w);
                kv(
                    &mut body,
                    "pipeline",
                    &format!("#{}", j.pipeline_id),
                    bc,
                    value_w,
                );
                kv(
                    &mut body,
                    "status",
                    &j.raw_status,
                    status_color(&j.status),
                    value_w,
                );
                kv(&mut body, "stage", &j.stage, C_DIM, value_w);
                kv(&mut body, "name", &j.name, C_WHITE, value_w);
                kv(&mut body, "duration", &dur, C_DIM, value_w);
                body.push(Line::from(""));
                kv(&mut body, "url", &j.web_url, bc, value_w);
            }
        }
        PlatformSubTab::Releases => {
            if let Some(r) = pv.releases.get(pv.releases_idx) {
                kv(&mut body, "tag", &r.tag, C_GREEN, value_w);
                kv(&mut body, "name", &r.name, C_WHITE, value_w);
                kv(&mut body, "created", &r.created_at, C_DIM, value_w);
                body.push(Line::from(""));
                kv(&mut body, "url", &r.web_url, bc, value_w);
            }
        }
        PlatformSubTab::Packages => {
            if let Some(p) = pv.packages.get(pv.packages_idx) {
                kv(&mut body, "name", &p.name, C_WHITE, value_w);
                kv(&mut body, "version", &p.version, C_GREEN, value_w);
                kv(&mut body, "type", &p.package_type, C_DIM, value_w);
                kv(&mut body, "created", &p.created_at, C_DIM, value_w);
            }
        }
        PlatformSubTab::Runners => {
            if let Some(r) = pv.runners.get(pv.runners_idx) {
                let tags = if r.tags.is_empty() {
                    "—".to_string()
                } else {
                    r.tags.join(", ")
                };
                let status_c = match r.status.as_str() {
                    "online" | "active" => C_GREEN,
                    "offline" | "stale" => C_DIM,
                    "paused" => C_DIM,
                    _ => C_SUBTLE,
                };
                kv(&mut body, "id", &format!("#{}", r.id), bc, value_w);
                kv(&mut body, "status", &r.status, status_c, value_w);
                kv(&mut body, "description", &r.description, C_WHITE, value_w);
                kv(&mut body, "type", &r.runner_type, C_DIM, value_w);
                kv(&mut body, "os", &r.os, C_DIM, value_w);
                if !r.ip_address.is_empty() {
                    kv(&mut body, "ip", &r.ip_address, C_DIM, value_w);
                }
                if !r.version.is_empty() {
                    kv(&mut body, "version", &r.version, C_DIM, value_w);
                }
                kv(&mut body, "tags", &tags, C_DIM, value_w);
                if !r.web_url.is_empty() {
                    body.push(Line::from(""));
                    kv(&mut body, "url", &r.web_url, bc, value_w);
                }
            }
        }
    }

    if body.is_empty() {
        body.push(Line::from(Span::styled(
            "no selection",
            Style::default().fg(C_DIM),
        )));
    }

    // 0.7.26: detail panel only carries entity data — hints and
    // action results live in the global bottom hint (ui.rs) and the
    // App-wide `status_msg` line, like every other view does.
    // 0.7.28: no Paragraph wrap — we wrap manually above so the
    // continuation lines stay indented to the value column.
    f.render_widget(
        Paragraph::new(body).block(
            Block::default()
                .title(Span::styled(
                    " detail ",
                    Style::default().fg(bc).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(bc)),
        ),
        area,
    );
}

fn render_job_log(f: &mut Frame, app: &App, area: Rect) {
    let bc = app.brand_color();
    let pv = &app.platform_view;
    let log = pv.job_log.as_deref().unwrap_or(if pv.loading {
        "loading log..."
    } else {
        "(no log)"
    });

    let live = if pv.job_log_live { " ● live  " } else { "" };
    let follow = if !pv.job_log_user_scrolled {
        "follow"
    } else {
        "manual"
    };
    // Title: "job log · <live?> · <follow|manual>". Same C_WHITE bold
    // as the rest of the focused-view titles (log.rs / branch.rs). The
    // live indicator is a coloured prefix span, not a colour-shifted
    // title — keeps the chrome consistent across sub-tabs.
    let mut title_spans: Vec<Span> = vec![Span::styled(
        " job log ",
        Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
    )];
    if pv.job_log_live {
        title_spans.push(Span::styled("· ● live ", Style::default().fg(C_GREEN)));
    }
    title_spans.push(Span::styled(
        format!("· {} ", follow),
        Style::default().fg(C_DIM),
    ));
    let _ = live;

    f.render_widget(
        Paragraph::new(log)
            .scroll((pv.job_log_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title(Line::from(title_spans))
                    .borders(Borders::ALL)
                    .border_type(app.border_type())
                    .border_style(Style::default().fg(bc)),
            ),
        area,
    );
}

/// Ops dropdown — single-key (`o`) menu of contextual actions for the
/// current sub-tab. List of (label, description, enabled) per row.
pub fn ops_for(pv: &crate::tui::app::PlatformState) -> Vec<(&'static str, &'static str)> {
    match pv.sub_tab {
        PlatformSubTab::Pipelines => vec![
            ("cancel pipeline", "stop the run server-side"),
            ("retry pipeline", "re-run failed/canceled jobs"),
        ],
        PlatformSubTab::Jobs => vec![
            ("cancel job", "stop this job (GitLab)"),
            ("retry job", "re-run this job (GitLab)"),
            ("download artifacts", "save zip to <repo>/artifacts/"),
        ],
        PlatformSubTab::Runners => vec![
            ("pause runner", "stop picking up jobs"),
            ("resume runner", "re-enable job pickup"),
            ("reset auth token", "rotate runner credential (GitLab)"),
            ("remove runner", "delete registration ⚠"),
        ],
        _ => vec![],
    }
}

fn render_ops_dropdown(f: &mut Frame, app: &App, area: Rect) {
    let pv = &app.platform_view;
    let bc = app.brand_color();
    let ops = ops_for(pv);
    if ops.is_empty() {
        return;
    }

    let w: u16 = 40;
    let h: u16 = ops.len() as u16 + 2;
    let popup = Rect {
        x: area.x + 4,
        y: area.y + 4,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = ops
        .iter()
        .enumerate()
        .map(|(i, (label, desc))| {
            let is_sel = i == pv.dropdown_idx;
            let danger = label.starts_with("remove");
            let label_color = if danger {
                C_RED
            } else if is_sel {
                C_WHITE
            } else {
                C_SUBTLE
            };
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(format!("{:<22}", label), Style::default().fg(label_color)),
                Span::styled(*desc, Style::default().fg(C_DIM)),
            ]))
            .style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(pv.dropdown_idx));

    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    " ops — Enter to run · Esc to close ",
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
        &mut state,
    );
}

/// Filter dropdown — combines status cycle + branch toggle in one
/// menu. Selecting a row applies it immediately and closes the
/// dropdown; the list reloads with the new filters.
pub fn filters_for(pv: &crate::tui::app::PlatformState) -> Vec<(&'static str, &'static str)> {
    let status = pv.filter_status.as_deref().unwrap_or("(none)");
    let branch = if pv.filter_branch_only {
        "✓ on"
    } else {
        "  off"
    };
    // We hand-write the labels each call so the current state shows
    // up in the dropdown header.
    let _ = status;
    let _ = branch;
    vec![
        ("status: any", "show all"),
        ("status: running", "only running"),
        ("status: failed", "only failed"),
        ("status: success", "only success"),
        ("status: pending", "only pending"),
        ("branch: toggle", "filter by the current branch"),
    ]
}

fn render_filter_dropdown(f: &mut Frame, app: &App, area: Rect) {
    let pv = &app.platform_view;
    let bc = app.brand_color();
    let rows = filters_for(pv);

    let w: u16 = 40;
    let h: u16 = rows.len() as u16 + 2;
    let popup = Rect {
        x: area.x + 4,
        y: area.y + 4,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, popup);

    let cur_status = pv.filter_status.as_deref().unwrap_or("(any)");

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, (label, desc))| {
            let is_sel = i == pv.dropdown_idx;
            let active = match (i, &pv.filter_status, pv.filter_branch_only) {
                (0, None, _) => true,
                (1, Some(s), _) if s == "running" => true,
                (2, Some(s), _) if s == "failed" => true,
                (3, Some(s), _) if s == "success" => true,
                (4, Some(s), _) if s == "pending" => true,
                (5, _, true) => true,
                _ => false,
            };
            let marker = if active { "●" } else { " " };
            let style = if is_sel {
                Style::default()
                    .bg(app.selected_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(bc)),
                Span::styled(format!("{} ", marker), Style::default().fg(C_GREEN)),
                Span::styled(
                    format!("{:<18}", label),
                    Style::default().fg(if is_sel { C_WHITE } else { C_SUBTLE }),
                ),
                Span::styled(*desc, Style::default().fg(C_DIM)),
            ]))
            .style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(pv.dropdown_idx));

    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    format!(" filters — status: {} ", cur_status),
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
        &mut state,
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
        vec![ListItem::new(Line::from(Span::styled(
            "  (no remotes)",
            Style::default().fg(C_DIM),
        )))]
    } else {
        pv.remotes
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let is_sel = i == pv.remote_popup_idx;
                let is_cur = name == &pv.remote;
                let style = if is_sel {
                    Style::default()
                        .bg(app.selected_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let marker = if is_cur { "●" } else { " " };
                ListItem::new(Line::from(vec![
                    Span::styled(if is_sel { "▶ " } else { "  " }, Style::default().fg(bc)),
                    Span::styled(format!("{} ", marker), Style::default().fg(C_GREEN)),
                    Span::styled(
                        name.clone(),
                        Style::default().fg(if is_sel { C_WHITE } else { C_SUBTLE }),
                    ),
                ]))
                .style(style)
            })
            .collect()
    };

    let mut state = ListState::default();
    state.select(Some(
        pv.remote_popup_idx.min(pv.remotes.len().saturating_sub(1)),
    ));

    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(
                    " select remote ",
                    Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(C_WHITE)),
        ),
        popup,
        &mut state,
    );
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn status_color(s: &str) -> ratatui::style::Color {
    match s {
        "success" => C_GREEN,
        "failed" => C_RED,
        "running" => C_YELLOW,
        "pending" => C_DIM,
        "canceled" => C_DIM,
        _ => C_SUBTLE,
    }
}

/// Width of the key column (`"  description "` = 2 leading + 11 label + 1
/// trailing space = 14). Used both to render the prefix and to compute
/// the indent for word-wrapped continuation lines.
const KV_PREFIX_W: usize = 14;

/// Push a key/value pair into the detail panel `body`, wrapping the
/// value to subsequent lines indented to the value column when it
/// exceeds `value_w`. Without this, ratatui's block-level `Wrap` would
/// drop the second half of a long value back to column 0, where it
/// reads as part of the *next* kv entry — the "concatenated" look the
/// user reported on long runner descriptions.
fn kv(body: &mut Vec<Line<'static>>, k: &str, v: &str, vc: ratatui::style::Color, value_w: usize) {
    let chunks = wrap_words(v, value_w.max(8));
    let indent = " ".repeat(KV_PREFIX_W);
    for (i, chunk) in chunks.into_iter().enumerate() {
        let prefix = if i == 0 {
            format!("  {:<11} ", k)
        } else {
            indent.clone()
        };
        body.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(C_SUBTLE)),
            Span::styled(chunk, Style::default().fg(vc)),
        ]));
    }
}

/// Greedy word-wrap. Breaks on whitespace; words longer than `max`
/// are emitted whole on their own line (we don't hyphenate). Returns
/// at least one chunk (empty string when input is empty).
fn wrap_words(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > max {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
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
        Span::styled(
            "error",
            Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
        ),
    ]))];
    for chunk in err.chars().collect::<Vec<_>>().chunks(50) {
        let s: String = chunk.iter().collect();
        items.push(ListItem::new(Line::from(vec![Span::styled(
            format!("  {}", s),
            Style::default().fg(C_SUBTLE),
        )])));
    }
    items
}
