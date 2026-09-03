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
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::tui::app::{App, PlatformFocus, PlatformSubTab};
use crate::tui::theme;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // sub-tab strip
            Constraint::Length(1), // rule
            Constraint::Min(1),    // body
        ])
        .split(area);

    render_tab_strip(f, app, rows[0]);

    // Drill-down: the job log takes the whole body.
    if app.platform_view.focus == PlatformFocus::JobLog {
        theme::hrule_content(f, rows[1], &[]);
        render_job_log(f, app, rows[2]);
    } else {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(rows[2]);

        // The list carries the rule; the detail sits the other side of it.
        let divider = theme::divider_right();
        let list_pane = divider.inner(panes[0]);
        f.render_widget(divider, panes[0]);
        let spine = panes[0].right().saturating_sub(1);
        theme::hrule_content(f, rows[1], &[(spine, theme::Tick::Down)]);
        theme::tie_below(f, rows[2], &[spine]);

        render_list(f, app, list_pane);
        render_detail(f, app, panes[1]);
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

/// The five sub-tabs as a strip of words. What used to be a boxed `Tabs`
/// widget with the remote in its title: the remote moved to the list heading,
/// which has the room for it.
fn render_tab_strip(f: &mut Frame, app: &App, area: Rect) {
    let pv = &app.platform_view;
    let focused = !app.sidebar_focused;

    let titles = [
        "1 pipelines",
        "2 jobs",
        "3 releases",
        "4 packages",
        "5 runners",
    ];
    let active_idx = match pv.sub_tab {
        PlatformSubTab::Pipelines => 0,
        PlatformSubTab::Jobs => 1,
        PlatformSubTab::Releases => 2,
        PlatformSubTab::Packages => 3,
        PlatformSubTab::Runners => 4,
    };

    let tabs = Tabs::new(titles.to_vec())
        .select(active_idx)
        .style(Style::default().fg(theme::INK_DIM))
        .highlight_style(if focused {
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme::INK_DIM)
                .add_modifier(Modifier::BOLD)
        })
        .divider(Span::styled("·", Style::default().fg(theme::RULE)))
        .padding(" ", " ");

    f.render_widget(tabs, area);
}

/// Where the data comes from, and what is filtering it — said once, in the
/// list heading, rather than in a box title.
fn context_spans(pv: &crate::tui::app::PlatformState) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        if pv.platform.is_empty() {
            format!("  {}", pv.remote)
        } else {
            format!("  {} → {}/{}", pv.remote, pv.owner, pv.repo_name)
        },
        Style::default().fg(theme::INK_FAINT),
    )];
    if let Some(status) = &pv.filter_status {
        spans.push(Span::styled(
            format!("  status:{}", status),
            Style::default().fg(theme::INK_FAINT),
        ));
    }
    if pv.filter_branch_only {
        spans.push(Span::styled(
            "  branch-only",
            Style::default().fg(theme::INK_FAINT),
        ));
    }
    if pv.auto_refresh {
        spans.push(Span::styled("  ⟳ live", Style::default().fg(theme::OK)));
    }
    spans
}

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let [heading_row, body] = theme::heading_and_body(area);
    let pv = &app.platform_view;
    let active = !app.sidebar_focused && pv.focus == PlatformFocus::List;

    let (title, items, selected): (String, Vec<ListItem>, usize) = if pv.loading {
        (
            list_title(pv),
            vec![ListItem::new(Span::styled(
                "loading…",
                Style::default().fg(theme::INK_FAINT),
            ))],
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

    let (label, count) = split_title(&title);
    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title(&label, count, active));
    heading.extend(context_spans(pv));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);

    f.render_stateful_widget(
        List::new(items).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body,
        &mut state,
    );
}

/// `list_title` still yields the old " label (n) " string, which several
/// call sites share. The heading wants the two halves apart.
fn split_title(title: &str) -> (String, Option<usize>) {
    let t = title.trim();
    match (t.rfind('('), t.rfind(')')) {
        (Some(open), Some(close)) if close > open => {
            let n = t[open + 1..close].parse().ok();
            (t[..open].trim().to_string(), n)
        }
        _ => (t.to_string(), None),
    }
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
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let id = format!("#{}", p.id);
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(col(&id, 13), Style::default().fg(theme::accent(app))),
                Span::styled(
                    col(&p.status, 10),
                    Style::default().fg(status_color(&p.status)),
                ),
                Span::styled(col(&p.branch, 18), Style::default().fg(theme::INK)),
                Span::styled(
                    col(&short_time(&p.created_at), 18),
                    Style::default().fg(theme::INK_FAINT),
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
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let dur = j
                .duration_seconds
                .map(|s| format!("{}s", s as u64))
                .unwrap_or_else(|| "—".into());
            let id = format!("#{}", j.id);
            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(col(&id, 13), Style::default().fg(theme::accent(app))),
                Span::styled(
                    col(&j.status, 10),
                    Style::default().fg(status_color(&j.status)),
                ),
                Span::styled(col(&j.stage, 10), Style::default().fg(theme::INK_FAINT)),
                Span::styled(col(&j.name, 24), Style::default().fg(theme::INK)),
                Span::styled(col(&dur, 8), Style::default().fg(theme::INK_FAINT)),
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
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(col(&r.tag, 16), Style::default().fg(theme::OK)),
                Span::styled(col(&r.name, 28), Style::default().fg(theme::INK)),
                Span::styled(
                    col(&short_time(&r.created_at), 18),
                    Style::default().fg(theme::INK_FAINT),
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
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                theme::caret(app, is_sel),
                Span::styled(col(&p.name, 22), Style::default().fg(theme::INK)),
                Span::styled(col(&p.version, 14), Style::default().fg(theme::OK)),
                Span::styled(
                    col(&p.package_type, 10),
                    Style::default().fg(theme::INK_FAINT),
                ),
                Span::styled(
                    col(&short_time(&p.created_at), 18),
                    Style::default().fg(theme::INK_FAINT),
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
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let status_color = match r.status.as_str() {
                "online" | "active" => theme::OK,
                "offline" | "stale" => theme::INK_FAINT,
                "paused" => theme::INK_FAINT,
                _ => theme::INK_DIM,
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
                theme::WARN
            } else {
                theme::accent(app)
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
                theme::caret(app, is_sel),
                Span::styled(
                    format!("{} ", scope_glyph),
                    Style::default().fg(scope_color),
                ),
                Span::styled(col(scope_label, 7), Style::default().fg(scope_color)),
                Span::styled(col(&id_disp, 18), Style::default().fg(theme::accent(app))),
                Span::styled(col(&r.status, 10), Style::default().fg(status_color)),
                Span::styled(col(&r.description, 22), Style::default().fg(theme::INK)),
                Span::styled(col(&tags_str, 24), Style::default().fg(theme::INK_FAINT)),
            ]))
            .style(style)
        })
        .collect();
    (list_title(pv), items, pv.runners_idx)
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
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
                kv(
                    &mut body,
                    "id",
                    &format!("#{}", p.id),
                    theme::INK_DIM,
                    value_w,
                );
                kv(
                    &mut body,
                    "status",
                    &p.raw_status,
                    status_color(&p.status),
                    value_w,
                );
                kv(&mut body, "branch", &p.branch, theme::INK, value_w);
                kv(
                    &mut body,
                    "sha",
                    &short_sha(&p.sha),
                    theme::INK_FAINT,
                    value_w,
                );
                kv(
                    &mut body,
                    "created",
                    &p.created_at,
                    theme::INK_FAINT,
                    value_w,
                );
                kv(
                    &mut body,
                    "updated",
                    &p.updated_at,
                    theme::INK_FAINT,
                    value_w,
                );
                body.push(Line::from(""));
                kv(&mut body, "url", &p.web_url, theme::INK_DIM, value_w);
            }
        }
        PlatformSubTab::Jobs => {
            if let Some(j) = pv.jobs.get(pv.jobs_idx) {
                let dur = j
                    .duration_seconds
                    .map(|s| format!("{}s", s as u64))
                    .unwrap_or_default();
                kv(
                    &mut body,
                    "id",
                    &format!("#{}", j.id),
                    theme::INK_DIM,
                    value_w,
                );
                kv(
                    &mut body,
                    "pipeline",
                    &format!("#{}", j.pipeline_id),
                    theme::INK_DIM,
                    value_w,
                );
                kv(
                    &mut body,
                    "status",
                    &j.raw_status,
                    status_color(&j.status),
                    value_w,
                );
                kv(&mut body, "stage", &j.stage, theme::INK_FAINT, value_w);
                kv(&mut body, "name", &j.name, theme::INK, value_w);
                kv(&mut body, "duration", &dur, theme::INK_FAINT, value_w);
                body.push(Line::from(""));
                kv(&mut body, "url", &j.web_url, theme::INK_DIM, value_w);
            }
        }
        PlatformSubTab::Releases => {
            if let Some(r) = pv.releases.get(pv.releases_idx) {
                kv(&mut body, "tag", &r.tag, theme::OK, value_w);
                kv(&mut body, "name", &r.name, theme::INK, value_w);
                kv(
                    &mut body,
                    "created",
                    &r.created_at,
                    theme::INK_FAINT,
                    value_w,
                );
                body.push(Line::from(""));
                kv(&mut body, "url", &r.web_url, theme::INK_DIM, value_w);
            }
        }
        PlatformSubTab::Packages => {
            if let Some(p) = pv.packages.get(pv.packages_idx) {
                kv(&mut body, "name", &p.name, theme::INK, value_w);
                kv(&mut body, "version", &p.version, theme::OK, value_w);
                kv(
                    &mut body,
                    "type",
                    &p.package_type,
                    theme::INK_FAINT,
                    value_w,
                );
                kv(
                    &mut body,
                    "created",
                    &p.created_at,
                    theme::INK_FAINT,
                    value_w,
                );
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
                    "online" | "active" => theme::OK,
                    "offline" | "stale" => theme::INK_FAINT,
                    "paused" => theme::INK_FAINT,
                    _ => theme::INK_DIM,
                };
                kv(
                    &mut body,
                    "id",
                    &format!("#{}", r.id),
                    theme::INK_DIM,
                    value_w,
                );
                kv(&mut body, "status", &r.status, status_c, value_w);
                kv(
                    &mut body,
                    "description",
                    &r.description,
                    theme::INK,
                    value_w,
                );
                kv(&mut body, "type", &r.runner_type, theme::INK_FAINT, value_w);
                kv(&mut body, "os", &r.os, theme::INK_FAINT, value_w);
                if !r.ip_address.is_empty() {
                    kv(&mut body, "ip", &r.ip_address, theme::INK_FAINT, value_w);
                }
                if !r.version.is_empty() {
                    kv(&mut body, "version", &r.version, theme::INK_FAINT, value_w);
                }
                kv(&mut body, "tags", &tags, theme::INK_FAINT, value_w);
                if !r.web_url.is_empty() {
                    body.push(Line::from(""));
                    kv(&mut body, "url", &r.web_url, theme::INK_DIM, value_w);
                }
            }
        }
    }

    if body.is_empty() {
        body.push(Line::from(Span::styled(
            "no selection",
            Style::default().fg(theme::INK_FAINT),
        )));
    }

    // 0.7.26: detail panel only carries entity data — hints and
    // action results live in the global bottom hint (ui.rs) and the
    // App-wide `status_msg` line, like every other view does.
    // 0.7.28: no Paragraph wrap — we wrap manually above so the
    // continuation lines stay indented to the value column.
    let [heading_row, body_area] = theme::heading_and_body(area);
    let mut heading = vec![Span::raw(" ")];
    heading.extend(theme::panel_title("detail", None, false));
    f.render_widget(Paragraph::new(Line::from(heading)), heading_row);
    f.render_widget(
        Paragraph::new(body).block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body_area,
    );
}

fn render_job_log(f: &mut Frame, app: &App, area: Rect) {
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
    // Title: "job log · <live?> · <follow|manual>". Same theme::INK bold
    // as the rest of the focused-view titles (log.rs / branch.rs). The
    // live indicator is a coloured prefix span, not a colour-shifted
    // title — keeps the chrome consistent across sub-tabs.
    let mut title_spans: Vec<Span> = vec![
        Span::raw(" "),
        Span::styled(
            "job log",
            Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
        ),
    ];
    if pv.job_log_live {
        title_spans.push(Span::styled("  ● live", Style::default().fg(theme::OK)));
    }
    title_spans.push(Span::styled(
        format!("  {}", follow),
        Style::default().fg(theme::INK_FAINT),
    ));
    let _ = live;

    let [heading_row, body_area] = theme::heading_and_body(area);
    f.render_widget(Paragraph::new(Line::from(title_spans)), heading_row);
    f.render_widget(
        Paragraph::new(log)
            .scroll((pv.job_log_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        body_area,
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
                theme::BAD
            } else if is_sel {
                theme::INK
            } else {
                theme::INK_DIM
            };
            let style = if is_sel {
                Style::default()
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme::accent(app))),
                Span::styled(format!("{:<22}", label), Style::default().fg(label_color)),
                Span::styled(*desc, Style::default().fg(theme::INK_FAINT)),
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
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::INK)),
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
                    .bg(theme::selection(app))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if is_sel { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme::accent(app))),
                Span::styled(format!("{} ", marker), Style::default().fg(theme::OK)),
                Span::styled(
                    format!("{:<18}", label),
                    Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
                ),
                Span::styled(*desc, Style::default().fg(theme::INK_FAINT)),
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
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::INK)),
        ),
        popup,
        &mut state,
    );
}

fn render_remote_popup(f: &mut Frame, app: &App, area: Rect) {
    let pv = &app.platform_view;

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
            Style::default().fg(theme::INK_FAINT),
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
                        .bg(theme::selection(app))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let marker = if is_cur { "●" } else { " " };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        if is_sel { "▶ " } else { "  " },
                        Style::default().fg(theme::accent(app)),
                    ),
                    Span::styled(format!("{} ", marker), Style::default().fg(theme::OK)),
                    Span::styled(
                        name.clone(),
                        Style::default().fg(if is_sel { theme::INK } else { theme::INK_DIM }),
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
                    Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(app.border_type())
                .border_style(Style::default().fg(theme::INK)),
        ),
        popup,
        &mut state,
    );
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn status_color(s: &str) -> ratatui::style::Color {
    match s {
        "success" => theme::OK,
        "failed" => theme::BAD,
        "running" => theme::WARN,
        "pending" => theme::INK_FAINT,
        "canceled" => theme::INK_FAINT,
        _ => theme::INK_DIM,
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
            Span::styled(prefix, Style::default().fg(theme::INK_DIM)),
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
        Span::styled("  ✗ ", Style::default().fg(theme::BAD)),
        Span::styled(
            "error",
            Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
        ),
    ]))];
    for chunk in err.chars().collect::<Vec<_>>().chunks(50) {
        let s: String = chunk.iter().collect();
        items.push(ListItem::new(Line::from(vec![Span::styled(
            format!("  {}", s),
            Style::default().fg(theme::INK_DIM),
        )])));
    }
    items
}
