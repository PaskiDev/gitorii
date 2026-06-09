//! Platform (CI) view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_platform(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{PlatformFocus, PlatformSubTab};

    // Remote popup grabs everything when active.
    if app.platform_view.focus == PlatformFocus::RemotePopup {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k'))
                if app.platform_view.remote_popup_idx > 0 =>
            {
                app.platform_view.remote_popup_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.platform_view.remote_popup_idx + 1 < app.platform_view.remotes.len() =>
            {
                app.platform_view.remote_popup_idx += 1;
            }
            (_, KeyCode::Enter) => {
                if let Some(name) = app
                    .platform_view
                    .remotes
                    .get(app.platform_view.remote_popup_idx)
                    .cloned()
                {
                    app.platform_view.remote = name;
                    app.platform_view.active_pipeline_id = None;
                    app.platform_view.sub_tab = PlatformSubTab::Pipelines;
                }
                app.platform_view.focus = PlatformFocus::List;
                app.load_platform_active_sub_tab();
            }
            _ => {}
        }
        return None;
    }

    // 0.7.26 ops dropdown — single-key actions menu.
    if app.platform_view.focus == PlatformFocus::OpsDropdown {
        let ops = crate::tui::views::platform::ops_for(&app.platform_view);
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.platform_view.dropdown_idx > 0 => {
                app.platform_view.dropdown_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.platform_view.dropdown_idx + 1 < ops.len() =>
            {
                app.platform_view.dropdown_idx += 1;
            }
            (_, KeyCode::Enter) => {
                dispatch_ops_action(app);
                app.platform_view.focus = PlatformFocus::List;
            }
            _ => {}
        }
        return None;
    }

    // 0.7.26 filter dropdown — status + branch toggle.
    if app.platform_view.focus == PlatformFocus::FilterDropdown {
        let rows_len = crate::tui::views::platform::filters_for(&app.platform_view).len();
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.platform_view.dropdown_idx > 0 => {
                app.platform_view.dropdown_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.platform_view.dropdown_idx + 1 < rows_len =>
            {
                app.platform_view.dropdown_idx += 1;
            }
            (_, KeyCode::Enter) => {
                dispatch_filter_action(app);
                app.platform_view.focus = PlatformFocus::List;
                app.load_platform_active_sub_tab();
            }
            _ => {}
        }
        return None;
    }

    // JobLog drill-down: scroll, toggle live tail, open in $PAGER.
    if app.platform_view.focus == PlatformFocus::JobLog {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                app.platform_view.job_log_scroll =
                    app.platform_view.job_log_scroll.saturating_sub(1);
                app.platform_view.job_log_user_scrolled = true;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                app.platform_view.job_log_scroll =
                    app.platform_view.job_log_scroll.saturating_add(1);
                app.platform_view.job_log_user_scrolled = true;
            }
            (_, KeyCode::PageUp) => {
                app.platform_view.job_log_scroll =
                    app.platform_view.job_log_scroll.saturating_sub(20);
                app.platform_view.job_log_user_scrolled = true;
            }
            (_, KeyCode::PageDown) => {
                app.platform_view.job_log_scroll =
                    app.platform_view.job_log_scroll.saturating_add(20);
                app.platform_view.job_log_user_scrolled = true;
            }
            (_, KeyCode::Home) => {
                app.platform_view.job_log_scroll = 0;
                app.platform_view.job_log_user_scrolled = true;
            }
            (_, KeyCode::End) => {
                // Re-enable auto-follow: clear the manual-scroll flag and
                // let the next render snap to the bottom.
                app.platform_view.job_log_user_scrolled = false;
            }
            (_, KeyCode::Char('p')) => {
                // Toggle live tail. Only meaningful when the job is still
                // running, but we let the user force it on either way —
                // worst case is one extra fetch that returns the same text.
                app.platform_view.job_log_live = !app.platform_view.job_log_live;
                app.platform_view.job_log_last_poll_at = None;
            }
            (_, KeyCode::Char('o')) => {
                // Open the current log in $PAGER (less by default). The TUI
                // suspends crossterm + the alt screen while the pager is
                // running so the user can scroll/search freely.
                return Some(Action::OpenJobLogInPager);
            }
            _ => {}
        }
        return None;
    }

    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }

    // Ctrl-R reloads the active sub-tab.
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('r') {
        app.load_platform_active_sub_tab();
        return None;
    }

    match (key.modifiers, key.code) {
        // Sub-tab switching.
        (_, KeyCode::Char('1')) => {
            app.platform_view.sub_tab = PlatformSubTab::Pipelines;
            app.platform_view.active_pipeline_id = None;
            app.load_platform_pipelines();
        }
        (_, KeyCode::Char('2')) => {
            // Jobs without a pipeline context = not meaningful; user must
            // drill from a pipeline. We keep the empty list as a hint.
            app.platform_view.sub_tab = PlatformSubTab::Jobs;
            if app.platform_view.active_pipeline_id.is_none() {
                app.platform_view.jobs.clear();
            }
        }
        (_, KeyCode::Char('3')) => {
            app.platform_view.sub_tab = PlatformSubTab::Releases;
            app.load_platform_releases();
        }
        (_, KeyCode::Char('4')) => {
            app.platform_view.sub_tab = PlatformSubTab::Packages;
            app.load_platform_packages();
        }
        (_, KeyCode::Char('5')) => {
            app.platform_view.sub_tab = PlatformSubTab::Runners;
            app.load_platform_runners();
        }

        // Toggle auto-refresh polling on/off.
        (_, KeyCode::Char('p')) => {
            app.platform_view.auto_refresh = !app.platform_view.auto_refresh;
            // Reset the timer so the first refresh after enabling fires
            // immediately rather than waiting a full interval.
            app.platform_view.last_poll_at = None;
        }

        // Remote-selector popup.
        (_, KeyCode::Char('r')) => {
            // Position cursor on the current remote so Enter is a no-op
            // if the user just wants to peek.
            let cur = app.platform_view.remote.clone();
            app.platform_view.remote_popup_idx = app
                .platform_view
                .remotes
                .iter()
                .position(|n| n == &cur)
                .unwrap_or(0);
            app.platform_view.focus = PlatformFocus::RemotePopup;
        }

        // List navigation.
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            let idx = current_list_idx_mut(app);
            if *idx > 0 {
                *idx -= 1;
            }
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            let len = current_list_len(app);
            let idx = current_list_idx_mut(app);
            if *idx + 1 < len {
                *idx += 1;
            }
        }

        // 0.7.26: ops + filter dropdowns. Single key opens a menu of
        // the contextual actions / filters for the current sub-tab.
        // The dropdown handler (above) dispatches Enter/Esc/arrows.
        (_, KeyCode::Char('o'))
            if !app.platform_view.action_in_flight
                && !crate::tui::views::platform::ops_for(&app.platform_view).is_empty() =>
        {
            app.platform_view.focus = PlatformFocus::OpsDropdown;
            app.platform_view.dropdown_idx = 0;
        }
        (_, KeyCode::Char('f')) => {
            app.platform_view.focus = PlatformFocus::FilterDropdown;
            app.platform_view.dropdown_idx = 0;
        }

        // Drill-down.
        (_, KeyCode::Enter) => match app.platform_view.sub_tab {
            PlatformSubTab::Pipelines => {
                if let Some(p) = app
                    .platform_view
                    .pipelines
                    .get(app.platform_view.pipelines_idx)
                {
                    if let Ok(pid) = p.id.parse::<u64>() {
                        app.platform_view.active_pipeline_id = Some(pid);
                        app.platform_view.sub_tab = PlatformSubTab::Jobs;
                        app.platform_view.focus = PlatformFocus::JobsOfPipeline;
                        app.platform_view.jobs_idx = 0;
                        app.load_platform_jobs_for_pipeline(p.id.clone());
                    } else {
                        // GitHub workflow runs use string ids — pass through.
                        let id = p.id.clone();
                        app.platform_view.active_pipeline_id = None;
                        app.platform_view.sub_tab = PlatformSubTab::Jobs;
                        app.platform_view.focus = PlatformFocus::JobsOfPipeline;
                        app.platform_view.jobs_idx = 0;
                        app.load_platform_jobs_for_pipeline(id);
                    }
                }
            }
            PlatformSubTab::Jobs => {
                if let Some(j) = app.platform_view.jobs.get(app.platform_view.jobs_idx) {
                    app.platform_view.focus = PlatformFocus::JobLog;
                    // Live tail only makes sense while the job is still
                    // producing output. Terminal statuses get a static
                    // snapshot; non-terminal ones enable auto-poll.
                    app.platform_view.job_log_status = j.status.clone();
                    app.platform_view.job_log_live =
                        matches!(j.status.as_str(), "running" | "pending");
                    app.platform_view.job_log_last_poll_at = None;
                    app.platform_view.job_log_user_scrolled = false;
                    app.load_platform_job_log(j.id.clone());
                }
            }
            _ => {}
        },
        _ => {}
    }
    None
}

pub(super) fn dispatch_ops_action(app: &mut App) {
    use crate::tui::app::PlatformSubTab;
    let idx = app.platform_view.dropdown_idx;
    match app.platform_view.sub_tab {
        PlatformSubTab::Pipelines => {
            let id = app
                .platform_view
                .pipelines
                .get(app.platform_view.pipelines_idx)
                .map(|p| p.id.clone());
            if let Some(id) = id {
                match idx {
                    0 => app.action_pipeline_cancel(id),
                    1 => app.action_pipeline_retry(id),
                    _ => {}
                }
            }
        }
        PlatformSubTab::Jobs => {
            let id = app
                .platform_view
                .jobs
                .get(app.platform_view.jobs_idx)
                .map(|j| j.id.clone());
            if let Some(id) = id {
                match idx {
                    0 => app.action_job_cancel(id),
                    1 => app.action_job_retry(id),
                    2 => app.action_job_artifacts(id),
                    _ => {}
                }
            }
        }
        PlatformSubTab::Runners => {
            // 0.8.1 — local-docker runners aren't reachable via the
            // platform API. Until v0.8.2 wires the in-TUI docker
            // verbs, surface a clear pointer to the CLI commands
            // instead of failing the platform API call.
            let row = app
                .platform_view
                .runners
                .get(app.platform_view.runners_idx)
                .cloned();
            if let Some(r) = row {
                if r.runner_type == "local-docker" {
                    app.set_status(format!(
                        "this is a local container — run from shell: torii runner start/stop/logs/destroy {}",
                        r.id
                    ));
                    return;
                }
                match idx {
                    0 => app.action_runner_pause(r.id),
                    1 => app.action_runner_resume(r.id),
                    2 => app.action_runner_reset_token(r.id),
                    3 => app.action_runner_remove(r.id),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

pub(super) fn dispatch_filter_action(app: &mut App) {
    match app.platform_view.dropdown_idx {
        0 => app.platform_view.filter_status = None,
        1 => app.platform_view.filter_status = Some("running".to_string()),
        2 => app.platform_view.filter_status = Some("failed".to_string()),
        3 => app.platform_view.filter_status = Some("success".to_string()),
        4 => app.platform_view.filter_status = Some("pending".to_string()),
        5 => app.platform_view.filter_branch_only = !app.platform_view.filter_branch_only,
        _ => {}
    }
}

pub(super) fn current_list_idx_mut(app: &mut App) -> &mut usize {
    use crate::tui::app::PlatformSubTab;
    match app.platform_view.sub_tab {
        PlatformSubTab::Pipelines => &mut app.platform_view.pipelines_idx,
        PlatformSubTab::Jobs => &mut app.platform_view.jobs_idx,
        PlatformSubTab::Releases => &mut app.platform_view.releases_idx,
        PlatformSubTab::Packages => &mut app.platform_view.packages_idx,
        PlatformSubTab::Runners => &mut app.platform_view.runners_idx,
    }
}

pub(super) fn current_list_len(app: &App) -> usize {
    use crate::tui::app::PlatformSubTab;
    match app.platform_view.sub_tab {
        PlatformSubTab::Pipelines => app.platform_view.pipelines.len(),
        PlatformSubTab::Jobs => app.platform_view.jobs.len(),
        PlatformSubTab::Releases => app.platform_view.releases.len(),
        PlatformSubTab::Packages => app.platform_view.packages.len(),
        PlatformSubTab::Runners => app.platform_view.runners.len(),
    }
}
