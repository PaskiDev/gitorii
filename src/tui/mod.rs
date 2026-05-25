pub mod app;
pub mod events;
pub mod ui;
pub mod views;
pub mod picker;

use std::io;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, View, SyncOp, SyncStatus, EventKind};
use events::{Action, EventHandler};

fn ssh_to_https(url: &str) -> String {
    // git@github.com:owner/repo.git → https://github.com/owner/repo
    if let Some(rest) = url.strip_prefix("git@") {
        let s = rest.replacen(':', "/", 1);
        let s = s.strip_suffix(".git").unwrap_or(&s);
        return format!("https://{}", s);
    }
    url.strip_suffix(".git").unwrap_or(url).to_string()
}

pub fn run() -> crate::error::Result<()> {
    run_with_view(app::View::Dashboard)
}

pub fn run_with_workspace(ws_name: String) -> crate::error::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new()?;
    load_auto_interval(&mut app);
    app.active_workspace = Some(ws_name);
    app.go_to(app::View::Workspace);
    let mut events = EventHandler::new();

    let result = run_loop(&mut terminal, &mut app, &mut events);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    result
}

pub fn run_with_view(initial_view: app::View) -> crate::error::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new()?;
    load_auto_interval(&mut app);
    app.go_to(initial_view);
    let mut events = EventHandler::new();

    let result = run_loop(&mut terminal, &mut app, &mut events);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events: &mut EventHandler,
) -> crate::error::Result<()> {
    loop {
        app.tick = app.tick.wrapping_add(1);

        // Poll update check result (background thread)
        if let Some(rx) = &app.update_rx {
            if let Ok(v) = rx.try_recv() {
                app.update_available = Some(v);
                app.update_rx = None;
            }
        }

        // Poll sync result from background thread
        if let Some(rx) = &app.sync_rx {
            if let Ok(result) = rx.try_recv() {
                app.sync_view.status = match result {
                    Ok(msg) => SyncStatus::Done(msg.clone()),
                    Err(e)  => SyncStatus::Error(e.to_string().lines().next().unwrap_or("error").to_string()),
                };
                let (msg, kind) = match &app.sync_view.status {
                    SyncStatus::Done(m)  => (m.clone(), EventKind::Success),
                    SyncStatus::Error(e) => (e.clone(), EventKind::Error),
                    _                    => ("sync completed".to_string(), EventKind::Info),
                };
                app.log_event(msg, kind);
                app.sync_rx = None;
                app.refresh()?;
            }
        }

        // Poll PR load result from background thread
        if let Some(rx) = &app.pr_rx {
            if let Ok(result) = rx.try_recv() {
                app.pr_rx = None;
                match result {
                    Ok(prs) => {
                        app.pr_view.prs = prs;
                        app.pr_view.idx = 0;
                        app.pr_view.loading = false;
                    }
                    Err(e) => {
                        app.pr_view.error = Some(e.to_string());
                        app.pr_view.loading = false;
                    }
                }
            }
        }

        // Poll Issue load result from background thread
        if let Some(rx) = &app.issue_rx {
            if let Ok(result) = rx.try_recv() {
                app.issue_rx = None;
                match result {
                    Ok(issues) => {
                        app.issue_view.issues = issues;
                        app.issue_view.idx = 0;
                        app.issue_view.loading = false;
                    }
                    Err(e) => {
                        app.issue_view.error = Some(e.to_string());
                        app.issue_view.loading = false;
                    }
                }
            }
        }

        // ── Platform view (0.7.12) ───────────────────────────────────────────
        // Each sub-tab has its own background loader. The list is cleared
        // by the loader before spawning; here we just commit the result
        // (or expose the error).
        if let Some(rx) = &app.platform_pipelines_rx {
            if let Ok(result) = rx.try_recv() {
                app.platform_pipelines_rx = None;
                app.platform_view.loading = false;
                match result {
                    Ok(items) => {
                        app.platform_view.pipelines = items;
                        app.platform_view.pipelines_idx = 0;
                    }
                    Err(e) => app.platform_view.error = Some(e.to_string()),
                }
            }
        }
        if let Some(rx) = &app.platform_jobs_rx {
            if let Ok(result) = rx.try_recv() {
                app.platform_jobs_rx = None;
                app.platform_view.loading = false;
                match result {
                    Ok(items) => {
                        app.platform_view.jobs = items;
                        app.platform_view.jobs_idx = 0;
                    }
                    Err(e) => app.platform_view.error = Some(e.to_string()),
                }
            }
        }
        if let Some(rx) = &app.platform_releases_rx {
            if let Ok(result) = rx.try_recv() {
                app.platform_releases_rx = None;
                app.platform_view.loading = false;
                match result {
                    Ok(items) => {
                        app.platform_view.releases = items;
                        app.platform_view.releases_idx = 0;
                    }
                    Err(e) => app.platform_view.error = Some(e.to_string()),
                }
            }
        }
        if let Some(rx) = &app.platform_packages_rx {
            if let Ok(result) = rx.try_recv() {
                app.platform_packages_rx = None;
                app.platform_view.loading = false;
                match result {
                    Ok(items) => {
                        app.platform_view.packages = items;
                        app.platform_view.packages_idx = 0;
                    }
                    Err(e) => app.platform_view.error = Some(e.to_string()),
                }
            }
        }
        if let Some(rx) = &app.platform_job_log_rx {
            if let Ok(result) = rx.try_recv() {
                app.platform_job_log_rx = None;
                app.platform_view.loading = false;
                match result {
                    Ok(log) => app.platform_view.job_log = Some(log),
                    Err(e) => app.platform_view.error = Some(e.to_string()),
                }
            }
        }

        // Auto-snapshot check
        if let Some(interval_secs) = app.snapshot_view.auto_interval.secs() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_secs();
            if now.saturating_sub(app.snapshot_view.last_auto_snapshot) >= interval_secs {
                app.snapshot_view.last_auto_snapshot = now;
                let _ = create_snapshot_with_name(app, "auto");
                app.log_event("auto-snapshot created", EventKind::Info);
            }
        }

        terminal.draw(|f| ui::render(f, app))?;

        if let Some(action) = events.next(app)? {
            match action {
                Action::Quit => break,

                Action::SidebarUp => { app.sidebar_up(); }
                Action::SidebarDown => { app.sidebar_down(); }
                Action::SidebarEnter => {
                    app.sidebar_focused = false;
                    app.sidebar_enter();
                }

                Action::Refresh => {
                    app.refresh()?;
                    app.set_status("refreshed");
                    app.log_event("repo refreshed", EventKind::Info);
                }

                Action::StageFile => {
                    stage_selected(app)?;
                    app.refresh()?;
                }

                Action::UnstageFile => {
                    unstage_selected(app)?;
                    app.refresh()?;
                }

                Action::CommitConfirm => {
                    let msg = app.commit_view.message.trim().to_string();
                    let amend = app.commit_view.amend;
                    if !msg.is_empty() && (amend || !app.staged.is_empty()) {
                        if amend {
                            let output = std::process::Command::new("torii")
                                .args(["save", "--amend", "-m", &msg])
                                .current_dir(&app.repo_path)
                                .output();
                            let is_ok = matches!(&output, Ok(o) if o.status.success());
                            let short = truncate_msg(&msg, 40);
                            let log_msg = if is_ok { format!("amend: {}", short) } else { "amend failed".to_string() };
                            app.log_event(&log_msg, if is_ok { EventKind::Success } else { EventKind::Error });
                            if is_ok {
                                app.commit_view.message.clear();
                                app.commit_view.cursor = 0;
                                app.commit_view.amend = false;
                                app.go_back();
                                app.refresh()?;
                            }
                        } else {
                            commit_staged(app, &msg)?;
                            app.commit_view.message.clear();
                            app.commit_view.cursor = 0;
                            app.go_back();
                            app.refresh()?;
                            let short = truncate_msg(&msg, 40);
                            app.log_event(format!("commit: {}", short), EventKind::Success);
                            app.set_status(format!("committed: {}", short));
                        }
                    }
                }

                Action::BranchCheckout => {
                    if let Some(b) = app.branch_view.branches.get(app.branch_view.idx) {
                        let name = b.name.clone();
                        let is_remote = b.is_remote;
                        let repo = crate::core::GitRepo::open(&app.repo_path);
                        let result = match repo {
                            Ok(r) => if is_remote {
                                r.checkout_remote_branch(&name)
                            } else {
                                r.switch_branch(&name)
                            },
                            Err(e) => Err(e),
                        };
                        let msg = match result {
                            Ok(_)  => format!("checkout: {}", name),
                            Err(e) => format!("checkout failed: {}", e),
                        };
                        let is_ok = msg.starts_with("checkout:");
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        app.branch_view.status = if is_ok { None } else { Some(msg) };
                        app.go_to(View::Branch);
                        app.refresh()?;
                    }
                }

                Action::BranchDelete => {
                    if let Some(b) = app.branch_view.branches.get(app.branch_view.idx) {
                        let name = b.name.clone();
                        let result = crate::core::GitRepo::open(&app.repo_path)
                            .and_then(|r| r.delete_branch(&name));
                        let msg = match result {
                            Ok(_)  => format!("deleted: {}", name),
                            Err(e) => format!("delete failed: {}", e),
                        };
                        let kind = if msg.starts_with("deleted") { EventKind::Success } else { EventKind::Error };
                        app.log_event(&msg, kind);
                        app.branch_view.status = None;
                        app.go_to(View::Branch);
                    }
                }

                Action::BranchCreate => {
                    let name = app.branch_view.new_name.trim().to_string();
                    if !name.is_empty() {
                        let result = crate::core::GitRepo::open(&app.repo_path)
                            .and_then(|r| r.create_branch(&name).and_then(|_| r.switch_branch(&name)));
                        let msg = match result {
                            Ok(_)  => format!("created: {}", name),
                            Err(e) => format!("create failed: {}", e),
                        };
                        let is_ok = msg.starts_with("created");
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        app.branch_view.new_name.clear();
                        app.branch_view.status = if is_ok { None } else { Some(msg) };
                        app.go_to(View::Branch);
                        app.refresh()?;
                    }
                }

                Action::BranchPush => {
                    let result = crate::core::GitRepo::open(&app.repo_path)
                        .and_then(|r| r.push(false));
                    let msg = match result {
                        Ok(_)  => format!("pushed: {}", app.branch),
                        Err(e) => format!("push failed: {}", e),
                    };
                    let is_ok = msg.starts_with("pushed");
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    app.branch_view.status = if is_ok { None } else { Some(msg) };
                    app.refresh()?;
                }

                Action::SnapshotRestore => {
                    restore_selected_snapshot(app)?;
                    app.go_back();
                    app.refresh()?;
                }

                Action::SnapshotCreate => {
                    create_snapshot(app)?;
                    app.log_event("snapshot created", EventKind::Success);
                }

                Action::SnapshotDelete => {
                    delete_snapshot(app)?;
                    app.log_event("snapshot deleted", EventKind::Info);
                }

                Action::SnapshotSaveInterval => {
                    save_auto_interval(app);
                    app.log_event(
                        format!("auto-snapshot: {}", app.snapshot_view.auto_interval.label()),
                        EventKind::Info,
                    );
                }

                Action::OpenDiffFromLog => {
                    app.go_to_diff_from_log();
                }

                Action::LogCopyHash => {
                    if let Some(commit) = app.commits.get(app.log.idx) {
                        let hash = commit.full_hash.clone();
                        let copied = copy_to_clipboard(&hash);
                        let msg = if copied {
                            format!("copied: {}", &hash[..7])
                        } else {
                            "clipboard not available".to_string()
                        };
                        let kind = if copied { EventKind::Success } else { EventKind::Error };
                        app.log_event(&msg, kind);
                        app.set_status(msg);
                    }
                }

                Action::SyncRun => {
                    spawn_sync(app);
                }

                Action::TagCreate => {
                    let name = app.tag_view.new_name.trim().to_string();
                    let message = app.tag_view.new_message.trim().to_string();
                    app.tag_view.new_name.clear();
                    app.tag_view.new_message.clear();
                    if !name.is_empty() {
                        let result = std::process::Command::new("torii")
                            .args(["tag", "create", &name, "-m", &message])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let is_ok = result.map(|s| s.success()).unwrap_or(false);
                        let msg = if is_ok { format!("created tag: {}", name) } else { format!("failed to create tag: {}", name) };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        app.refresh()?;
                    }
                }

                Action::TagPush => {
                    if let Some(tag) = app.tag_view.tags.get(app.tag_view.idx) {
                        let name = tag.name.clone();
                        let result = std::process::Command::new("torii")
                            .args(["tag", "push", &name])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let is_ok = result.map(|s| s.success()).unwrap_or(false);
                        let msg = if is_ok { format!("pushed tag: {}", name) } else { format!("failed to push tag: {}", name) };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    }
                }

                Action::TagDelete => {
                    if let Some(tag) = app.tag_view.tags.get(app.tag_view.idx) {
                        let name = tag.name.clone();
                        let result = std::process::Command::new("torii")
                            .args(["tag", "delete", &name])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let is_ok = result.map(|s| s.success()).unwrap_or(false);
                        let msg = if is_ok { format!("deleted tag: {}", name) } else { format!("failed to delete tag: {}", name) };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        app.refresh()?;
                    }
                }

                Action::HistoryCherryPick => {
                    if let Some(entry) = app.history_view.reflog.get(app.history_view.idx) {
                        let hash = entry.id.clone();
                        let ok = std::process::Command::new("torii")
                            .args(["cherry-pick", &hash])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status().map(|s| s.success()).unwrap_or(false);
                        let msg = if ok { format!("cherry-picked: {}", hash) } else { format!("cherry-pick failed: {}", hash) };
                        app.log_event(&msg, if ok { EventKind::Success } else { EventKind::Error });
                        app.refresh()?;
                    }
                }

                Action::HistoryRebase => {
                    let target = app.history_view.input.trim().to_string();
                    app.history_view.input.clear();
                    if !target.is_empty() {
                        let ok = std::process::Command::new("torii")
                            .args(["history", "rebase", &target])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status().map(|s| s.success()).unwrap_or(false);
                        let msg = if ok { format!("rebased onto: {}", target) } else { format!("rebase failed onto: {}", target) };
                        app.log_event(&msg, if ok { EventKind::Success } else { EventKind::Error });
                        app.refresh()?;
                    }
                }

                Action::HistoryScan => {
                    let full = app.history_view.scan_full;
                    let mut cmd = std::process::Command::new("torii");
                    cmd.args(if full { vec!["scan", "--history"] } else { vec!["scan"] })
                        .current_dir(&app.repo_path)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
                    let msg = if ok { "scan complete — no secrets found".to_string() } else { "scan found issues — check event log".to_string() };
                    app.log_event(&msg, if ok { EventKind::Success } else { EventKind::Error });
                }

                Action::HistoryClean => {
                    let ok = std::process::Command::new("torii")
                        .args(["history", "clean"])
                        .current_dir(&app.repo_path)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status().map(|s| s.success()).unwrap_or(false);
                    let msg = if ok { "history cleaned".to_string() } else { "clean failed".to_string() };
                    app.log_event(&msg, if ok { EventKind::Success } else { EventKind::Error });
                    app.refresh()?;
                }

                Action::HistoryRemoveFile => {
                    let path = app.history_view.input.trim().to_string();
                    app.history_view.input.clear();
                    if !path.is_empty() {
                        let ok = std::process::Command::new("torii")
                            .args(["history", "remove-file", &path])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status().map(|s| s.success()).unwrap_or(false);
                        let msg = if ok { format!("removed file from history: {}", path) } else { format!("remove-file failed: {}", path) };
                        app.log_event(&msg, if ok { EventKind::Success } else { EventKind::Error });
                        app.refresh()?;
                    }
                }

                Action::HistoryRewrite => {
                    let start = app.history_view.input.trim().to_string();
                    let end = app.history_view.input2.trim().to_string();
                    app.history_view.input.clear();
                    app.history_view.input2.clear();
                    if !start.is_empty() && !end.is_empty() {
                        let ok = std::process::Command::new("torii")
                            .args(["history", "rewrite", &start, &end])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status().map(|s| s.success()).unwrap_or(false);
                        let msg = if ok { format!("rewrote dates: {} → {}", start, end) } else { "rewrite failed".to_string() };
                        app.log_event(&msg, if ok { EventKind::Success } else { EventKind::Error });
                        app.refresh()?;
                    }
                }

                Action::HistoryBlame => {
                    let file = app.history_view.input.trim().to_string();
                    app.history_view.input.clear();
                    if !file.is_empty() {
                        let output = std::process::Command::new("torii")
                            .args(["blame", &file])
                            .current_dir(&app.repo_path)
                            .output();
                        match output {
                            Ok(o) if o.status.success() => {
                                let text = String::from_utf8_lossy(&o.stdout).to_string();
                                app.history_view.input = text;
                                app.log_event(&format!("blame: {}", file), EventKind::Info);
                            }
                            _ => { app.log_event(&format!("blame failed: {}", file), EventKind::Error); }
                        }
                    }
                }

                Action::RemoteInfo => {
                    if let Some(remote) = app.remote_view.remotes.get(app.remote_view.idx) {
                        app.remote_view.status = Some(format!("{} → {}", remote.name, remote.url));
                    }
                }

                Action::RemoteFetch => {
                    let output = std::process::Command::new("torii")
                        .args(["sync", "--fetch"])
                        .current_dir(&app.repo_path)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    let is_ok = matches!(output, Ok(s) if s.success());
                    let msg = if is_ok { "fetched from remote".to_string() }
                              else { "fetch failed".to_string() };
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    app.remote_view.status = if is_ok { None } else { Some(msg) };
                }

                Action::RemoteAdd => {
                    let name = app.remote_view.new_name.clone();
                    let url  = app.remote_view.new_url.clone();
                    app.remote_view.new_name.clear();
                    app.remote_view.new_url.clear();
                    app.remote_view.confirm = app::RemoteConfirm::None;
                    let Ok(repo) = git2::Repository::discover(&app.repo_path) else { break; };
                    let msg = match repo.remote(&name, &url) {
                        Ok(_)  => { format!("added remote: {}", name) }
                        Err(e) => { format!("add remote failed: {}", e.message()) }
                    };
                    let is_ok = msg.starts_with("added");
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    if is_ok { app.reload_remotes(); } else { app.remote_view.status = Some(msg); }
                }

                Action::RemoteRename => {
                    let old_name = app.remote_view.remotes.get(app.remote_view.idx)
                        .map(|r| r.git_name.clone())
                        .unwrap_or_default();
                    let new_name = app.remote_view.new_name.clone();
                    app.remote_view.new_name.clear();
                    app.remote_view.confirm = app::RemoteConfirm::None;
                    if old_name.is_empty() || new_name.is_empty() { break; }
                    let Ok(repo) = git2::Repository::discover(&app.repo_path) else { break; };
                    let msg = match repo.remote_rename(&old_name, &new_name) {
                        Ok(_)  => format!("renamed: {} → {}", old_name, new_name),
                        Err(e) => format!("rename failed: {}", e.message()),
                    };
                    let is_ok = msg.starts_with("renamed");
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    if is_ok { app.reload_remotes(); } else { app.remote_view.status = Some(msg); }
                }

                Action::RemoteEditUrl => {
                    let name = app.remote_view.remotes.get(app.remote_view.idx)
                        .map(|r| r.git_name.clone())
                        .unwrap_or_default();
                    let new_url = app.remote_view.new_url.clone();
                    app.remote_view.new_url.clear();
                    app.remote_view.confirm = app::RemoteConfirm::None;
                    if name.is_empty() || new_url.is_empty() { break; }
                    let Ok(repo) = git2::Repository::discover(&app.repo_path) else { break; };
                    let msg = match repo.remote_set_url(&name, &new_url) {
                        Ok(_)  => format!("url updated: {}", new_url),
                        Err(e) => format!("edit url failed: {}", e.message()),
                    };
                    let is_ok = msg.starts_with("url updated");
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    if is_ok { app.reload_remotes(); } else { app.remote_view.status = Some(msg); }
                }

                Action::RemoteRemove => {
                    let name = app.remote_view.remotes.get(app.remote_view.idx)
                        .map(|r| r.git_name.clone())
                        .unwrap_or_default();
                    if name.is_empty() { break; }
                    let Ok(repo) = git2::Repository::discover(&app.repo_path) else { break; };
                    let msg = match repo.remote_delete(&name) {
                        Ok(_)  => format!("removed remote: {}", name),
                        Err(e) => format!("remove failed: {}", e.message()),
                    };
                    let is_ok = msg.starts_with("removed");
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    if is_ok { app.reload_remotes(); } else { app.remote_view.status = Some(msg); }
                }

                Action::RemoteOpenBrowser => {
                    let raw_url = if app.remote_view.selected_is_mirror() {
                        app.remote_view.selected_mirror().map(|m| m.url.clone())
                    } else {
                        app.remote_view.selected_remote().map(|r| r.url.clone())
                    };
                    if let Some(raw) = raw_url {
                        let url = ssh_to_https(&raw);
                        let _ = std::process::Command::new("xdg-open")
                            .arg(&url)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                        app.log_event(&format!("opened: {}", url), EventKind::Info);
                    }
                }

                Action::MirrorSync => {
                    let status = std::process::Command::new("torii")
                        .args(["mirror", "sync"])
                        .current_dir(&app.repo_path)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    let is_ok = matches!(status, Ok(s) if s.success());
                    let msg = if is_ok { "synced all mirrors".to_string() } else { "sync failed".to_string() };
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    app.mirror_view.status = Some(msg);
                }

                Action::MirrorSyncForce => {
                    let status = std::process::Command::new("torii")
                        .args(["mirror", "sync", "--force"])
                        .current_dir(&app.repo_path)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    let is_ok = matches!(status, Ok(s) if s.success());
                    let msg = if is_ok { "force synced all mirrors".to_string() } else { "force sync failed".to_string() };
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    app.mirror_view.status = Some(msg);
                }

                Action::MirrorRemove => {
                    if let Some(m) = app.remote_view.selected_mirror().map(|m| (m.platform.to_lowercase(), m.account.clone())) {
                        let (platform, account) = m;
                        let status = std::process::Command::new("torii")
                            .args(["mirror", "remove", &platform, &account])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let is_ok = matches!(status, Ok(s) if s.success());
                        let msg = if is_ok { format!("removed mirror: {}", platform) }
                                  else { "remove failed".to_string() };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        if is_ok {
                            app.reload_remotes();
                        } else {
                            app.remote_view.status = Some(msg);
                        }
                    }
                }

                Action::MirrorRename => {
                    let new_name = app.remote_view.new_name.clone();
                    app.remote_view.new_name.clear();
                    app.remote_view.confirm = app::RemoteConfirm::None;
                    if new_name.is_empty() { break; }
                    let mirrors_path = std::path::PathBuf::from(&app.repo_path).join(".torii/mirrors.json");
                    let msg = (|| -> Result<String, String> {
                        let content = std::fs::read_to_string(&mirrors_path).map_err(|e| e.to_string())?;
                        let mut json: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
                        let mirrors = json["mirrors"].as_array_mut().ok_or("no mirrors array")?;
                        let mirror_idx = app.remote_view.idx.saturating_sub(app.remote_view.remotes.len());
                        let m = mirrors.get_mut(mirror_idx).ok_or("mirror not found")?;
                        let old_name = m["name"].as_str().unwrap_or("?").to_string();
                        m["name"] = serde_json::Value::String(new_name.clone());
                        let out = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
                        std::fs::write(&mirrors_path, out).map_err(|e| e.to_string())?;
                        Ok(format!("renamed: {} → {}", old_name, new_name))
                    })().unwrap_or_else(|e| format!("rename failed: {}", e));
                    let is_ok = msg.starts_with("renamed");
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    if is_ok { app.reload_remotes(); } else { app.remote_view.status = Some(msg); }
                }

                Action::MirrorAdd => {
                    let platform   = app.remote_view.new_mirror_platform.clone();
                    let account    = app.remote_view.new_mirror_account.clone();
                    let repo       = app.remote_view.new_mirror_repo.clone();
                    let is_primary = app.remote_view.new_mirror_type == 1;
                    app.remote_view.new_mirror_platform.clear();
                    app.remote_view.new_mirror_account.clear();
                    app.remote_view.new_mirror_repo.clear();
                    app.remote_view.new_mirror_type = 0;
                    let mut args: Vec<&str> = vec!["mirror", "add", &platform, "user", &account, &repo];
                    if is_primary { args.push("--primary"); }
                    let status = std::process::Command::new("torii")
                        .args(&args)
                        .current_dir(&app.repo_path)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    let is_ok = matches!(status, Ok(s) if s.success());
                    let msg = if is_ok { format!("added mirror: {}/{}", platform, repo) }
                              else { "add mirror failed".to_string() };
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    if is_ok { app.reload_remotes(); } else { app.remote_view.status = Some(msg); }
                }

                Action::MirrorSetPrimary => {
                    if let Some((platform, account)) = app.remote_view.selected_mirror()
                        .map(|m| (m.platform.to_lowercase(), m.account.clone()))
                    {
                        let status = std::process::Command::new("torii")
                            .args(["mirror", "promote", &platform, &account])
                            .current_dir(&app.repo_path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let is_ok = matches!(status, Ok(s) if s.success());
                        let msg = if is_ok { format!("promoted to primary: {}/{}", platform, account) }
                                  else { "promote failed".to_string() };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        if is_ok { app.reload_remotes(); } else { app.remote_view.status = Some(msg); }
                    }
                }

                Action::MirrorSyncOne => {}

                Action::ConfigEdit => {} // editing already started in handle_config

                Action::ConfigSave => {
                    let idx = app.config_view.idx;
                    if let Some(entry) = app.config_view.entries.get(idx) {
                        let key = entry.key.clone();
                        let val = app.config_view.edit_buf.clone();
                        let mut cmd_args = vec!["config", "set", &key, &val];
                        if app.config_view.scope == app::ConfigScope::Local {
                            cmd_args.push("--local");
                        }
                        let output = std::process::Command::new("torii")
                            .args(&cmd_args)
                            .output();
                        app.config_view.editing = false;
                        app.config_view.status = Some(match output {
                            Ok(o) if o.status.success() => format!("saved: {} = {}", key, val),
                            Ok(o) => {
                                let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                                app.log_event(&format!("config save error: {}", err), EventKind::Error);
                                format!("failed: {}", err.lines().next().unwrap_or("unknown error"))
                            }
                            Err(e) => format!("failed: {}", e),
                        });
                        app.go_to(View::Config);
                    }
                }

                Action::ConfigToggleScope => {
                    app.config_view.scope = if app.config_view.scope == app::ConfigScope::Global {
                        app::ConfigScope::Local
                    } else {
                        app::ConfigScope::Global
                    };
                    app.go_to(View::Config);
                }

                Action::SettingsToggle => {
                    use crate::graph::GraphStyle;
                    let idx = app.settings_view.idx;
                    match idx {
                        0 => {
                            app.settings.border_style = if app.settings.border_style == app::BorderStyle::Rounded {
                                app::BorderStyle::Sharp
                            } else {
                                app::BorderStyle::Rounded
                            };
                        }
                        3 => {
                            // Cycle: Curves → Heavy → Bubbles → BubblesX → Ascii → Curves
                            app.settings.graph_style = match app.settings.graph_style {
                                GraphStyle::Curves   => GraphStyle::Heavy,
                                GraphStyle::Heavy    => GraphStyle::Bubbles,
                                GraphStyle::Bubbles  => GraphStyle::BubblesX,
                                GraphStyle::BubblesX => GraphStyle::Ascii,
                                GraphStyle::Ascii    => GraphStyle::Curves,
                            };
                            // Always-on graph in Log view → re-render now.
                            app.recompute_graph_rows();
                        }
                        4 => app.settings.show_history_view   = !app.settings.show_history_view,
                        5 => app.settings.show_remote_view    = !app.settings.show_remote_view,
                        6 => app.settings.show_mirror_view    = !app.settings.show_mirror_view,
                        7 => app.settings.show_workspace_view = !app.settings.show_workspace_view,
                        8 => app.settings.show_help_view      = !app.settings.show_help_view,
                        _ => {}
                    }
                }

                Action::SettingsSave => {
                    app.settings.save();
                    app.settings_view.status = Some("settings saved".to_string());
                }


                Action::WorkspaceSync => {
                    if let Some(ws) = app.workspace_view.workspaces.get(app.workspace_view.ws_idx) {
                        let name = ws.name.clone();
                        let status = std::process::Command::new("torii")
                            .args(["workspace", "sync", &name])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let msg = match status {
                            Ok(s) if s.success() => format!("synced workspace: {}", name),
                            _ => format!("sync failed for: {}", name),
                        };
                        let kind = if msg.starts_with("synced") { EventKind::Success } else { EventKind::Error };
                        app.log_event(&msg, kind);
                        app.workspace_view.status = Some(msg);
                        app.go_to(View::Workspace);
                    }
                }

                Action::WorkspaceSyncOne => {
                    let repo_path = app.workspace_view.workspaces
                        .get(app.workspace_view.ws_idx)
                        .and_then(|ws| ws.repos.get(app.workspace_view.repo_idx))
                        .map(|r| r.path.clone());
                    if let Some(path) = repo_path {
                        let status = std::process::Command::new("torii")
                            .args(["sync"])
                            .current_dir(&path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let msg = match status {
                            Ok(s) if s.success() => format!("synced: {}", path),
                            _ => format!("sync failed: {}", path),
                        };
                        let kind = if msg.starts_with("synced") { EventKind::Success } else { EventKind::Error };
                        app.log_event(&msg, kind);
                        app.workspace_view.status = Some(msg);
                        app.refresh().ok();
                    }
                }

                Action::WorkspaceOpenRepo => {
                    let result = app.workspace_view.workspaces
                        .get(app.workspace_view.ws_idx)
                        .and_then(|ws| {
                            ws.repos.get(app.workspace_view.repo_idx)
                                .map(|r| (ws.name.clone(), r.path.clone()))
                        });
                    if let Some((ws_name, path)) = result {
                        app.log_event(format!("opened: {}", path), EventKind::Info);
                        app.repo_path = path;
                        app.active_workspace = Some(ws_name);
                        app.refresh().ok();
                        app.go_to(View::Dashboard);
                    }
                }

                Action::WorkspaceDelete => {
                    if let Some(ws) = app.workspace_view.workspaces.get(app.workspace_view.ws_idx) {
                        let name = ws.name.clone();
                        let status = std::process::Command::new("torii")
                            .args(["workspace", "delete", &name])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let is_ok = matches!(status, Ok(s) if s.success());
                        let msg = if is_ok { format!("deleted workspace: {}", name) }
                                  else { format!("delete failed: {}", name) };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        app.workspace_view.status = Some(msg);
                        app.go_to(View::Workspace);
                    }
                }

                Action::WorkspaceSave => {
                    let (name, msg_text) = if let Some(ws) = app.workspace_view.workspaces.get(app.workspace_view.ws_idx) {
                        (ws.name.clone(), app.workspace_view.input.clone())
                    } else { continue; };
                    app.workspace_view.input.clear();
                    let status = std::process::Command::new("torii")
                        .args(["workspace", "save", &name, "-m", &msg_text])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    let is_ok = matches!(status, Ok(s) if s.success());
                    let msg = if is_ok { format!("saved workspace: {}", name) }
                              else { format!("save failed: {}", name) };
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    app.workspace_view.status = Some(msg);
                    app.go_to(View::Workspace);
                }

                Action::WorkspaceAddRepo => {
                    let (name, path) = if let Some(ws) = app.workspace_view.workspaces.get(app.workspace_view.ws_idx) {
                        (ws.name.clone(), app.workspace_view.input.clone())
                    } else { continue; };
                    app.workspace_view.input.clear();
                    let status = std::process::Command::new("torii")
                        .args(["workspace", "add", &name, &path])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    let is_ok = matches!(status, Ok(s) if s.success());
                    let msg = if is_ok { format!("added repo to {}", name) }
                              else { "add repo failed".to_string() };
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    app.workspace_view.status = Some(msg);
                    app.go_to(View::Workspace);
                }

                Action::WorkspaceRename => {
                    let (old_name, new_name) = if let Some(ws) = app.workspace_view.workspaces.get(app.workspace_view.ws_idx) {
                        (ws.name.clone(), app.workspace_view.input.trim().to_string())
                    } else { continue; };
                    app.workspace_view.input.clear();
                    let ws_path = dirs::home_dir()
                        .map(|h| h.join(".torii/workspaces.toml"))
                        .unwrap_or_default();
                    let result = std::fs::read_to_string(&ws_path).ok().map(|content| {
                        content.lines().map(|line| {
                            if line.trim() == format!("[{}]", old_name) {
                                format!("[{}]", new_name)
                            } else {
                                line.to_string()
                            }
                        }).collect::<Vec<_>>().join("\n")
                    });
                    let is_ok = if let Some(new_content) = result {
                        std::fs::write(&ws_path, new_content).is_ok()
                    } else { false };
                    let msg = if is_ok { format!("renamed: {} → {}", old_name, new_name) }
                              else { "rename failed".to_string() };
                    app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                    if is_ok {
                        if app.active_workspace.as_deref() == Some(&old_name) {
                            app.active_workspace = Some(new_name);
                        }
                        app.go_to(View::Workspace);
                    } else {
                        app.workspace_view.status = Some(msg);
                    }
                }

                Action::WorkspaceRemoveRepo => {
                    let (name, path) = if let Some(ws) = app.workspace_view.workspaces.get(app.workspace_view.ws_idx) {
                        let path = ws.repos.get(app.workspace_view.repo_idx).map(|r| r.path.clone());
                        (ws.name.clone(), path)
                    } else { continue; };
                    if let Some(path) = path {
                        let status = std::process::Command::new("torii")
                            .args(["workspace", "remove", &name, &path])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        let is_ok = matches!(status, Ok(s) if s.success());
                        let msg = if is_ok { format!("removed repo from {}", name) }
                                  else { "remove repo failed".to_string() };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        app.workspace_view.status = Some(msg);
                        app.go_to(View::Workspace);
                    }
                }

                Action::PrCreate => {
                    let title = app.pr_view.create_title.clone();
                    let base  = app.pr_view.create_base.clone();
                    let desc  = app.pr_view.create_desc.clone();
                    let draft = app.pr_view.create_draft;
                    if title.is_empty() {
                        app.log_event("create failed: title required", EventKind::Error);
                    } else {
                        let mut args = vec!["pr", "create", "-t", &title, "-b", &base];
                        if !desc.is_empty() { args.extend(["-d", &desc]); }
                        if draft { args.push("--draft"); }
                        let output = std::process::Command::new("torii")
                            .args(&args)
                            .output();
                        let is_ok = matches!(&output, Ok(o) if o.status.success());
                        let msg = if is_ok {
                            format!("created PR: {}", title)
                        } else {
                            let stderr = output.ok()
                                .and_then(|o| String::from_utf8(o.stderr).ok())
                                .unwrap_or_default();
                            let hint = if stderr.contains("token") || stderr.contains("TOKEN") {
                                let platform = &app.pr_view.platform;
                                if platform == "gitlab" {
                                    " — set auth.gitlab_token in torii config".to_string()
                                } else {
                                    " — set auth.github_token in torii config".to_string()
                                }
                            } else { String::new() };
                            format!("create failed{}", hint)
                        };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        if is_ok { app.load_prs(); }
                    }
                }

                Action::PrCreateMulti => {
                    let title  = app.pr_view.create_title.clone();
                    let base   = app.pr_view.create_base.clone();
                    let desc   = app.pr_view.create_desc.clone();
                    let draft  = app.pr_view.create_draft;
                    let head   = app.pr_view.create_head.clone();
                    if title.is_empty() {
                        app.log_event("create failed: title required", EventKind::Error);
                        break;
                    }
                    let platforms: Vec<_> = app.pr_view.available_platforms.iter()
                        .zip(app.pr_view.create_platform_selected.iter())
                        .filter(|(_, &sel)| sel)
                        .map(|(p, _)| p.clone())
                        .collect();
                    if platforms.is_empty() {
                        app.log_event("create failed: select at least one platform", EventKind::Error);
                        break;
                    }
                    use crate::pr::{get_pr_client, CreatePrOptions};
                    let mut any_ok = false;
                    for entry in &platforms {
                        let opts = CreatePrOptions {
                            title: title.clone(),
                            body:  if desc.is_empty() { None } else { Some(desc.clone()) },
                            head:  head.clone(),
                            base:  base.clone(),
                            draft,
                        };
                        match get_pr_client(&entry.platform).and_then(|c| c.create(&entry.owner, &entry.repo, opts)) {
                            Ok(pr) => {
                                app.log_event(&format!("created {} #{} on {}: {}", if entry.platform == "gitlab" { "MR" } else { "PR" }, pr.number, entry.platform, title), EventKind::Success);
                                any_ok = true;
                            }
                            Err(e) => app.log_event(&format!("create failed on {}: {}", entry.platform, e), EventKind::Error),
                        }
                    }
                    app.pr_view.create_title.clear();
                    app.pr_view.create_desc.clear();
                    app.pr_view.create_input.clear();
                    if any_ok { app.load_prs(); }
                }

                Action::PrMerge => {
                    if let Some(pr) = app.pr_view.prs.get(app.pr_view.idx) {
                        let number = pr.number;
                        let head_branch = pr.head.clone();
                        let base_branch = pr.base.clone();
                        let method = match app.pr_view.merge_method {
                            1 => MergeMethod::Squash,
                            2 => MergeMethod::Rebase,
                            _ => MergeMethod::Merge,
                        };
                        let platform = app.pr_view.platform.clone();
                        let owner = app.pr_view.owner.clone();
                        let repo_name = app.pr_view.repo_name.clone();
                        let repo_path = app.repo_path.clone();
                        use crate::pr::{get_pr_client, MergeMethod};
                        match get_pr_client(&platform).and_then(|c| c.merge(&owner, &repo_name, number, method)) {
                            Ok(_) => {
                                // 1. checkout base branch
                                let _ = std::process::Command::new("torii")
                                    .args(["branch", &base_branch])
                                    .current_dir(&repo_path)
                                    .output();
                                // 2. pull to get the merge commit locally
                                let _ = std::process::Command::new("torii")
                                    .args(["sync", "--pull"])
                                    .current_dir(&repo_path)
                                    .output();
                                // 3. delete head branch on all remotes
                                let del_remote = std::process::Command::new("torii")
                                    .args(["branch", "--delete-remote", &head_branch])
                                    .current_dir(&repo_path)
                                    .output();
                                let remote_ok = matches!(&del_remote, Ok(o) if o.status.success());
                                // 4. force delete local branch (already on base, so -d might fail if not merged locally)
                                let _ = std::process::Command::new("torii")
                                    .args(["branch", "-d", &head_branch, "--force"])
                                    .current_dir(&repo_path)
                                    .output();
                                let del_msg = if remote_ok {
                                    format!("branch '{}' deleted on all remotes", head_branch)
                                } else {
                                    format!("branch '{}' — remote delete failed (may not exist)", head_branch)
                                };
                                app.log_event(&format!("merged #{} → {} — {}", number, base_branch, del_msg), EventKind::Success);
                                app.refresh().ok();
                                app.load_prs();
                            }
                            Err(e) => app.log_event(&format!("merge failed: {}", e), EventKind::Error),
                        }
                    }
                }

                Action::PrClose => {
                    if let Some(pr) = app.pr_view.prs.get(app.pr_view.idx) {
                        let number = pr.number.to_string();
                        let output = std::process::Command::new("torii")
                            .args(["pr", "close", &number])
                            .output();
                        let is_ok = matches!(&output, Ok(o) if o.status.success());
                        let msg = if is_ok { format!("closed PR #{}", number) }
                                  else { format!("close failed: PR #{}", number) };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        if is_ok { app.load_prs(); }
                    }
                }

                Action::PrCheckout => {
                    if let Some(pr) = app.pr_view.prs.get(app.pr_view.idx) {
                        let number = pr.number.to_string();
                        let output = std::process::Command::new("torii")
                            .args(["pr", "checkout", &number])
                            .output();
                        let is_ok = matches!(&output, Ok(o) if o.status.success());
                        let msg = if is_ok { format!("checked out PR #{}", number) }
                                  else { format!("checkout failed: PR #{}", number) };
                        app.log_event(&msg, if is_ok { EventKind::Success } else { EventKind::Error });
                        if is_ok { app.refresh()?; }
                    }
                }

                Action::PrOpenBrowser => {
                    if let Some(pr) = app.pr_view.prs.get(app.pr_view.idx) {
                        let number = pr.number.to_string();
                        let _ = std::process::Command::new("torii")
                            .args(["pr", "open", &number])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                    }
                }

                Action::PrUpdate => {
                    if let Some(pr) = app.pr_view.prs.get(app.pr_view.idx) {
                        let number = pr.number;
                        let platform = app.pr_view.platform.clone();
                        let owner = app.pr_view.owner.clone();
                        let repo_name = app.pr_view.repo_name.clone();
                        let new_title = app.pr_view.edit_input.trim().to_string();
                        let new_desc = app.pr_view.edit_desc.trim().to_string();
                        let new_base = app.pr_view.branches.get(app.pr_view.branch_idx).cloned();
                        app.pr_view.edit_input.clear();
                        app.pr_view.edit_desc.clear();
                        use crate::pr::{get_pr_client, UpdatePrOptions};
                        let opts = UpdatePrOptions {
                            title: if new_title.is_empty() { None } else { Some(new_title) },
                            body:  if new_desc.is_empty()  { None } else { Some(new_desc) },
                            base:  new_base,
                        };
                        match get_pr_client(&platform).and_then(|c| c.update(&owner, &repo_name, number, opts)) {
                            Ok(_) => {
                                app.log_event(&format!("updated PR #{}", number), EventKind::Success);
                                app.load_prs();
                            }
                            Err(e) => app.log_event(&format!("update failed: {}", e), EventKind::Error),
                        }
                    }
                }

                Action::PrSwitchPlatform => {
                    if let Some(entry) = app.pr_view.available_platforms.get(app.pr_view.platform_idx) {
                        app.pr_view.platform  = entry.platform.clone();
                        app.pr_view.owner     = entry.owner.clone();
                        app.pr_view.repo_name = entry.repo.clone();
                    }
                    app.pr_view.confirm = app::PrConfirm::None;
                    app.load_prs();
                }

                Action::PrRefresh => {
                    app.load_prs();
                }

                Action::IssueClose => {
                    let iv = &app.issue_view;
                    let Some(issue) = iv.issues.get(iv.idx) else { break; };
                    let number = issue.number;
                    let platform = iv.platform.clone();
                    let owner = iv.owner.clone();
                    let repo_name = iv.repo_name.clone();
                    use crate::issue::get_issue_client;
                    match get_issue_client(&platform).and_then(|c| c.close(&owner, &repo_name, number)) {
                        Ok(_) => {
                            app.log_event(&format!("closed issue #{}", number), EventKind::Success);
                            app.load_issues();
                        }
                        Err(e) => app.log_event(&format!("close failed: {}", e), EventKind::Error),
                    }
                }

                Action::IssueCreate => {
                    let iv = &app.issue_view;
                    let title = iv.create_title.trim().to_string();
                    let desc = iv.create_desc.trim().to_string();
                    let platform = iv.platform.clone();
                    let owner = iv.owner.clone();
                    let repo_name = iv.repo_name.clone();
                    if title.is_empty() { break; }
                    app.issue_view.confirm = app::IssueConfirm::None;
                    use crate::issue::{get_issue_client, CreateIssueOptions};
                    let opts = CreateIssueOptions {
                        title: title.clone(),
                        body: if desc.is_empty() { None } else { Some(desc) },
                    };
                    match get_issue_client(&platform).and_then(|c| c.create(&owner, &repo_name, opts)) {
                        Ok(i) => {
                            app.log_event(&format!("created issue #{}: {}", i.number, title), EventKind::Success);
                            app.issue_view.create_title.clear();
                            app.issue_view.create_desc.clear();
                            app.load_issues();
                        }
                        Err(e) => app.log_event(&format!("create failed: {}", e), EventKind::Error),
                    }
                }

                Action::IssueComment => {
                    let iv = &app.issue_view;
                    let Some(issue) = iv.issues.get(iv.idx) else { break; };
                    let number = issue.number;
                    let body = iv.comment_input.trim().to_string();
                    let platform = iv.platform.clone();
                    let owner = iv.owner.clone();
                    let repo_name = iv.repo_name.clone();
                    if body.is_empty() { break; }
                    app.issue_view.confirm = app::IssueConfirm::None;
                    app.issue_view.comment_input.clear();
                    use crate::issue::get_issue_client;
                    match get_issue_client(&platform).and_then(|c| c.comment(&owner, &repo_name, number, &body)) {
                        Ok(_) => app.log_event(&format!("comment added to #{}", number), EventKind::Success),
                        Err(e) => app.log_event(&format!("comment failed: {}", e), EventKind::Error),
                    }
                }

                Action::IssueOpenBrowser => {
                    if let Some(issue) = app.issue_view.issues.get(app.issue_view.idx) {
                        let url = issue.url.clone();
                        let _ = std::process::Command::new("xdg-open")
                            .arg(&url)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                        app.log_event(&format!("opened: {}", url), EventKind::Info);
                    }
                }

                Action::IssueRefresh => {
                    app.load_issues();
                }

            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

// ── Git operations ────────────────────────────────────────────────────────────

fn stage_selected(app: &mut App) -> crate::error::Result<()> {
    use git2::Repository;
    use app::Panel;

    let repo = Repository::discover(&app.repo_path).map_err(crate::error::ToriiError::Git)?;
    let mut index = repo.index().map_err(crate::error::ToriiError::Git)?;

    let path = match app.dashboard.selected_panel {
        Panel::Unstaged  => app.unstaged.get(app.dashboard.unstaged_idx).map(|e| e.path.clone()),
        Panel::Untracked => app.untracked.get(app.dashboard.untracked_idx).map(|e| e.path.clone()),
        _ => None,
    };

    if let Some(p) = path {
        index.add_path(std::path::Path::new(&p)).map_err(crate::error::ToriiError::Git)?;
        index.write().map_err(crate::error::ToriiError::Git)?;
        app.set_status(format!("staged: {}", p));
    }
    Ok(())
}

fn unstage_selected(app: &mut App) -> crate::error::Result<()> {
    use git2::Repository;

    let repo = Repository::discover(&app.repo_path).map_err(crate::error::ToriiError::Git)?;
    let path = app.staged.get(app.dashboard.staged_idx).map(|e| e.path.clone());

    if let Some(p) = path {
        let head = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        if let Some(commit) = head {
            let treeish = commit.as_object();
            repo.reset_default(Some(treeish), [p.as_str()].iter())
                .map_err(crate::error::ToriiError::Git)?;
        } else {
            // No commits yet — remove from index directly
            let mut index = repo.index().map_err(crate::error::ToriiError::Git)?;
            index.remove_path(std::path::Path::new(&p)).map_err(crate::error::ToriiError::Git)?;
            index.write().map_err(crate::error::ToriiError::Git)?;
        }
        app.set_status(format!("unstaged: {}", p));
    }
    Ok(())
}

fn commit_staged(app: &App, message: &str) -> crate::error::Result<()> {
    use git2::Repository;

    let repo = Repository::discover(&app.repo_path).map_err(crate::error::ToriiError::Git)?;
    let mut index = repo.index().map_err(crate::error::ToriiError::Git)?;
    let tree_oid = index.write_tree().map_err(crate::error::ToriiError::Git)?;
    let tree = repo.find_tree(tree_oid).map_err(crate::error::ToriiError::Git)?;

    let author = crate::core::resolve_signature(&repo)?;

    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();

    // 0.7.14: route through commit_inner so `git.sign_commits = true`
    // honours GPG signing from the TUI commit view too.
    crate::core::commit_inner(&repo, Some("HEAD"), &author, message, &tree, &parents)?;

    Ok(())
}


fn snapshot_dir(app: &App) -> std::path::PathBuf {
    std::path::Path::new(&app.repo_path).join(".git/torii-snapshots")
}

fn torii_dir(app: &App) -> std::path::PathBuf {
    std::path::Path::new(&app.repo_path).join(".torii")
}

fn save_auto_interval(app: &App) {
    use crate::tui::app::AutoSnapshotInterval;
    let dir = torii_dir(app);
    let _ = std::fs::create_dir_all(&dir);
    let val = match app.snapshot_view.auto_interval {
        AutoSnapshotInterval::Off   => "off",
        AutoSnapshotInterval::Min5  => "5min",
        AutoSnapshotInterval::Min15 => "15min",
        AutoSnapshotInterval::Min30 => "30min",
        AutoSnapshotInterval::Hour1 => "1h",
    };
    let _ = std::fs::write(dir.join("auto-interval"), val);
}

fn load_auto_interval(app: &mut App) {
    use crate::tui::app::AutoSnapshotInterval;
    let path = torii_dir(app).join("auto-interval");
    let val = std::fs::read_to_string(path).unwrap_or_default();
    app.snapshot_view.auto_interval = match val.trim() {
        "5min"  => AutoSnapshotInterval::Min5,
        "15min" => AutoSnapshotInterval::Min15,
        "30min" => AutoSnapshotInterval::Min30,
        "1h"    => AutoSnapshotInterval::Hour1,
        _       => AutoSnapshotInterval::Off,
    };
    app.snapshot_view.auto_interval_idx = AutoSnapshotInterval::all()
        .iter().position(|i| i == &app.snapshot_view.auto_interval)
        .unwrap_or(0);
}

fn create_snapshot_with_name(app: &mut App, name: &str) -> crate::error::Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dir = snapshot_dir(app);
    std::fs::create_dir_all(&dir)?;
    let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs().to_string();
    std::fs::write(dir.join(format!("{}.meta", id)), name)?;
    app.load_snapshots();
    Ok(())
}

fn create_snapshot(app: &mut App) -> crate::error::Result<()> {
    use crate::tui::app::SnapshotFocus;
    let name = if app.snapshot_view.create_name.trim().is_empty() {
        "snapshot".to_string()
    } else {
        app.snapshot_view.create_name.trim().to_string()
    };
    app.snapshot_view.focus = SnapshotFocus::List;
    create_snapshot_with_name(app, &name)
}

fn delete_snapshot(app: &mut App) -> crate::error::Result<()> {
    if let Some(s) = app.snapshot_view.snapshots.get(app.snapshot_view.idx) {
        let id = s.id.clone();
        let dir = snapshot_dir(app);
        let _ = std::fs::remove_file(dir.join(format!("{}.meta", id)));
    }
    app.load_snapshots();
    Ok(())
}

fn restore_selected_snapshot(app: &mut App) -> crate::error::Result<()> {
    let snap = app.snapshot_view.snapshots.get(app.snapshot_view.idx);
    if let Some(s) = snap {
        app.set_status(format!("restored: {}", s.name));
    }
    Ok(())
}

fn spawn_sync(app: &mut App) {
    use crate::core::GitRepo;
    use std::sync::mpsc;

    app.sync_view.status = SyncStatus::Running;

    let repo_path = app.repo_path.clone();
    let op = app.sync_view.selected_op.clone();

    let (tx, rx) = mpsc::channel();
    app.sync_rx = Some(rx);

    std::thread::spawn(move || {
        let result = match op {
            SyncOp::PullPush  => GitRepo::open(&repo_path)
                .and_then(|r| r.pull().and_then(|_| r.push(false)))
                .map(|_| "synced with remote".to_string()),
            SyncOp::PullOnly  => GitRepo::open(&repo_path)
                .and_then(|r| r.pull())
                .map(|_| "pulled from remote".to_string()),
            SyncOp::PushOnly  => GitRepo::open(&repo_path)
                .and_then(|r| r.push(false))
                .map(|_| "pushed to remote".to_string()),
            SyncOp::ForcePush => GitRepo::open(&repo_path)
                .and_then(|r| r.push(true))
                .map(|_| "force pushed to remote".to_string()),
            SyncOp::Fetch     => GitRepo::open(&repo_path)
                .and_then(|r| r.fetch())
                .map(|_| "fetched remote refs".to_string()),
        };
        let _ = tx.send(result);
    });
}

fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let pipe_cmd = |cmd: &str, args: &[&str]| -> bool {
        Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                c.stdin.as_mut().unwrap().write_all(text.as_bytes())?;
                c.wait()
            })
            .map(|s| s.success())
            .unwrap_or(false)
    };

    // macOS
    if cfg!(target_os = "macos") {
        return pipe_cmd("pbcopy", &[]);
    }

    // Windows
    if cfg!(target_os = "windows") {
        return pipe_cmd("clip", &[]);
    }

    // Wayland
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        if Command::new("wl-copy").arg(text).status().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }

    // X11
    if pipe_cmd("xclip", &["-selection", "clipboard"]) { return true; }
    if pipe_cmd("xsel", &["--clipboard", "--input"])    { return true; }

    false
}

fn truncate_msg(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() }
    else { format!("{}…", &s[..max.saturating_sub(1)]) }
}
