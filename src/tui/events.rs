use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use std::time::Duration;
use crate::error::Result;
use super::app::{App, View, Panel, CommitFocus, WorkspaceFocus, WorkspaceConfirm, SnapshotFocus, BranchConfirm, TagConfirm, HistoryConfirm, RemoteConfirm, PrConfirm, IssueConfirm};


#[allow(dead_code)]
pub enum Action {
    Quit,
    Refresh,
    SidebarUp,
    SidebarDown,
    SidebarEnter,
    StageFile,
    UnstageFile,
    CommitConfirm,
    BranchCheckout,
    BranchDelete,
    BranchCreate,
    BranchPush,
    SnapshotRestore,
    SnapshotCreate,
    SnapshotDelete,
    SnapshotSaveInterval,
    OpenDiffFromLog,
    LogCopyHash,
    SyncRun,
    TagPush,
    TagDelete,
    TagCreate,
    HistoryCherryPick,
    HistoryRebase,
    HistoryScan,
    HistoryClean,
    HistoryRemoveFile,
    HistoryRewrite,
    HistoryBlame,
    RemoteInfo,
    RemoteFetch,
    RemoteAdd,
    RemoteRemove,
    RemoteRename,
    RemoteEditUrl,
    RemoteOpenBrowser,
    MirrorSync,
    MirrorSyncOne,
    MirrorSyncForce,
    MirrorRemove,
    MirrorRename,
    MirrorAdd,
    MirrorSetPrimary,
    WorkspaceSync,
    WorkspaceSyncOne,
    WorkspaceOpenRepo,
    WorkspaceDelete,
    WorkspaceSave,
    WorkspaceAddRepo,
    WorkspaceRemoveRepo,
    WorkspaceRename,
    PrMerge,
    PrClose,
    PrCreate,
    PrCheckout,
    PrOpenBrowser,
    PrRefresh,
    PrUpdate,
    PrSwitchPlatform,
    PrCreateMulti,
    IssueClose,
    IssueCreate,
    IssueComment,
    IssueOpenBrowser,
    IssueRefresh,
    ConfigEdit,
    ConfigSave,
    ConfigToggleScope,
    SettingsToggle,
    SettingsSave,
    /// 0.7.24 — open the currently focused job log in `$PAGER` (or `less`).
    /// Handled in the main loop because the pager replaces the TUI.
    OpenJobLogInPager,
}

pub struct EventHandler;

impl EventHandler {
    pub fn new() -> Self { Self }

    pub fn next(&mut self, app: &mut App) -> Result<Option<Action>> {
        if !event::poll(Duration::from_millis(200))? {
            return Ok(None);
        }

        match event::read()? {
            Event::Key(key) => {
                // Clear event log when panel is open
                if app.show_event_log {
                    if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::NONE {
                        app.event_log.clear();
                        return Ok(None);
                    }
                }
                // Tab cycles focus: sidebar → view panels → sidebar
                if key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE {
                    app.tab_cycle();
                    return Ok(None);
                }
                // Repo picker — global, except when typing
                if key.code == KeyCode::Char('W') && key.modifiers == KeyModifiers::SHIFT {
                    if app.repo_picker_open {
                        app.repo_picker_open = false;
                    } else {
                        app.open_repo_picker();
                    }
                    return Ok(None);
                }

                // Repo picker navigation when open
                if app.repo_picker_open {
                    match (key.modifiers, key.code) {
                        (_, KeyCode::Esc) => { app.repo_picker_open = false; }
                        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                            if app.repo_picker_idx > 0 { app.repo_picker_idx -= 1; }
                        }
                        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                            let max = app.workspace_repo_paths().len().saturating_sub(1);
                            if app.repo_picker_idx < max { app.repo_picker_idx += 1; }
                        }
                        (_, KeyCode::Enter) => {
                            let ws_name = app.active_workspace.clone();
                            let paths = app.workspace_repo_paths();
                            if let Some(path) = paths.get(app.repo_picker_idx) {
                                app.repo_path = path.clone();
                                app.active_workspace = ws_name; // mantener el workspace activo
                                app.repo_picker_open = false;
                                app.refresh().ok();
                                app.go_to(View::Dashboard);
                            }
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(Some(Action::Quit)),
                        _ => {}
                    }
                    return Ok(None);
                }

                // e toggles event log from anywhere — except when typing in a text input
                let typing = match app.view {
                    View::Commit    => app.commit_view.focus == CommitFocus::Input,
                    View::Snapshot  => app.snapshot_view.focus == SnapshotFocus::Create || app.snapshot_view.search_mode,
                    View::Log       => app.log.search_mode,
                    View::Branch    => app.branch_view.confirm == BranchConfirm::NewBranch || app.branch_view.search_mode,
                    View::Tag       => matches!(app.tag_view.confirm, TagConfirm::CreateName | TagConfirm::CreateMessage) || app.tag_view.search_mode,
                    View::History   => matches!(app.history_view.confirm,
                        HistoryConfirm::Rebase | HistoryConfirm::RemoveFile |
                        HistoryConfirm::RewriteStart | HistoryConfirm::RewriteEnd |
                        HistoryConfirm::Blame),
                    View::Remote    => matches!(app.remote_view.confirm,
                        RemoteConfirm::AddName | RemoteConfirm::AddUrl | RemoteConfirm::Rename |
                        RemoteConfirm::EditUrl |
                        RemoteConfirm::MirrorRename | RemoteConfirm::MirrorAddPlatform |
                        RemoteConfirm::MirrorAddAccount | RemoteConfirm::MirrorAddRepo),
                    View::Workspace => matches!(app.workspace_view.confirm,
                        WorkspaceConfirm::SaveMessage | WorkspaceConfirm::AddRepoPath),
                    View::Pr        => matches!(app.pr_view.confirm,
                        PrConfirm::CreateTitle | PrConfirm::CreateDesc |
                        PrConfirm::EditTitle | PrConfirm::EditDesc),
                    View::Issue     => matches!(app.issue_view.confirm,
                        IssueConfirm::CreateTitle | IssueConfirm::CreateDesc | IssueConfirm::Comment),
                    View::Config    => app.config_view.editing,
                    _               => false,
                };
                if key.code == KeyCode::Char('e') && key.modifiers == KeyModifiers::NONE && !typing {
                    app.show_event_log = !app.show_event_log;
                    return Ok(None);
                }
                // Sidebar navigation takes priority when focused
                // but pass Enter and action keys to the current view too
                if app.sidebar_focused {
                    let is_nav = matches!(key.code,
                        KeyCode::Up | KeyCode::Down |
                        KeyCode::Char('j') | KeyCode::Char('k')
                    ) && key.modifiers == KeyModifiers::NONE;
                    let is_enter = key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE;
                    let is_quit = matches!(key.code, KeyCode::Char('q'))
                        || (key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c'));
                    // when typing in an overlay, block sidebar nav entirely
                    if typing {
                        // 0.7.35 bugfix — Commit/Snapshot/Log/Config
                        // were missing from this delegation. Their text
                        // inputs absorbed Char events through the
                        // sidebar-focused path below, but Esc / Enter /
                        // Backspace / arrows fell through to None and
                        // the user was stuck inside the field with no
                        // way to confirm or cancel. Now every view
                        // whose `typing` predicate above can return
                        // true is delegated here too.
                        return Ok(match app.view {
                            View::Commit    => handle_commit(key, app),
                            View::Pr        => handle_pr(key, app),
                            View::Issue     => handle_issue(key, app),
                            View::Branch    => handle_branch(key, app),
                            View::Tag       => handle_tag(key, app),
                            View::Snapshot  => handle_snapshot(key, app),
                            View::Log       => handle_log(key, app),
                            View::History   => handle_history(key, app),
                            View::Remote | View::Mirror => handle_remote(key, app),
                            View::Workspace => handle_workspace(key, app),
                            View::Config    => handle_config(key, app),
                            _ => None,
                        });
                    }
                    if is_nav || is_enter || is_quit {
                        return Ok(handle_sidebar(key, app));
                    }
                    // For action keys, delegate to view handler
                    let view_result = match app.view {
                        View::Log       => handle_log(key, app),
                        View::Branch    => handle_branch(key, app),
                        View::Tag       => handle_tag(key, app),
                        View::History   => handle_history(key, app),
                        View::Remote    => handle_remote(key, app),
                        View::Mirror    => handle_mirror(key, app),
                        View::Snapshot  => handle_snapshot(key, app),
                        View::Workspace => handle_workspace(key, app),
                        View::Pr        => handle_pr(key, app),
                        View::Issue     => handle_issue(key, app),
                        _ => None,
                    };
                    if view_result.is_some() {
                        return Ok(view_result);
                    }
                    return Ok(handle_sidebar(key, app));
                }
                // Esc always returns focus to sidebar unless the view handles it specially
                if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE && app.repo_picker_open {
                    app.repo_picker_open = false;
                    return Ok(None);
                }
                if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
                    let handled_by_view = match app.view {
                        View::Diff      => { app.go_back(); true }
                        View::Commit    => app.commit_view.focus == CommitFocus::Input,
                        View::Config    => app.config_view.editing,
                        View::Settings  => false,
                        View::Log       => {
                            if app.log.ops_mode {
                                app.log.ops_mode = false;
                                true
                            } else if app.log.search_mode {
                                app.log.search_mode = false;
                                app.log.search_query.clear();
                                app.log.filtered.clear();
                                true
                            } else {
                                false
                            }
                        }
                        View::Branch    => {
                            if app.branch_view.search_mode {
                                app.branch_view.search_mode = false;
                                app.branch_view.search_query.clear();
                                app.branch_view.filtered.clear();
                                true
                            } else if app.branch_view.ops_mode {
                                app.branch_view.ops_mode = false;
                                true
                            } else if app.branch_view.confirm != BranchConfirm::None {
                                app.branch_view.confirm = BranchConfirm::None;
                                app.branch_view.new_name.clear();
                                true
                            } else {
                                false
                            }
                        }
                        View::Tag       => {
                            if app.tag_view.search_mode {
                                app.tag_view.search_mode = false;
                                app.tag_view.search_query.clear();
                                app.tag_view.filtered.clear();
                                true
                            } else if app.tag_view.ops_mode {
                                app.tag_view.ops_mode = false;
                                true
                            } else if app.tag_view.confirm != TagConfirm::None {
                                app.tag_view.confirm = TagConfirm::None;
                                app.tag_view.new_name.clear();
                                app.tag_view.new_message.clear();
                                true
                            } else {
                                false
                            }
                        }
                        View::History   => {
                            if app.history_view.ops_mode {
                                app.history_view.ops_mode = false;
                                true
                            } else if app.history_view.confirm != HistoryConfirm::None {
                                app.history_view.confirm = HistoryConfirm::None;
                                app.history_view.input.clear();
                                app.history_view.input2.clear();
                                true
                            } else {
                                false
                            }
                        }
                        View::Snapshot  => {
                            if app.snapshot_view.ops_mode {
                                app.snapshot_view.ops_mode = false;
                                true
                            } else if app.snapshot_view.search_mode {
                                app.snapshot_view.search_mode = false;
                                app.snapshot_view.search_query.clear();
                                app.snapshot_view.filtered.clear();
                                app.snapshot_view.idx = 0;
                                true
                            } else {
                                false
                            }
                        }
                        View::Mirror    => {
                            if app.mirror_view.ops_mode {
                                app.mirror_view.ops_mode = false;
                                true
                            } else {
                                false
                            }
                        }
                        View::Pr => {
                            if app.pr_view.ops_mode {
                                app.pr_view.ops_mode = false;
                                true
                            } else if app.pr_view.confirm != PrConfirm::None {
                                app.pr_view.confirm = PrConfirm::None;
                                app.pr_view.create_input.clear();
                                true
                            } else {
                                false
                            }
                        }
                        View::Issue => {
                            if app.issue_view.ops_mode {
                                app.issue_view.ops_mode = false;
                                true
                            } else if app.issue_view.confirm != IssueConfirm::None {
                                app.issue_view.confirm = IssueConfirm::None;
                                app.issue_view.create_input.clear();
                                app.issue_view.comment_input.clear();
                                true
                            } else {
                                false
                            }
                        }
                        View::Workspace => {
                            if app.workspace_view.ops_mode {
                                app.workspace_view.ops_mode = false;
                                true
                            } else if app.workspace_view.confirm != WorkspaceConfirm::None {
                                app.workspace_view.confirm = WorkspaceConfirm::None;
                                app.workspace_view.input.clear();
                                true
                            } else if app.workspace_view.focus == WorkspaceFocus::Repos {
                                app.workspace_view.focus = WorkspaceFocus::Workspaces;
                                true
                            } else {
                                false
                            }
                        }
                        View::Remote    => {
                            if app.remote_view.ops_mode {
                                app.remote_view.ops_mode = false;
                                true
                            } else if app.remote_view.confirm != RemoteConfirm::None {
                                app.remote_view.confirm = RemoteConfirm::None;
                                app.remote_view.new_name.clear();
                                app.remote_view.new_url.clear();
                                app.remote_view.new_mirror_platform.clear();
                                app.remote_view.new_mirror_account.clear();
                                app.remote_view.new_mirror_repo.clear();
                                app.remote_view.new_mirror_type = 0;
                                true
                            } else {
                                false
                            }
                        }

                        // 0.7.12 — Platform: Esc walks back through the
                        // drill-down (popup → JobLog → JobsOfPipeline →
                        // List, then defers to the global handler which
                        // returns focus to the sidebar).
                        // 0.7.33 — Bisect overlays close on Esc; back to list.
                        View::Bisect => {
                            use crate::tui::app::BisectFocus;
                            if app.bisect_view.focus != BisectFocus::List {
                                app.bisect_view.focus = BisectFocus::List;
                                true
                            } else {
                                false
                            }
                        }

                        // 0.7.30 — Auth overlays close on Esc; back to list.
                        // 0.7.32 — also clears the in-flight OAuth modal so
                        // the user can bail out of a polling worker.
                        View::Auth => {
                            use crate::tui::app::AuthFocus;
                            if app.auth_view.focus == AuthFocus::OauthFlow {
                                app.auth_view.oauth_flow = None;
                                app.auth_oauth_rx = None;
                                app.auth_view.focus = AuthFocus::List;
                                true
                            } else if app.auth_view.focus != AuthFocus::List {
                                app.auth_view.focus = AuthFocus::List;
                                true
                            } else {
                                false
                            }
                        }

                        View::Platform  => {
                            use crate::tui::app::PlatformFocus;
                            match app.platform_view.focus {
                                PlatformFocus::RemotePopup
                                | PlatformFocus::OpsDropdown
                                | PlatformFocus::FilterDropdown => {
                                    app.platform_view.focus = PlatformFocus::List;
                                    true
                                }
                                PlatformFocus::JobLog => {
                                    app.platform_view.focus = PlatformFocus::JobsOfPipeline;
                                    app.platform_view.job_log = None;
                                    true
                                }
                                PlatformFocus::JobsOfPipeline => {
                                    use crate::tui::app::PlatformSubTab;
                                    app.platform_view.focus = PlatformFocus::List;
                                    app.platform_view.active_pipeline_id = None;
                                    app.platform_view.jobs.clear();
                                    app.platform_view.sub_tab = PlatformSubTab::Pipelines;
                                    true
                                }
                                PlatformFocus::List => false,
                            }
                        }

                        _               => false,
                    };
                    if !handled_by_view {
                        app.sidebar_focused = true;
                    }
                    return Ok(None);
                }
                return Ok(match app.view {
                    View::Dashboard => handle_dashboard(key, app),
                    View::Diff      => handle_diff(key, app),
                    View::Log       => handle_log(key, app),
                    View::Branch    => handle_branch(key, app),
                    View::Commit    => handle_commit(key, app),
                    View::Snapshot  => handle_snapshot(key, app),
                    View::Sync      => handle_sync(key, app),
                    View::Tag       => handle_tag(key, app),
                    View::History   => handle_log(key, app), // fused into Log
                    View::Remote    => handle_remote(key, app),
                    View::Mirror    => handle_remote(key, app), // fused into Remote
                    View::Workspace => handle_workspace(key, app),
                    View::Pr        => handle_pr(key, app),
                    View::Issue     => handle_issue(key, app),
                    // 0.7.2: no custom keybinds yet for the four new views;
                    // they're informative and refresh on entry. ↑/↓ etc. are
                    // handled in the generic list navigation block above.
                    View::Worktree | View::Submodule => None,
                    View::Bisect    => handle_bisect(key, app),
                    View::Auth      => handle_auth(key, app),
                    // 0.7.12 — unified Platform view.
                    View::Platform  => handle_platform(key, app),
                    View::Config    => handle_config(key, app),
                    View::Settings  => handle_config(key, app), // fused into Config
                    View::Help      => handle_help(key, app),
                });
            }
            Event::Resize(_, _) => {}
            _ => {}
        }

        Ok(None)
    }
}

fn handle_dashboard(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // Global nav first
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }

    match (key.modifiers, key.code) {
        (_, KeyCode::BackTab) => app.prev_panel(),
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.move_down(),

        (KeyModifiers::CONTROL, KeyCode::Char('r')) => return Some(Action::Refresh),

        (_, KeyCode::Char(' ')) => {
            match app.dashboard.selected_panel {
                Panel::Unstaged | Panel::Untracked => return Some(Action::StageFile),
                Panel::Staged                      => return Some(Action::UnstageFile),
                Panel::Log                         => {}
            }
        }

        (_, KeyCode::Char('d')) => app.go_to(View::Diff),

        _ => {}
    }
    None
}

/// Universal shortcuts available from any view. View-switching is intentionally
/// NOT here — it lives in the sidebar (Tab to focus, j/k navigate, Enter open),
/// freeing every letter for view-local keybindings (e.g. 'g' toggles graph in
/// Log view without jumping to Config).
fn handle_global_nav(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match (key.modifiers, key.code) {
        (_, KeyCode::Char('q')) |
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
        (_, KeyCode::Char('?')) => app.go_to(View::Help),
        _ => return None,
    }
    None
}

fn handle_diff(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) | (_, KeyCode::Char('q')) => app.go_back(),
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.diff_scroll_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.diff_scroll_down(),
        (_, KeyCode::PageUp)                          => app.diff_page_up(),
        (_, KeyCode::PageDown)                        => app.diff_page_down(),
        (KeyModifiers::CONTROL, KeyCode::Char('c'))  => return Some(Action::Quit),
        _ => {}
    }
    None
}

fn handle_log(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if app.log.search_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.log.search_mode = false;
                app.log.search_query.clear();
                app.log.filtered.clear();
            }
            (_, KeyCode::Enter) => {
                app.log.search_mode = false;
            }
            (_, KeyCode::Backspace) => {
                app.log.search_query.pop();
                app.log_update_filter();
            }
            (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                      key.modifiers == KeyModifiers::SHIFT
                                    => {
                app.log.search_query.push(c);
                app.log_update_filter();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }
    if app.log.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.log.ops_idx > 0 { app.log.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.log.ops_idx < 2 { app.log.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.log.ops_idx;
                app.log.ops_mode = false;
                match idx {
                    0 => return Some(Action::OpenDiffFromLog),
                    1 => return Some(Action::LogCopyHash),
                    2 => {
                        app.log.search_mode = true;
                        app.log.search_query.clear();
                        app.log.filtered.clear();
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.log.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    // 0.7.36 — armor overlay grabs every key so the user can dismiss
    // it without accidentally firing log-navigation keybinds.
    if app.log.signature_overlay.is_some() {
        // Esc / any non-modifier key closes the modal.
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {
                app.log.signature_overlay = None;
                app.log_signature_rx = None;
            }
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)     | (_, KeyCode::Char('k')) => { app.log_move_up(); app.log.ops_mode = false; }
        (_, KeyCode::Down)   | (_, KeyCode::Char('j')) => { app.log_move_down(); app.log.ops_mode = false; }
        (_, KeyCode::Char('o')) => {
            app.log.ops_mode = true;
            app.log.ops_idx = 0;
        }
        (_, KeyCode::Char('/')) => {
            app.log.search_mode = true;
            app.log.search_query.clear();
            app.log.filtered.clear();
        }
        // 0.7.36 — toggle the GPG signature column. Populates the
        // verdict cache the first time it turns on (cheap because we
        // only run gpg --verify on the commits already loaded into
        // the log slice, typically ≤ page_size).
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (_, KeyCode::Char('G')) => {
            app.log.show_signatures = !app.log.show_signatures;
            if app.log.show_signatures {
                app.refresh_signature_cache();
            }
        }
        // 0.7.36 — open the armor overlay for the currently selected
        // commit. Spawns a worker so the modal can render `loading…`
        // while gpg runs.
        (KeyModifiers::SHIFT, KeyCode::Char('S')) | (_, KeyCode::Char('S')) => {
            if let Some(c) = app.commits.get(app.log.idx) {
                let oid = c.full_hash.clone();
                app.start_signature_overlay(oid);
            }
        }
        _ => {}
    }
    None
}

fn handle_branch(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match app.branch_view.confirm.clone() {
        BranchConfirm::Delete => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.branch_view.confirm = BranchConfirm::None;
                    return Some(Action::BranchDelete);
                }
                _ => {
                    app.branch_view.confirm = BranchConfirm::None;
                    app.branch_view.status = Some("cancelled".to_string());
                }
            }
            return None;
        }
        BranchConfirm::NewBranch => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.branch_view.confirm = BranchConfirm::None;
                    app.branch_view.new_name.clear();
                }
                (_, KeyCode::Enter) => {
                    app.branch_view.confirm = BranchConfirm::None;
                    return Some(Action::BranchCreate);
                }
                (_, KeyCode::Backspace) => { app.branch_view.new_name.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.branch_view.new_name.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        BranchConfirm::None => {}
    }
    if app.branch_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.branch_view.ops_idx > 0 { app.branch_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.branch_view.ops_idx < 3 { app.branch_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.branch_view.ops_idx;
                app.branch_view.ops_mode = false;
                match idx {
                    0 => return Some(Action::BranchCheckout),
                    1 => {
                        app.branch_view.new_name.clear();
                        app.branch_view.confirm = BranchConfirm::NewBranch;
                    }
                    2 => {
                        if !app.branch_view.current_has_upstream {
                            return Some(Action::BranchPush);
                        }
                    }
                    3 => {
                        if let Some(b) = app.branch_view.branches.get(app.branch_view.idx) {
                            if !b.is_current && !b.is_remote {
                                app.branch_view.confirm = BranchConfirm::Delete;
                            } else if b.is_remote {
                                app.branch_view.status = Some("cannot delete remote branch".to_string());
                            } else {
                                app.branch_view.status = Some("cannot delete current branch".to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.branch_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    if app.branch_view.search_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.branch_view.search_mode = false;
                app.branch_view.search_query.clear();
                app.branch_view.filtered.clear();
            }
            (_, KeyCode::Enter) => { app.branch_view.search_mode = false; }
            (_, KeyCode::Backspace) => {
                app.branch_view.search_query.pop();
                app.branch_update_filter();
            }
            (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => {
                app.branch_view.search_query.push(c);
                app.branch_update_filter();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)    | (_, KeyCode::Char('k')) => app.branch_move_up(),
        (_, KeyCode::Down)  | (_, KeyCode::Char('j')) => app.branch_move_down(),
        (_, KeyCode::Char('o')) => {
            app.branch_view.ops_mode = true;
            app.branch_view.ops_idx = 0;
        }
        (_, KeyCode::Char('/')) => {
            app.branch_view.search_mode = true;
            app.branch_view.search_query.clear();
            app.branch_view.filtered.clear();
        }
        _ => {}
    }
    None
}

fn handle_commit(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    const N_TYPES: usize = 8;
    match app.commit_view.focus {
        CommitFocus::List => {
            if let Some(a) = handle_global_nav(key, app) { return Some(a); }
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => app.commit_view.focus = CommitFocus::TypeSelector,
                (_, KeyCode::Char('i')) => {
                    app.commit_view.focus = CommitFocus::Input;
                }
                (_, KeyCode::Char('a')) => {
                    app.commit_view.amend = !app.commit_view.amend;
                    if app.commit_view.amend && app.commit_view.message.is_empty() {
                        // Pre-fill with last commit message
                        if let Some(c) = app.commits.first() {
                            app.commit_view.message = c.message.clone();
                            app.commit_view.cursor = c.message.len();
                        }
                    }
                }
                _ => {}
            }
        }
        CommitFocus::TypeSelector => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => {
                    if app.commit_view.type_idx > 0 { app.commit_view.type_idx -= 1; }
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                    if app.commit_view.type_idx < N_TYPES - 1 { app.commit_view.type_idx += 1; }
                }
                (_, KeyCode::Enter) => {
                    let prefix = COMMIT_TYPES[app.commit_view.type_idx].0;
                    let prefix_str = format!("{}: ", prefix);
                    if !app.commit_view.message.starts_with(&prefix_str) {
                        // Strip any existing type prefix first
                        let base = if let Some(colon) = app.commit_view.message.find(": ") {
                            app.commit_view.message[colon + 2..].to_string()
                        } else {
                            app.commit_view.message.clone()
                        };
                        app.commit_view.message = format!("{}{}", prefix_str, base);
                        app.commit_view.cursor = app.commit_view.message.len();
                    }
                    app.commit_view.focus = CommitFocus::Input;
                }
                (_, KeyCode::Esc) => app.commit_view.focus = CommitFocus::List,
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
        }
        CommitFocus::Input => match (key.modifiers, key.code) {
            (_, KeyCode::Esc)                            => app.commit_view.focus = CommitFocus::TypeSelector,
            (_, KeyCode::Enter)                          => return Some(Action::CommitConfirm),
            (_, KeyCode::Backspace)                      => app.commit_backspace(),
            (_, KeyCode::Left)                           => app.commit_cursor_left(),
            (_, KeyCode::Right)                          => app.commit_cursor_right(),
            (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                      key.modifiers == KeyModifiers::SHIFT
                                                         => app.commit_type_char(c),
            (KeyModifiers::CONTROL, KeyCode::Char('c'))  => return Some(Action::Quit),
            _ => {}
        },
    }
    None
}

pub const COMMIT_TYPES: &[(&str, &str)] = &[
    ("feat",     "new feature"),
    ("fix",      "bug fix"),
    ("chore",    "maintenance task"),
    ("docs",     "documentation"),
    ("refactor", "code restructure"),
    ("test",     "tests"),
    ("ci",       "CI/CD changes"),
    ("perf",     "performance improvement"),
];

fn handle_snapshot(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{SnapshotFocus, AutoSnapshotInterval};

    // ops dropdown (only in List focus)
    if app.snapshot_view.focus == SnapshotFocus::List && app.snapshot_view.ops_mode {
        const OPS_LEN: usize = 3;
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.snapshot_view.ops_idx > 0 { app.snapshot_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.snapshot_view.ops_idx < OPS_LEN - 1 { app.snapshot_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.snapshot_view.ops_idx;
                app.snapshot_view.ops_mode = false;
                return match idx {
                    0 => Some(Action::SnapshotRestore),
                    1 => {
                        app.snapshot_view.create_name.clear();
                        app.snapshot_view.focus = SnapshotFocus::Create;
                        None
                    }
                    2 => Some(Action::SnapshotDelete),
                    _ => None,
                };
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.snapshot_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    match app.snapshot_view.focus {
        SnapshotFocus::List => {
            // search mode input
            if app.snapshot_view.search_mode {
                match (key.modifiers, key.code) {
                    (_, KeyCode::Esc) => {
                        app.snapshot_view.search_mode = false;
                        app.snapshot_view.search_query.clear();
                        app.snapshot_view.filtered.clear();
                        app.snapshot_view.idx = 0;
                    }
                    (_, KeyCode::Enter) => {
                        app.snapshot_view.search_mode = false;
                    }
                    (_, KeyCode::Backspace) => {
                        app.snapshot_view.search_query.pop();
                        snapshot_update_filter(app);
                    }
                    (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                              key.modifiers == KeyModifiers::SHIFT
                                            => {
                        app.snapshot_view.search_query.push(c);
                        snapshot_update_filter(app);
                    }
                    _ => {}
                }
                return None;
            }

            if let Some(a) = handle_global_nav(key, app) { return Some(a); }
            match (key.modifiers, key.code) {
                (_, KeyCode::Up)    | (_, KeyCode::Char('k')) => app.snapshot_move_up(),
                (_, KeyCode::Down)  | (_, KeyCode::Char('j')) => app.snapshot_move_down(),
                (_, KeyCode::Char('o')) => {
                    app.snapshot_view.ops_mode = true;
                    app.snapshot_view.ops_idx = 0;
                }
                (_, KeyCode::Char('/')) => {
                    app.snapshot_view.search_mode = true;
                    app.snapshot_view.search_query.clear();
                    app.snapshot_view.filtered.clear();
                    app.snapshot_view.idx = 0;
                }
                (_, KeyCode::Char('n'))                       => {
                    app.snapshot_view.create_name.clear();
                    app.snapshot_view.focus = SnapshotFocus::Create;
                }
                (_, KeyCode::Char('a'))                       => {
                    app.snapshot_view.auto_interval_idx = AutoSnapshotInterval::all()
                        .iter().position(|i| i == &app.snapshot_view.auto_interval)
                        .unwrap_or(0);
                    app.snapshot_view.focus = SnapshotFocus::AutoConfig;
                }
                _ => {}
            }
        }
        SnapshotFocus::Create => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc)       => app.snapshot_view.focus = SnapshotFocus::List,
                (_, KeyCode::Enter)     => return Some(Action::SnapshotCreate),
                (_, KeyCode::Backspace) => { app.snapshot_view.create_name.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.snapshot_view.create_name.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
        }
        SnapshotFocus::AutoConfig => {
            let n = AutoSnapshotInterval::all().len();
            match (key.modifiers, key.code) {
                (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => {
                    if app.snapshot_view.auto_interval_idx > 0 {
                        app.snapshot_view.auto_interval_idx -= 1;
                    }
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                    if app.snapshot_view.auto_interval_idx < n - 1 {
                        app.snapshot_view.auto_interval_idx += 1;
                    }
                }
                (_, KeyCode::Enter) => {
                    app.snapshot_view.auto_interval =
                        AutoSnapshotInterval::all()[app.snapshot_view.auto_interval_idx].clone();
                    app.snapshot_view.focus = SnapshotFocus::List;
                    return Some(Action::SnapshotSaveInterval);
                }
                (_, KeyCode::Esc) => app.snapshot_view.focus = SnapshotFocus::List,
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
        }
    }
    None
}

fn handle_sync(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.sync_op_prev(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.sync_op_next(),
        (_, KeyCode::Enter)                          => return Some(Action::SyncRun),
        _ => {}
    }
    None
}

fn snapshot_update_filter(app: &mut App) {
    let q = app.snapshot_view.search_query.to_lowercase();
    if q.is_empty() {
        app.snapshot_view.filtered.clear();
    } else {
        app.snapshot_view.filtered = app.snapshot_view.snapshots
            .iter().enumerate()
            .filter(|(_, s)| s.name.to_lowercase().contains(&q) || s.id.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
    }
    app.snapshot_view.idx = 0;
}

fn handle_sidebar(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)    | (_, KeyCode::Char('k')) => return Some(Action::SidebarUp),
        (_, KeyCode::Down)  | (_, KeyCode::Char('j')) => return Some(Action::SidebarDown),
        (_, KeyCode::Enter)                           => return Some(Action::SidebarEnter),
        (_, KeyCode::Char('?'))                       => { app.go_to(View::Help); }
        (_, KeyCode::Char('q')) |
        (KeyModifiers::CONTROL, KeyCode::Char('c'))   => return Some(Action::Quit),
        _ => {}
    }
    None
}

fn handle_help(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) | (_, KeyCode::Char('?')) | (_, KeyCode::Char('q')) => app.go_back(),
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
        _ => {}
    }
    None
}

fn handle_tag(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match app.tag_view.confirm.clone() {
        TagConfirm::Delete => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.tag_view.confirm = TagConfirm::None;
                    return Some(Action::TagDelete);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => { app.tag_view.confirm = TagConfirm::None; }
            }
            return None;
        }
        TagConfirm::CreateName => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    if !app.tag_view.new_name.trim().is_empty() {
                        app.tag_view.confirm = TagConfirm::CreateMessage;
                    }
                }
                (_, KeyCode::Backspace) => { app.tag_view.new_name.pop(); }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.tag_view.new_name.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        TagConfirm::CreateMessage => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    app.tag_view.confirm = TagConfirm::None;
                    return Some(Action::TagCreate);
                }
                (_, KeyCode::Backspace) => { app.tag_view.new_message.pop(); }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.tag_view.new_message.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        TagConfirm::None => {}
    }
    if app.tag_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.tag_view.ops_idx > 0 { app.tag_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.tag_view.ops_idx < 2 { app.tag_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.tag_view.ops_idx;
                app.tag_view.ops_mode = false;
                match idx {
                    0 => return Some(Action::TagPush),
                    1 => {
                        app.tag_view.new_name.clear();
                        app.tag_view.new_message.clear();
                        app.tag_view.confirm = TagConfirm::CreateName;
                    }
                    2 => {
                        if !app.tag_view.tags.is_empty() {
                            app.tag_view.confirm = TagConfirm::Delete;
                        }
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.tag_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    if app.tag_view.search_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.tag_view.search_mode = false;
                app.tag_view.search_query.clear();
                app.tag_view.filtered.clear();
            }
            (_, KeyCode::Enter) => { app.tag_view.search_mode = false; }
            (_, KeyCode::Backspace) => {
                app.tag_view.search_query.pop();
                app.tag_update_filter();
            }
            (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => {
                app.tag_view.search_query.push(c);
                app.tag_update_filter();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.tag_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.tag_move_down(),
        (_, KeyCode::Char('o')) => {
            app.tag_view.ops_mode = true;
            app.tag_view.ops_idx = 0;
        }
        (_, KeyCode::Char('/')) => {
            app.tag_view.search_mode = true;
            app.tag_view.search_query.clear();
            app.tag_view.filtered.clear();
        }
        _ => {}
    }
    None
}

#[allow(dead_code)]
fn handle_history(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match app.history_view.confirm.clone() {
        HistoryConfirm::CherryPick => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryCherryPick);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => { app.history_view.confirm = HistoryConfirm::None; }
            }
            return None;
        }
        HistoryConfirm::Clean => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryClean);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => { app.history_view.confirm = HistoryConfirm::None; }
            }
            return None;
        }
        HistoryConfirm::RemoveFile => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    if !app.history_view.input.trim().is_empty() {
                        app.history_view.confirm = HistoryConfirm::None;
                        return Some(Action::HistoryRemoveFile);
                    }
                }
                (_, KeyCode::Backspace) => { app.history_view.input.pop(); }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::Rebase => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    if !app.history_view.input.trim().is_empty() {
                        app.history_view.confirm = HistoryConfirm::None;
                        return Some(Action::HistoryRebase);
                    }
                }
                (_, KeyCode::Backspace) => { app.history_view.input.pop(); }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::RewriteStart => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    if !app.history_view.input.trim().is_empty() {
                        app.history_view.confirm = HistoryConfirm::RewriteEnd;
                    }
                }
                (_, KeyCode::Backspace) => { app.history_view.input.pop(); }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::RewriteEnd => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    if !app.history_view.input2.trim().is_empty() {
                        app.history_view.confirm = HistoryConfirm::None;
                        return Some(Action::HistoryRewrite);
                    }
                }
                (_, KeyCode::Backspace) => { app.history_view.input2.pop(); }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input2.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::Blame => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    if !app.history_view.input.trim().is_empty() {
                        app.history_view.confirm = HistoryConfirm::None;
                        return Some(Action::HistoryBlame);
                    }
                }
                (_, KeyCode::Backspace) => { app.history_view.input.pop(); }
                (KeyModifiers::NONE, KeyCode::Char(c)) => app.history_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        HistoryConfirm::Scan => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('f')) => {
                    app.history_view.scan_full = !app.history_view.scan_full;
                }
                (_, KeyCode::Enter) => {
                    app.history_view.confirm = HistoryConfirm::None;
                    return Some(Action::HistoryScan);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => { app.history_view.confirm = HistoryConfirm::None; }
            }
            return None;
        }
        HistoryConfirm::None => {}
    }
    if app.history_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.history_view.ops_idx > 0 { app.history_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.history_view.ops_idx < 6 { app.history_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.history_view.ops_idx;
                app.history_view.ops_mode = false;
                match idx {
                    0 => { app.history_view.confirm = HistoryConfirm::CherryPick; }
                    1 => { app.history_view.input.clear(); app.history_view.confirm = HistoryConfirm::Rebase; }
                    2 => { app.history_view.confirm = HistoryConfirm::Scan; }
                    3 => { app.history_view.confirm = HistoryConfirm::Clean; }
                    4 => { app.history_view.input.clear(); app.history_view.confirm = HistoryConfirm::Blame; }
                    5 => { app.history_view.input.clear(); app.history_view.input2.clear(); app.history_view.confirm = HistoryConfirm::RewriteStart; }
                    6 => { app.history_view.input.clear(); app.history_view.confirm = HistoryConfirm::RemoveFile; }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.history_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.history_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.history_move_down(),
        (_, KeyCode::Char('o')) => {
            app.history_view.ops_mode = true;
            app.history_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}

fn handle_remote(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // confirm states (text input)
    match &app.remote_view.confirm {
        RemoteConfirm::AddName => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_name.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.remote_view.new_name.is_empty() {
                        app.remote_view.confirm = RemoteConfirm::AddUrl;
                    }
                }
                (_, KeyCode::Backspace) => { app.remote_view.new_name.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.remote_view.new_name.push(c),
                _ => {}
            }
            return None;
        }
        RemoteConfirm::AddUrl => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_name.clear();
                    app.remote_view.new_url.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.remote_view.new_url.is_empty() {
                        return Some(Action::RemoteAdd);
                    }
                }
                (_, KeyCode::Backspace) => { app.remote_view.new_url.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.remote_view.new_url.push(c),
                _ => {}
            }
            return None;
        }
        RemoteConfirm::Remove => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    return Some(Action::RemoteRemove);
                }
                _ => { app.remote_view.confirm = RemoteConfirm::None; }
            }
            return None;
        }
        RemoteConfirm::Rename => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_name.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.remote_view.new_name.is_empty() {
                        return Some(Action::RemoteRename);
                    }
                }
                (_, KeyCode::Backspace) => { app.remote_view.new_name.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.remote_view.new_name.push(c),
                _ => {}
            }
            return None;
        }
        RemoteConfirm::EditUrl => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_url.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.remote_view.new_url.is_empty() {
                        return Some(Action::RemoteEditUrl);
                    }
                }
                (_, KeyCode::Backspace) => { app.remote_view.new_url.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.remote_view.new_url.push(c),
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorRename => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_name.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.remote_view.new_name.is_empty() {
                        return Some(Action::MirrorRename);
                    }
                }
                (_, KeyCode::Backspace) => { app.remote_view.new_name.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.remote_view.new_name.push(c),
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorAddPlatform => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_mirror_platform.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.remote_view.new_mirror_platform.is_empty() {
                        app.remote_view.new_mirror_account.clear();
                        app.remote_view.confirm = RemoteConfirm::MirrorAddAccount;
                    }
                }
                (_, KeyCode::Backspace) => { app.remote_view.new_mirror_platform.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.remote_view.new_mirror_platform.push(c),
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorAddAccount => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_mirror_platform.clear();
                    app.remote_view.new_mirror_account.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.remote_view.new_mirror_account.is_empty() {
                        app.remote_view.new_mirror_repo.clear();
                        app.remote_view.confirm = RemoteConfirm::MirrorAddRepo;
                    }
                }
                (_, KeyCode::Backspace) => { app.remote_view.new_mirror_account.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.remote_view.new_mirror_account.push(c),
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorAddRepo => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    app.remote_view.new_mirror_platform.clear();
                    app.remote_view.new_mirror_account.clear();
                    app.remote_view.new_mirror_repo.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.remote_view.new_mirror_repo.is_empty() {
                        app.remote_view.new_mirror_type = 0;
                        app.remote_view.confirm = RemoteConfirm::MirrorAddType;
                    }
                }
                (_, KeyCode::Backspace) => { app.remote_view.new_mirror_repo.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.remote_view.new_mirror_repo.push(c),
                _ => {}
            }
            return None;
        }
        RemoteConfirm::MirrorAddType => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                }
                (_, KeyCode::Left) | (_, KeyCode::Char('h')) => {
                    app.remote_view.new_mirror_type = 0;
                }
                (_, KeyCode::Right) | (_, KeyCode::Char('l')) => {
                    app.remote_view.new_mirror_type = 1;
                }
                (_, KeyCode::Enter) => {
                    app.remote_view.confirm = RemoteConfirm::None;
                    return Some(Action::MirrorAdd);
                }
                _ => {}
            }
            return None;
        }
        RemoteConfirm::None => {}
    }

    // ops dropdown
    if app.remote_view.ops_mode {
        let is_mirror = app.remote_view.selected_is_mirror();
        // mirror dropdown: 6 ops; git-remote dropdown: 7 ops (gained "add
        // mirror" in 0.7.12 so users can create the first mirror without
        // already having one selected).
        let ops_len = if is_mirror { 6 } else { 7 };
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.remote_view.ops_idx > 0 { app.remote_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.remote_view.ops_idx < ops_len - 1 { app.remote_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.remote_view.ops_idx;
                app.remote_view.ops_mode = false;
                if is_mirror {
                    // mirror ops: sync all(0), force sync(1), add mirror(2), set primary(3), rename(4), remove(5)
                    return match idx {
                        0 => Some(Action::MirrorSync),
                        1 => Some(Action::MirrorSyncForce),
                        2 => {
                            app.remote_view.new_mirror_platform.clear();
                            app.remote_view.new_mirror_account.clear();
                            app.remote_view.new_mirror_repo.clear();
                            app.remote_view.new_mirror_type = 0;
                            app.remote_view.confirm = RemoteConfirm::MirrorAddPlatform;
                            None
                        }
                        3 => Some(Action::MirrorSetPrimary),
                        4 => {
                            app.remote_view.new_name.clear();
                            app.remote_view.confirm = RemoteConfirm::MirrorRename;
                            None
                        }
                        5 => Some(Action::MirrorRemove),
                        _ => None,
                    };
                } else {
                    // git remote ops: fetch(0), add remote(1), add mirror(2), rename(3), edit url(4), remove(5), open(6)
                    return match idx {
                        0 => Some(Action::RemoteFetch),
                        1 => {
                            app.remote_view.new_name.clear();
                            app.remote_view.confirm = RemoteConfirm::AddName;
                            None
                        }
                        2 => {
                            app.remote_view.new_mirror_platform.clear();
                            app.remote_view.new_mirror_account.clear();
                            app.remote_view.new_mirror_repo.clear();
                            app.remote_view.new_mirror_type = 0;
                            app.remote_view.confirm = RemoteConfirm::MirrorAddPlatform;
                            None
                        }
                        3 => {
                            app.remote_view.new_name.clear();
                            app.remote_view.confirm = RemoteConfirm::Rename;
                            None
                        }
                        4 => {
                            app.remote_view.new_url.clear();
                            app.remote_view.confirm = RemoteConfirm::EditUrl;
                            None
                        }
                        5 => {
                            app.remote_view.confirm = RemoteConfirm::Remove;
                            None
                        }
                        6 => Some(Action::RemoteOpenBrowser),
                        _ => None,
                    };
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.remote_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => { app.remote_move_up(); }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => { app.remote_move_down(); }
        (_, KeyCode::Char('o')) => {
            app.remote_view.ops_mode = true;
            app.remote_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}

fn handle_mirror(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if app.mirror_view.ops_mode {
        const OPS_LEN: usize = 3;
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.mirror_view.ops_idx > 0 { app.mirror_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.mirror_view.ops_idx < OPS_LEN - 1 { app.mirror_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.mirror_view.ops_idx;
                app.mirror_view.ops_mode = false;
                return match idx {
                    0 => Some(Action::MirrorSync),
                    1 => Some(Action::MirrorSyncForce),
                    2 => Some(Action::MirrorRemove),
                    _ => None,
                };
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.mirror_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.mirror_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.mirror_move_down(),
        (_, KeyCode::Char('o')) => {
            app.mirror_view.ops_mode = true;
            app.mirror_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}

fn handle_workspace(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // confirm states
    match &app.workspace_view.confirm {
        WorkspaceConfirm::DeleteWorkspace => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    return Some(Action::WorkspaceDelete);
                }
                _ => { app.workspace_view.confirm = WorkspaceConfirm::None; }
            }
            return None;
        }
        WorkspaceConfirm::RemoveRepo => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    return Some(Action::WorkspaceRemoveRepo);
                }
                _ => { app.workspace_view.confirm = WorkspaceConfirm::None; }
            }
            return None;
        }
        WorkspaceConfirm::SaveMessage => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    app.workspace_view.input.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.workspace_view.input.trim().is_empty() {
                        app.workspace_view.confirm = WorkspaceConfirm::None;
                        return Some(Action::WorkspaceSave);
                    }
                }
                (_, KeyCode::Backspace) => { app.workspace_view.input.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.workspace_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        WorkspaceConfirm::AddRepoPath => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    app.workspace_view.input.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.workspace_view.input.trim().is_empty() {
                        app.workspace_view.confirm = WorkspaceConfirm::None;
                        return Some(Action::WorkspaceAddRepo);
                    }
                }
                (_, KeyCode::Backspace) => { app.workspace_view.input.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.workspace_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        WorkspaceConfirm::RenameWorkspace => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.workspace_view.confirm = WorkspaceConfirm::None;
                    app.workspace_view.input.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.workspace_view.input.trim().is_empty() {
                        app.workspace_view.confirm = WorkspaceConfirm::None;
                        return Some(Action::WorkspaceRename);
                    }
                }
                (_, KeyCode::Backspace) => { app.workspace_view.input.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.workspace_view.input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        WorkspaceConfirm::None => {}
    }

    // ops dropdown
    if app.workspace_view.ops_mode {
        let is_repos = app.workspace_view.focus == WorkspaceFocus::Repos;
        let ops_len = if is_repos { 4 } else { 5 };
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.workspace_view.ops_idx > 0 { app.workspace_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.workspace_view.ops_idx < ops_len - 1 { app.workspace_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.workspace_view.ops_idx;
                app.workspace_view.ops_mode = false;
                if is_repos {
                    // open(0), sync repo(1), sync workspace(2), remove from workspace(3)
                    return match idx {
                        0 => Some(Action::WorkspaceOpenRepo),
                        1 => Some(Action::WorkspaceSyncOne),
                        2 => Some(Action::WorkspaceSync),
                        3 => {
                            app.workspace_view.confirm = WorkspaceConfirm::RemoveRepo;
                            None
                        }
                        _ => None,
                    };
                } else {
                    // sync all(0), save all(1), rename(2), add repo(3), delete workspace(4)
                    return match idx {
                        0 => Some(Action::WorkspaceSync),
                        1 => {
                            app.workspace_view.input.clear();
                            app.workspace_view.confirm = WorkspaceConfirm::SaveMessage;
                            None
                        }
                        2 => {
                            app.workspace_view.input.clear();
                            app.workspace_view.confirm = WorkspaceConfirm::RenameWorkspace;
                            None
                        }
                        3 => {
                            app.workspace_view.input.clear();
                            app.workspace_view.confirm = WorkspaceConfirm::AddRepoPath;
                            None
                        }
                        4 => {
                            app.workspace_view.confirm = WorkspaceConfirm::DeleteWorkspace;
                            None
                        }
                        _ => None,
                    };
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.workspace_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match app.workspace_view.focus {
        WorkspaceFocus::Workspaces => match (key.modifiers, key.code) {
            (_, KeyCode::Up)    | (_, KeyCode::Char('k')) => app.workspace_move_up(),
            (_, KeyCode::Down)  | (_, KeyCode::Char('j')) => app.workspace_move_down(),
            (_, KeyCode::Right) | (_, KeyCode::Char('l')) => app.workspace_focus_repos(),
            (_, KeyCode::Enter)                           => app.workspace_focus_repos(),
            (_, KeyCode::Char('o')) => {
                app.workspace_view.ops_mode = true;
                app.workspace_view.ops_idx = 0;
            }
            _ => {}
        },
        WorkspaceFocus::Repos => match (key.modifiers, key.code) {
            (_, KeyCode::Up)    | (_, KeyCode::Char('k')) => app.workspace_move_up(),
            (_, KeyCode::Down)  | (_, KeyCode::Char('j')) => app.workspace_move_down(),
            (_, KeyCode::Left)  | (_, KeyCode::Char('h')) => app.workspace_focus_workspaces(),
            (_, KeyCode::Enter)                           => return Some(Action::WorkspaceOpenRepo),
            (_, KeyCode::Char('o')) => {
                app.workspace_view.ops_mode = true;
                app.workspace_view.ops_idx = 0;
            }
            _ => {}
        },
    }
    None
}

fn handle_pr(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::PrConfirm;

    // Create flow — multi-step text input
    if matches!(app.pr_view.confirm, PrConfirm::CreateTitle | PrConfirm::CreateDesc) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.create_input.clear();
                app.pr_view.create_desc.clear();
            }
            (_, KeyCode::Backspace) => {
                if app.pr_view.create_input.is_empty() && app.pr_view.confirm == PrConfirm::CreateDesc {
                    // remove last char from accumulated desc
                    app.pr_view.create_desc.pop();
                } else {
                    app.pr_view.create_input.pop();
                }
            }
            (_, KeyCode::Enter) => {
                match app.pr_view.confirm.clone() {
                    PrConfirm::CreateTitle => {
                        app.pr_view.create_title = app.pr_view.create_input.trim().to_string();
                        app.pr_view.create_input.clear();
                        // load branches for head dropdown
                        app.load_pr_branches();
                        // pre-select current branch as head
                        let current = git2::Repository::discover(&app.repo_path).ok()
                            .and_then(|r| {
                                r.head().ok()
                                    .and_then(|h| h.shorthand().map(|s| s.to_string()))
                            })
                            .unwrap_or_default();
                        app.pr_view.create_head = current.clone();
                        app.pr_view.branch_idx = app.pr_view.branches.iter()
                            .position(|b| *b == current).unwrap_or(0);
                        app.pr_view.confirm = PrConfirm::CreateHead;
                    }
                    PrConfirm::CreateBase => {}
                    PrConfirm::CreateDesc => {
                        // Enter adds a newline to description
                        if !app.pr_view.create_input.is_empty() {
                            if !app.pr_view.create_desc.is_empty() {
                                app.pr_view.create_desc.push('\n');
                            }
                            app.pr_view.create_desc.push_str(&app.pr_view.create_input);
                            app.pr_view.create_input.clear();
                        } else {
                            app.pr_view.create_desc.push('\n');
                        }
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Tab) if app.pr_view.confirm == PrConfirm::CreateDesc => {
                app.pr_view.create_draft = !app.pr_view.create_draft;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('s')) if app.pr_view.confirm == PrConfirm::CreateDesc => {
                // Ctrl+S submits — flush current line first
                if !app.pr_view.create_input.is_empty() {
                    if !app.pr_view.create_desc.is_empty() {
                        app.pr_view.create_desc.push('\n');
                    }
                    app.pr_view.create_desc.push_str(&app.pr_view.create_input);
                    app.pr_view.create_input.clear();
                }
                // advance to platform selection
                app.load_pr_platforms();
                // pre-select current platform
                let n = app.pr_view.available_platforms.len();
                app.pr_view.create_platform_selected = vec![false; n];
                if n > 0 {
                    let cur_platform = app.pr_view.platform.clone();
                    let cur_owner = app.pr_view.owner.clone();
                    let idx = app.pr_view.available_platforms.iter()
                        .position(|p| p.platform == cur_platform && p.owner == cur_owner)
                        .unwrap_or(0);
                    if let Some(s) = app.pr_view.create_platform_selected.get_mut(idx) { *s = true; }
                }
                app.pr_view.create_platform_idx = 0;
                app.pr_view.confirm = PrConfirm::CreatePlatforms;
            }
            (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE
                                  || key.modifiers == KeyModifiers::SHIFT => {
                // enforce title limit
                if app.pr_view.confirm == PrConfirm::CreateTitle
                    && app.pr_view.create_input.chars().count() >= 255 {
                    return None;
                }
                app.pr_view.create_input.push(c);
                // auto-wrap at 56 chars (overlay inner width)
                if app.pr_view.create_input.chars().count() >= 56 {
                    if !app.pr_view.create_desc.is_empty() {
                        app.pr_view.create_desc.push('\n');
                    }
                    app.pr_view.create_desc.push_str(&app.pr_view.create_input);
                    app.pr_view.create_input.clear();
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Create head branch — dropdown
    if app.pr_view.confirm == PrConfirm::CreateHead {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.create_input.clear();
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.pr_view.branch_idx > 0 { app.pr_view.branch_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.pr_view.branch_idx + 1 < app.pr_view.branches.len() {
                    app.pr_view.branch_idx += 1;
                }
            }
            (_, KeyCode::Enter) => {
                if let Some(branch) = app.pr_view.branches.get(app.pr_view.branch_idx) {
                    app.pr_view.create_head = branch.clone();
                }
                // load branches again for base dropdown, pre-select main/master
                app.load_pr_branches();
                let base = app.pr_view.create_base.clone();
                app.pr_view.branch_idx = app.pr_view.branches.iter()
                    .position(|b| b == &base)
                    .or_else(|| app.pr_view.branches.iter().position(|b| b == "main"))
                    .or_else(|| app.pr_view.branches.iter().position(|b| b == "master"))
                    .unwrap_or(0);
                app.pr_view.confirm = PrConfirm::CreateBase;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Create base branch — dropdown
    if app.pr_view.confirm == PrConfirm::CreateBase {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.create_input.clear();
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.pr_view.branch_idx > 0 { app.pr_view.branch_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.pr_view.branch_idx + 1 < app.pr_view.branches.len() {
                    app.pr_view.branch_idx += 1;
                }
            }
            (_, KeyCode::Enter) => {
                if let Some(branch) = app.pr_view.branches.get(app.pr_view.branch_idx) {
                    app.pr_view.create_base = branch.clone();
                }
                app.pr_view.create_input.clear();
                app.pr_view.confirm = PrConfirm::CreateDesc;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Create — platform multi-select
    if app.pr_view.confirm == PrConfirm::CreatePlatforms {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.pr_view.create_platform_idx > 0 { app.pr_view.create_platform_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                let n = app.pr_view.available_platforms.len();
                if app.pr_view.create_platform_idx + 1 < n { app.pr_view.create_platform_idx += 1; }
            }
            (_, KeyCode::Char(' ')) => {
                let idx = app.pr_view.create_platform_idx;
                if let Some(s) = app.pr_view.create_platform_selected.get_mut(idx) { *s = !*s; }
            }
            (_, KeyCode::Char('a')) if key.modifiers == KeyModifiers::NONE => {
                let all = app.pr_view.create_platform_selected.iter().all(|&s| s);
                app.pr_view.create_platform_selected.iter_mut().for_each(|s| *s = !all);
            }
            (_, KeyCode::Enter) => {
                app.pr_view.confirm = PrConfirm::None;
                return Some(Action::PrCreateMulti);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Switch platform dropdown
    if app.pr_view.confirm == PrConfirm::SwitchPlatform {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => { app.pr_view.confirm = PrConfirm::None; }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.pr_view.platform_idx > 0 { app.pr_view.platform_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.pr_view.platform_idx + 1 < app.pr_view.available_platforms.len() {
                    app.pr_view.platform_idx += 1;
                }
            }
            (_, KeyCode::Enter) => {
                return Some(Action::PrSwitchPlatform);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Edit title
    if app.pr_view.confirm == PrConfirm::EditTitle {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.edit_input.clear();
                app.pr_view.edit_desc.clear();
            }
            (_, KeyCode::Enter) => {
                app.pr_view.confirm = PrConfirm::EditDesc;
            }
            (_, KeyCode::Backspace) => { app.pr_view.edit_input.pop(); }
            (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE
                                  || key.modifiers == KeyModifiers::SHIFT => {
                app.pr_view.edit_input.push(c);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Edit description
    if app.pr_view.confirm == PrConfirm::EditDesc {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.edit_input.clear();
                app.pr_view.edit_desc.clear();
            }
            (_, KeyCode::Enter) => {
                app.pr_view.edit_desc.push('\n');
            }
            (_, KeyCode::Backspace) => { app.pr_view.edit_desc.pop(); }
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                // Ctrl+S advances to base branch selection
                app.pr_view.confirm = PrConfirm::EditBase;
            }
            (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE
                                  || key.modifiers == KeyModifiers::SHIFT => {
                app.pr_view.edit_desc.push(c);
                if app.pr_view.edit_desc.chars().rev().take_while(|&ch| ch != '\n').count() >= 56 {
                    app.pr_view.edit_desc.push('\n');
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Edit base branch — dropdown
    if app.pr_view.confirm == PrConfirm::EditBase {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.edit_input.clear();
                app.pr_view.edit_desc.clear();
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.pr_view.branch_idx > 0 { app.pr_view.branch_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.pr_view.branch_idx + 1 < app.pr_view.branches.len() {
                    app.pr_view.branch_idx += 1;
                }
            }
            (_, KeyCode::Enter) => {
                app.pr_view.confirm = PrConfirm::None;
                return Some(Action::PrUpdate);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Merge method selector
    if app.pr_view.confirm == PrConfirm::Merge {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
            }
            (_, KeyCode::Left) | (_, KeyCode::Char('h')) => {
                if app.pr_view.merge_method > 0 { app.pr_view.merge_method -= 1; }
            }
            (_, KeyCode::Right) | (_, KeyCode::Char('l')) => {
                if app.pr_view.merge_method < 2 { app.pr_view.merge_method += 1; }
            }
            (_, KeyCode::Enter) => {
                app.pr_view.confirm = PrConfirm::None;
                return Some(Action::PrMerge);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Close confirmation
    if app.pr_view.confirm == PrConfirm::Close {
        match (key.modifiers, key.code) {
            (_, KeyCode::Char('y')) => {
                app.pr_view.confirm = PrConfirm::None;
                return Some(Action::PrClose);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {
                app.pr_view.confirm = PrConfirm::None;
            }
        }
        return None;
    }

    // Ops dropdown
    if app.pr_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.pr_view.ops_idx > 0 { app.pr_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.pr_view.ops_idx < 6 { app.pr_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.pr_view.ops_idx;
                app.pr_view.ops_mode = false;
                match idx {
                    0 => {
                        // create new PR/MR
                        app.pr_view.create_title.clear();
                        app.pr_view.create_base = "main".to_string();
                        app.pr_view.create_desc.clear();
                        app.pr_view.create_draft = false;
                        app.pr_view.create_input.clear();
                        app.pr_view.confirm = PrConfirm::CreateTitle;
                    }
                    1 => {
                        // edit PR/MR — pre-fill from selected
                        let (title, desc, base) = app.pr_view.prs.get(app.pr_view.idx)
                            .map(|pr| (pr.title.clone(), pr.body.clone().unwrap_or_default(), pr.base.clone()))
                            .unwrap_or_default();
                        app.pr_view.edit_input = title;
                        app.pr_view.edit_desc = desc;
                        app.load_pr_branches();
                        app.pr_view.branch_idx = app.pr_view.branches.iter()
                            .position(|b| *b == base).unwrap_or(0);
                        app.pr_view.confirm = PrConfirm::EditTitle;
                    }
                    2 => {
                        app.pr_view.merge_method = 0;
                        app.pr_view.confirm = PrConfirm::Merge;
                    }
                    3 => { app.pr_view.confirm = PrConfirm::Close; }
                    4 => return Some(Action::PrCheckout),
                    5 => return Some(Action::PrOpenBrowser),
                    6 => {
                        app.load_pr_platforms();
                        app.pr_view.confirm = PrConfirm::SwitchPlatform;
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.pr_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    // Intercept ^r before global_nav steals it
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('r') {
        return Some(Action::PrRefresh);
    }
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.pr_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.pr_move_down(),
        (_, KeyCode::Tab)                             => {
            app.pr_view.filter = match app.pr_view.filter {
                crate::tui::app::PrStateFilter::Open   => crate::tui::app::PrStateFilter::Closed,
                crate::tui::app::PrStateFilter::Closed => crate::tui::app::PrStateFilter::All,
                crate::tui::app::PrStateFilter::All    => crate::tui::app::PrStateFilter::Open,
            };
            app.load_prs();
        }
        (_, KeyCode::Char('o')) => {
            app.pr_view.ops_mode = true;
            app.pr_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}

fn handle_config(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if app.config_view.editing {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc)       => { app.config_view.editing = false; }
            (_, KeyCode::Enter)     => return Some(Action::ConfigSave),
            (_, KeyCode::Backspace) => app.config_backspace(),
            (_, KeyCode::Left)      => app.config_cursor_left(),
            (_, KeyCode::Right)     => app.config_cursor_right(),
            (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                      key.modifiers == KeyModifiers::SHIFT
                                    => app.config_type_char(c),
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.config_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.config_move_down(),
        (_, KeyCode::Enter)                          => { app.config_start_edit(); return Some(Action::ConfigEdit); }
        (_, KeyCode::Tab)                            => return Some(Action::ConfigToggleScope),
        _ => {}
    }
    None
}

#[allow(dead_code)]
fn handle_settings(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => app.settings_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.settings_move_down(),
        (_, KeyCode::Enter)                          => return Some(Action::SettingsToggle),
        (_, KeyCode::Char('s'))                      => return Some(Action::SettingsSave),
        _ => {}
    }
    None
}

fn handle_issue(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    // text input states
    match &app.issue_view.confirm {
        IssueConfirm::CreateTitle => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.issue_view.confirm = IssueConfirm::None;
                    app.issue_view.create_title.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.issue_view.create_title.is_empty() {
                        app.issue_view.create_desc.clear();
                        app.issue_view.confirm = IssueConfirm::CreateDesc;
                    }
                }
                (_, KeyCode::Backspace) => { app.issue_view.create_title.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.issue_view.create_title.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        IssueConfirm::CreateDesc => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.issue_view.confirm = IssueConfirm::None;
                    app.issue_view.create_title.clear();
                    app.issue_view.create_desc.clear();
                }
                (_, KeyCode::Enter) => {
                    return Some(Action::IssueCreate);
                }
                (_, KeyCode::Backspace) => { app.issue_view.create_desc.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.issue_view.create_desc.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        IssueConfirm::Comment => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.issue_view.confirm = IssueConfirm::None;
                    app.issue_view.comment_input.clear();
                }
                (_, KeyCode::Enter) => {
                    if !app.issue_view.comment_input.is_empty() {
                        return Some(Action::IssueComment);
                    }
                }
                (_, KeyCode::Backspace) => { app.issue_view.comment_input.pop(); }
                (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE ||
                                          key.modifiers == KeyModifiers::SHIFT
                                        => app.issue_view.comment_input.push(c),
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => {}
            }
            return None;
        }
        IssueConfirm::Close => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) => {
                    app.issue_view.confirm = IssueConfirm::None;
                    return Some(Action::IssueClose);
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
                _ => { app.issue_view.confirm = IssueConfirm::None; }
            }
            return None;
        }
        IssueConfirm::None => {}
    }

    // ops dropdown
    if app.issue_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.issue_view.ops_idx > 0 { app.issue_view.ops_idx -= 1; }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.issue_view.ops_idx < 3 { app.issue_view.ops_idx += 1; }
            }
            (_, KeyCode::Enter) => {
                let idx = app.issue_view.ops_idx;
                app.issue_view.ops_mode = false;
                match idx {
                    0 => {
                        app.issue_view.create_title.clear();
                        app.issue_view.confirm = IssueConfirm::CreateTitle;
                    }
                    1 => {
                        app.issue_view.comment_input.clear();
                        app.issue_view.confirm = IssueConfirm::Comment;
                    }
                    2 => return Some(Action::IssueOpenBrowser),
                    3 => { app.issue_view.confirm = IssueConfirm::Close; }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.issue_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('r') {
        return Some(Action::IssueRefresh);
    }
    if let Some(a) = handle_global_nav(key, app) { return Some(a); }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => {
            if app.issue_view.idx > 0 { app.issue_view.idx -= 1; }
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            if app.issue_view.idx + 1 < app.issue_view.issues.len() { app.issue_view.idx += 1; }
        }
        (_, KeyCode::Char('o')) => {
            app.issue_view.ops_mode = true;
            app.issue_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}

// ── Platform view (0.7.12) ────────────────────────────────────────────────────
//
// Sub-tabs: 1/2/3/4 → pipelines/jobs/releases/packages.
// 'r' opens the remote-selector popup (centred over the view); inside the
// popup ↑/↓ navigates, Enter selects, Esc closes.
// On the Pipelines list, Enter drills into the selected pipeline's Jobs.
// On the Jobs list (drill-down), Enter fetches the job log into a
// scrollable panel; Esc walks the drill-down back (handled outside).
// Ctrl-R re-runs the loader for whatever sub-tab is active.
// 0.7.33 — Bisect view: contextual ops via `o` dropdown. start/good/
// bad/skip/run/reset routed through `crate::bisect`. Same dropdown +
// input + confirm chrome family as the Auth view.
fn handle_bisect(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{BisectFocus, BisectPendingOp};

    match app.bisect_view.focus {
        BisectFocus::List => {
            if (key.modifiers, key.code) == (KeyModifiers::NONE, KeyCode::Char('o')) {
                app.bisect_view.dropdown_idx = 0;
                app.bisect_view.focus = BisectFocus::OpsDropdown;
            }
            None
        }
        BisectFocus::OpsDropdown => {
            let ops = crate::tui::views::bisect::ops_for(&app.bisect_view);
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                    if app.bisect_view.dropdown_idx > 0 {
                        app.bisect_view.dropdown_idx -= 1;
                    }
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                    if app.bisect_view.dropdown_idx + 1 < ops.len() {
                        app.bisect_view.dropdown_idx += 1;
                    }
                }
                (_, KeyCode::Enter) => return dispatch_bisect_op(app),
                _ => {}
            }
            None
        }
        BisectFocus::RefPicker => {
            return handle_bisect_picker(key, app);
        }
        BisectFocus::InputArgs => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    let buf = std::mem::take(&mut app.bisect_view.input_buffer);
                    let op = app.bisect_view.pending_op.clone();
                    app.bisect_view.focus = BisectFocus::List;
                    app.bisect_view.pending_op = BisectPendingOp::None;
                    let trimmed = buf.trim();
                    if trimmed.is_empty() {
                        app.set_status("✗ empty input, aborted");
                        return None;
                    }
                    match op {
                        BisectPendingOp::Run => {
                            let cmd: Vec<String> = trimmed.split_whitespace().map(String::from).collect();
                            match crate::bisect::run(std::path::Path::new("."), &cmd) {
                                Ok(_)  => app.set_status("✓ bisect run finished"),
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                        }
                        BisectPendingOp::None => {}
                    }
                    crate::tui::views::bisect::refresh(app);
                }
                (_, KeyCode::Backspace) => {
                    app.bisect_view.input_buffer.pop();
                }
                (_, KeyCode::Char(c)) => {
                    if key.modifiers != KeyModifiers::CONTROL {
                        app.bisect_view.input_buffer.push(c);
                    }
                }
                _ => {}
            }
            None
        }
        BisectFocus::ConfirmReset => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    match crate::bisect::reset(std::path::Path::new(".")) {
                        Ok(_)  => app.set_status("✓ bisect reset"),
                        Err(e) => app.set_status(format!("✗ {}", e)),
                    }
                    app.bisect_view.focus = BisectFocus::List;
                    crate::tui::views::bisect::refresh(app);
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    app.bisect_view.focus = BisectFocus::List;
                }
                _ => {}
            }
            None
        }
    }
}

fn dispatch_bisect_op(app: &mut App) -> Option<Action> {
    use crate::tui::app::{BisectFocus, BisectPendingOp, RefPickerOp, RefPickerState, RefPickerTab};
    let ops = crate::tui::views::bisect::ops_for(&app.bisect_view);
    let idx = app.bisect_view.dropdown_idx;
    let label = ops.get(idx).map(|o| o.0).unwrap_or("");
    let path = std::path::Path::new(".");

    let open_picker = |app: &mut App, op: RefPickerOp| {
        app.bisect_view.picker = RefPickerState {
            op,
            tab: RefPickerTab::Bad,
            all: crate::tui::views::bisect::load_refs(),
            idx: 0,
            filter: String::new(),
            bad_pick: None,
            good_picks: Vec::new(),
        };
        app.bisect_view.focus = BisectFocus::RefPicker;
    };

    match label {
        "Start" => open_picker(app, RefPickerOp::Start),
        "Mark good <ref>…" => open_picker(app, RefPickerOp::MarkGood),
        "Mark bad <ref>…"  => open_picker(app, RefPickerOp::MarkBad),
        "Skip <ref>…"      => open_picker(app, RefPickerOp::Skip),

        "Mark HEAD good" => {
            match crate::bisect::good(path, None) {
                Ok(_)  => app.set_status("✓ marked good"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            crate::tui::views::bisect::refresh(app);
        }
        "Mark HEAD bad" => {
            match crate::bisect::bad(path, None) {
                Ok(_)  => app.set_status("✓ marked bad"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            crate::tui::views::bisect::refresh(app);
        }
        "Skip HEAD" => {
            match crate::bisect::skip(path, None) {
                Ok(_)  => app.set_status("✓ skipped"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            crate::tui::views::bisect::refresh(app);
        }
        "Run command" => {
            app.bisect_view.focus = BisectFocus::InputArgs;
            app.bisect_view.input_buffer.clear();
            app.bisect_view.input_prompt = "command (e.g. cargo test -q)".to_string();
            app.bisect_view.pending_op = BisectPendingOp::Run;
        }
        "Reset" => {
            app.bisect_view.focus = BisectFocus::ConfirmReset;
        }
        _ => {
            app.bisect_view.focus = BisectFocus::List;
        }
    }
    None
}

fn handle_bisect_picker(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{RefPickerOp, RefPickerTab};

    // Resolve the filtered slice + currently highlighted entry every
    // call so any filter-buffer mutation immediately reflects in
    // navigation.
    let filtered = crate::tui::views::bisect::filter_indexes(
        &app.bisect_view.picker.all,
        &app.bisect_view.picker.filter,
    );
    let total = filtered.len();
    let cur_idx = app.bisect_view.picker.idx.min(total.saturating_sub(1));

    match (key.modifiers, key.code) {
        // Navigation. `j`/`k` only when the filter is empty so they
        // don't collide with typing into the filter.
        (_, KeyCode::Up) => {
            if cur_idx > 0 { app.bisect_view.picker.idx = cur_idx - 1; }
        }
        (_, KeyCode::Down) => {
            if cur_idx + 1 < total { app.bisect_view.picker.idx = cur_idx + 1; }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) if app.bisect_view.picker.filter.is_empty() => {
            if cur_idx > 0 { app.bisect_view.picker.idx = cur_idx - 1; }
        }
        (KeyModifiers::NONE, KeyCode::Char('j')) if app.bisect_view.picker.filter.is_empty() => {
            if cur_idx + 1 < total { app.bisect_view.picker.idx = cur_idx + 1; }
        }

        // Tab switches Bad ↔ Good only for the Start op.
        (_, KeyCode::Tab) | (_, KeyCode::BackTab) => {
            if matches!(app.bisect_view.picker.op, RefPickerOp::Start) {
                app.bisect_view.picker.tab = match app.bisect_view.picker.tab {
                    RefPickerTab::Bad  => RefPickerTab::Good,
                    RefPickerTab::Good => RefPickerTab::Bad,
                };
                app.bisect_view.picker.idx = 0;
                app.bisect_view.picker.filter.clear();
            }
        }

        // Space: in Start/Good tab, toggle the current entry in the
        // good_picks set (multi-select). Ignored elsewhere.
        (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            if matches!(app.bisect_view.picker.op, RefPickerOp::Start)
                && app.bisect_view.picker.tab == RefPickerTab::Good
                && total > 0
            {
                if let Some(&orig) = filtered.get(cur_idx) {
                    let entry = app.bisect_view.picker.all[orig].clone();
                    let pos = app.bisect_view.picker.good_picks
                        .iter().position(|g| g.target == entry.target);
                    match pos {
                        Some(p) => { app.bisect_view.picker.good_picks.remove(p); }
                        None    => { app.bisect_view.picker.good_picks.push(entry); }
                    }
                }
            }
        }

        // Enter: commit the picker depending on op.
        (_, KeyCode::Enter) => {
            return commit_bisect_picker(app, filtered, cur_idx);
        }

        // Filter: backspace removes one char.
        (_, KeyCode::Backspace) => {
            app.bisect_view.picker.filter.pop();
            app.bisect_view.picker.idx = 0;
        }

        // Filter: any printable char that isn't a navigation key.
        (mods, KeyCode::Char(c))
            if mods == KeyModifiers::NONE || mods == KeyModifiers::SHIFT =>
        {
            app.bisect_view.picker.filter.push(c);
            app.bisect_view.picker.idx = 0;
        }

        _ => {}
    }

    None
}

fn commit_bisect_picker(app: &mut App, filtered: Vec<usize>, cur_idx: usize) -> Option<Action> {
    use crate::tui::app::{BisectFocus, RefPickerOp, RefPickerTab};
    let path = std::path::Path::new(".");

    let Some(&orig) = filtered.get(cur_idx) else { return None };
    let entry = app.bisect_view.picker.all[orig].clone();

    match app.bisect_view.picker.op.clone() {
        RefPickerOp::Start => {
            match app.bisect_view.picker.tab {
                RefPickerTab::Bad => {
                    // Pick this as bad and advance to the Good tab.
                    app.bisect_view.picker.bad_pick = Some(entry);
                    app.bisect_view.picker.tab = RefPickerTab::Good;
                    app.bisect_view.picker.idx = 0;
                    app.bisect_view.picker.filter.clear();
                }
                RefPickerTab::Good => {
                    // If the user hit Enter with nothing toggled, take
                    // the highlighted row as the (single) good. Either
                    // way we need at least one bad + one good.
                    if app.bisect_view.picker.good_picks.is_empty() {
                        app.bisect_view.picker.good_picks.push(entry);
                    }
                    let bad = app.bisect_view.picker.bad_pick.clone();
                    let goods: Vec<String> = app.bisect_view.picker.good_picks.iter()
                        .map(|e| e.target.clone()).collect();
                    let Some(bad) = bad else {
                        app.set_status("✗ no bad ref selected");
                        return None;
                    };
                    let res = crate::bisect::start(path, Some(&bad.target), &goods);
                    match res {
                        Ok(_)  => app.set_status(format!(
                            "✓ bisect started (bad: {} · {} good ref(s))",
                            bad.display, goods.len()
                        )),
                        Err(e) => app.set_status(format!("✗ {}", e)),
                    }
                    app.bisect_view.focus = BisectFocus::List;
                    app.bisect_view.picker = Default::default();
                    crate::tui::views::bisect::refresh(app);
                }
            }
        }
        RefPickerOp::MarkGood => {
            let res = crate::bisect::good(path, Some(&entry.target));
            match res {
                Ok(_)  => app.set_status(format!("✓ marked good: {}", entry.display)),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            app.bisect_view.picker = Default::default();
            crate::tui::views::bisect::refresh(app);
        }
        RefPickerOp::MarkBad => {
            let res = crate::bisect::bad(path, Some(&entry.target));
            match res {
                Ok(_)  => app.set_status(format!("✓ marked bad: {}", entry.display)),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            app.bisect_view.picker = Default::default();
            crate::tui::views::bisect::refresh(app);
        }
        RefPickerOp::Skip => {
            let res = crate::bisect::skip(path, Some(&entry.target));
            match res {
                Ok(_)  => app.set_status(format!("✓ skipped: {}", entry.display)),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.bisect_view.focus = BisectFocus::List;
            app.bisect_view.picker = Default::default();
            crate::tui::views::bisect::refresh(app);
        }
    }
    None
}

// 0.7.30 — Auth view: contextual operations on the selected provider
// (oauth re-auth / rotate / set-token paste / remove). Same dropdown-
// driven pattern as Platform: `o` opens the menu, Enter dispatches,
// Esc closes. OAuth and rotate are externalised to a subprocess (the
// CLI variants) because they require a browser dance — the main loop
// suspends the TUI for them, same as the job-log pager.
fn handle_auth(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{AuthFocus, AuthPendingOp};

    match app.auth_view.focus {
        AuthFocus::List => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                    if app.auth_view.idx > 0 { app.auth_view.idx -= 1; }
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                    if app.auth_view.idx + 1 < app.auth_view.items.len() {
                        app.auth_view.idx += 1;
                    }
                }
                (_, KeyCode::Char('o')) => {
                    if let Some(e) = app.auth_view.items.get(app.auth_view.idx) {
                        app.auth_view.pending_provider = e.provider.clone();
                        app.auth_view.dropdown_idx = 0;
                        app.auth_view.focus = AuthFocus::OpsDropdown;
                    }
                }
                _ => {}
            }
            None
        }
        AuthFocus::OpsDropdown => {
            let ops = crate::tui::views::auth::ops_for(&app.auth_view);
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                    if app.auth_view.dropdown_idx > 0 {
                        app.auth_view.dropdown_idx -= 1;
                    }
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                    if app.auth_view.dropdown_idx + 1 < ops.len() {
                        app.auth_view.dropdown_idx += 1;
                    }
                }
                (_, KeyCode::Enter) => {
                    return dispatch_auth_op(app);
                }
                _ => {}
            }
            None
        }
        AuthFocus::InputToken => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    let token = std::mem::take(&mut app.auth_view.input_buffer);
                    let provider = app.auth_view.pending_provider.clone();
                    let op = app.auth_view.pending_op.clone();
                    app.auth_view.focus = AuthFocus::List;
                    app.auth_view.pending_op = AuthPendingOp::None;
                    if token.trim().is_empty() {
                        app.set_status("✗ empty token, aborted");
                        return None;
                    }
                    match op {
                        AuthPendingOp::SetToken => {
                            match crate::auth::set_token(&provider, token.trim(), None) {
                                Ok(_)  => app.set_status(format!("✓ {} token saved", provider)),
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                        }
                        AuthPendingOp::Login => {
                            let endpoint = crate::auth::default_endpoint();
                            match crate::auth::save_cloud(token.trim(), &endpoint) {
                                Ok(_)  => app.set_status("✓ cloud key saved"),
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                        }
                        AuthPendingOp::None => {}
                    }
                    crate::tui::views::auth::refresh(app);
                }
                (_, KeyCode::Backspace) => {
                    app.auth_view.input_buffer.pop();
                }
                (_, KeyCode::Char(c)) => {
                    if key.modifiers != KeyModifiers::CONTROL {
                        app.auth_view.input_buffer.push(c);
                    }
                }
                _ => {}
            }
            None
        }
        AuthFocus::OauthFlow => {
            // While the worker is in flight (Starting/Waiting/Saving)
            // we ignore keys — the user can still hit Esc, which is
            // handled by the global Esc path above to clear the focus.
            // On Done/Error any key closes the modal so the user can
            // dismiss it without hunting for a specific binding.
            use crate::tui::app::OauthStatus;
            if let Some(state) = app.auth_view.oauth_flow.as_ref() {
                match state.status {
                    OauthStatus::Done(_) | OauthStatus::Error(_) => {
                        app.auth_view.oauth_flow = None;
                        app.auth_view.focus = AuthFocus::List;
                    }
                    _ => {}
                }
            }
            None
        }
        AuthFocus::ConfirmRemove => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    let provider = app.auth_view.pending_provider.clone();
                    match crate::auth::remove_token(&provider, None) {
                        Ok(true)  => app.set_status(format!("✓ {} token removed", provider)),
                        Ok(false) => app.set_status(format!("(no {} token was set)", provider)),
                        Err(e)    => app.set_status(format!("✗ {}", e)),
                    }
                    app.auth_view.focus = AuthFocus::List;
                    crate::tui::views::auth::refresh(app);
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    app.auth_view.focus = AuthFocus::List;
                }
                _ => {}
            }
            None
        }
    }
}

fn dispatch_auth_op(app: &mut App) -> Option<Action> {
    use crate::tui::app::{AuthFocus, AuthPendingOp};
    let provider = app.auth_view.pending_provider.clone();
    let ops = crate::tui::views::auth::ops_for(&app.auth_view);
    let idx = app.auth_view.dropdown_idx;
    let label = ops.get(idx).map(|o| o.0).unwrap_or("");
    match label {
        "Set token (paste)" => {
            app.auth_view.focus = AuthFocus::InputToken;
            app.auth_view.input_buffer.clear();
            app.auth_view.input_prompt = format!("Paste token for {}:", provider);
            app.auth_view.pending_op = AuthPendingOp::SetToken;
        }
        "Remove token" => {
            app.auth_view.focus = AuthFocus::ConfirmRemove;
        }
        "OAuth re-auth" => {
            // Driven entirely inside the TUI now: spawn the worker
            // and let the modal pump status updates.
            let prov = provider.clone();
            app.start_oauth_flow(prov, false, None);
        }
        "Rotate (OAuth)" => {
            // Capture the old token before kicking the worker so it
            // can revoke it after the new one lands.
            let old = crate::auth::resolve_token(&provider, ".").value;
            app.start_oauth_flow(provider.clone(), true, old);
        }
        "Rotate as PAT (GitLab)" => {
            // PAT rotate is one synchronous HTTP call — no need for
            // a modal, but we keep the user inside the TUI by
            // dispatching it here instead of suspending the screen.
            let old = crate::auth::resolve_token(&provider, ".").value;
            match old {
                None => app.set_status("✗ no GitLab token to rotate"),
                Some(old_tok) => match crate::oauth::rotate_gitlab_pat(&old_tok) {
                    Ok(new_tok) => match crate::auth::set_token(&provider, &new_tok, None) {
                        Ok(_) => {
                            crate::auth::drop_token_cache();
                            app.set_status("✓ GitLab PAT rotated");
                            crate::tui::views::auth::refresh(app);
                        }
                        Err(e) => app.set_status(format!("✗ save: {}", e)),
                    },
                    Err(e) => app.set_status(format!("✗ rotate: {}", e)),
                },
            }
            app.auth_view.focus = AuthFocus::List;
        }
        "Login (paste cloud key)" => {
            app.auth_view.focus = AuthFocus::InputToken;
            app.auth_view.input_buffer.clear();
            app.auth_view.input_prompt = "Paste gitorii_sk_… key:".to_string();
            app.auth_view.pending_op = AuthPendingOp::Login;
        }
        "Logout (cloud)" => {
            match crate::auth::delete() {
                Ok(_)  => app.set_status("✓ cloud key removed"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.auth_view.focus = AuthFocus::List;
            crate::tui::views::auth::refresh(app);
        }
        _ => {
            app.auth_view.focus = AuthFocus::List;
        }
    }
    None
}

fn handle_platform(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{PlatformFocus, PlatformSubTab};

    // Remote popup grabs everything when active.
    if app.platform_view.focus == PlatformFocus::RemotePopup {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.platform_view.remote_popup_idx > 0 {
                    app.platform_view.remote_popup_idx -= 1;
                }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.platform_view.remote_popup_idx + 1 < app.platform_view.remotes.len() {
                    app.platform_view.remote_popup_idx += 1;
                }
            }
            (_, KeyCode::Enter) => {
                if let Some(name) = app.platform_view.remotes
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
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.platform_view.dropdown_idx > 0 {
                    app.platform_view.dropdown_idx -= 1;
                }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.platform_view.dropdown_idx + 1 < ops.len() {
                    app.platform_view.dropdown_idx += 1;
                }
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
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                if app.platform_view.dropdown_idx > 0 {
                    app.platform_view.dropdown_idx -= 1;
                }
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                if app.platform_view.dropdown_idx + 1 < rows_len {
                    app.platform_view.dropdown_idx += 1;
                }
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
            (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => {
                app.platform_view.job_log_scroll = app.platform_view.job_log_scroll.saturating_sub(1);
                app.platform_view.job_log_user_scrolled = true;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                app.platform_view.job_log_scroll = app.platform_view.job_log_scroll.saturating_add(1);
                app.platform_view.job_log_user_scrolled = true;
            }
            (_, KeyCode::PageUp) => {
                app.platform_view.job_log_scroll = app.platform_view.job_log_scroll.saturating_sub(20);
                app.platform_view.job_log_user_scrolled = true;
            }
            (_, KeyCode::PageDown) => {
                app.platform_view.job_log_scroll = app.platform_view.job_log_scroll.saturating_add(20);
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

    if let Some(a) = handle_global_nav(key, app) { return Some(a); }

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
            app.platform_view.remote_popup_idx = app.platform_view.remotes
                .iter()
                .position(|n| n == &cur)
                .unwrap_or(0);
            app.platform_view.focus = PlatformFocus::RemotePopup;
        }

        // List navigation.
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            let idx = current_list_idx_mut(app);
            if *idx > 0 { *idx -= 1; }
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            let len = current_list_len(app);
            let idx = current_list_idx_mut(app);
            if *idx + 1 < len { *idx += 1; }
        }

        // 0.7.26: ops + filter dropdowns. Single key opens a menu of
        // the contextual actions / filters for the current sub-tab.
        // The dropdown handler (above) dispatches Enter/Esc/arrows.
        (_, KeyCode::Char('o')) if !app.platform_view.action_in_flight => {
            if !crate::tui::views::platform::ops_for(&app.platform_view).is_empty() {
                app.platform_view.focus = PlatformFocus::OpsDropdown;
                app.platform_view.dropdown_idx = 0;
            }
        }
        (_, KeyCode::Char('f')) => {
            app.platform_view.focus = PlatformFocus::FilterDropdown;
            app.platform_view.dropdown_idx = 0;
        }

        // Drill-down.
        (_, KeyCode::Enter) => match app.platform_view.sub_tab {
            PlatformSubTab::Pipelines => {
                if let Some(p) = app.platform_view.pipelines.get(app.platform_view.pipelines_idx) {
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
                    app.platform_view.job_log_live = matches!(
                        j.status.as_str(),
                        "running" | "pending"
                    );
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

fn dispatch_ops_action(app: &mut App) {
    use crate::tui::app::PlatformSubTab;
    let idx = app.platform_view.dropdown_idx;
    match app.platform_view.sub_tab {
        PlatformSubTab::Pipelines => {
            let id = app.platform_view.pipelines
                .get(app.platform_view.pipelines_idx).map(|p| p.id.clone());
            if let Some(id) = id {
                match idx {
                    0 => app.action_pipeline_cancel(id),
                    1 => app.action_pipeline_retry(id),
                    _ => {}
                }
            }
        }
        PlatformSubTab::Jobs => {
            let id = app.platform_view.jobs
                .get(app.platform_view.jobs_idx).map(|j| j.id.clone());
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
            let id = app.platform_view.runners
                .get(app.platform_view.runners_idx).map(|r| r.id.clone());
            if let Some(id) = id {
                match idx {
                    0 => app.action_runner_pause(id),
                    1 => app.action_runner_resume(id),
                    2 => app.action_runner_reset_token(id),
                    3 => app.action_runner_remove(id),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn dispatch_filter_action(app: &mut App) {
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

fn current_list_idx_mut(app: &mut App) -> &mut usize {
    use crate::tui::app::PlatformSubTab;
    match app.platform_view.sub_tab {
        PlatformSubTab::Pipelines => &mut app.platform_view.pipelines_idx,
        PlatformSubTab::Jobs      => &mut app.platform_view.jobs_idx,
        PlatformSubTab::Releases  => &mut app.platform_view.releases_idx,
        PlatformSubTab::Packages  => &mut app.platform_view.packages_idx,
        PlatformSubTab::Runners   => &mut app.platform_view.runners_idx,
    }
}

fn current_list_len(app: &App) -> usize {
    use crate::tui::app::PlatformSubTab;
    match app.platform_view.sub_tab {
        PlatformSubTab::Pipelines => app.platform_view.pipelines.len(),
        PlatformSubTab::Jobs      => app.platform_view.jobs.len(),
        PlatformSubTab::Releases  => app.platform_view.releases.len(),
        PlatformSubTab::Packages  => app.platform_view.packages.len(),
        PlatformSubTab::Runners   => app.platform_view.runners.len(),
    }
}

