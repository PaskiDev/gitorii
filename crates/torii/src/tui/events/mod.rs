use super::app::{
    App, BranchConfirm, CommitFocus, HistoryConfirm, IssueConfirm, Panel, PrConfirm, RemoteConfirm,
    SnapshotFocus, TagConfirm, View, WorkspaceConfirm, WorkspaceFocus,
};
use crate::error::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use std::time::Duration;

mod auth;
mod bisect;
mod branch;
mod commit;
mod config;
mod history;
mod issue;
mod log;
mod platform;
mod pr;
mod remote;
mod snapshot;
mod submodule;
mod tag;
mod workspace;
mod worktree;
use auth::*;
use bisect::*;
use branch::*;
pub use commit::COMMIT_TYPES;
use commit::*;
use config::*;
use history::*;
use issue::*;
use log::*;
use platform::*;
use pr::*;
use remote::*;
use snapshot::*;
use submodule::*;
use tag::*;
use workspace::*;
use worktree::*;

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
    pub fn new() -> Self {
        Self
    }

    pub fn next(&mut self, app: &mut App) -> Result<Option<Action>> {
        if !event::poll(Duration::from_millis(200))? {
            return Ok(None);
        }

        match event::read()? {
            Event::Key(key) => {
                // Clear event log when panel is open
                if app.show_event_log
                    && key.code == KeyCode::Char('c')
                    && key.modifiers == KeyModifiers::NONE
                {
                    app.event_log.clear();
                    return Ok(None);
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
                        (_, KeyCode::Esc) => {
                            app.repo_picker_open = false;
                        }
                        (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.repo_picker_idx > 0 => {
                            app.repo_picker_idx -= 1;
                        }
                        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                            let max = app.workspace_repo_paths().len().saturating_sub(1);
                            if app.repo_picker_idx < max {
                                app.repo_picker_idx += 1;
                            }
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
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                            return Ok(Some(Action::Quit))
                        }
                        _ => {}
                    }
                    return Ok(None);
                }

                // e toggles event log from anywhere — except when typing in a text input
                let typing = match app.view {
                    View::Commit => app.commit_view.focus == CommitFocus::Input,
                    View::Snapshot => {
                        app.snapshot_view.focus == SnapshotFocus::Create
                            || app.snapshot_view.search_mode
                    }
                    View::Log => app.log.search_mode,
                    View::Branch => {
                        app.branch_view.confirm == BranchConfirm::NewBranch
                            || app.branch_view.search_mode
                    }
                    View::Tag => {
                        matches!(
                            app.tag_view.confirm,
                            TagConfirm::CreateName | TagConfirm::CreateMessage
                        ) || app.tag_view.search_mode
                    }
                    View::History => matches!(
                        app.history_view.confirm,
                        HistoryConfirm::Rebase
                            | HistoryConfirm::RemoveFile
                            | HistoryConfirm::RewriteStart
                            | HistoryConfirm::RewriteEnd
                            | HistoryConfirm::Blame
                    ),
                    View::Remote => matches!(
                        app.remote_view.confirm,
                        RemoteConfirm::AddName
                            | RemoteConfirm::AddUrl
                            | RemoteConfirm::Rename
                            | RemoteConfirm::EditUrl
                            | RemoteConfirm::MirrorRename
                            | RemoteConfirm::MirrorAddPlatform
                            | RemoteConfirm::MirrorAddAccount
                            | RemoteConfirm::MirrorAddRepo
                    ),
                    View::Workspace => matches!(
                        app.workspace_view.confirm,
                        WorkspaceConfirm::SaveMessage | WorkspaceConfirm::AddRepoPath
                    ),
                    View::Pr => matches!(
                        app.pr_view.confirm,
                        PrConfirm::CreateTitle
                            | PrConfirm::CreateDesc
                            | PrConfirm::EditTitle
                            | PrConfirm::EditDesc
                    ),
                    View::Issue => matches!(
                        app.issue_view.confirm,
                        IssueConfirm::CreateTitle
                            | IssueConfirm::CreateDesc
                            | IssueConfirm::Comment
                    ),
                    View::Config => app.config_view.editing,
                    _ => false,
                };
                if key.code == KeyCode::Char('e') && key.modifiers == KeyModifiers::NONE && !typing
                {
                    app.show_event_log = !app.show_event_log;
                    return Ok(None);
                }
                // Sidebar navigation takes priority when focused
                // but pass Enter and action keys to the current view too
                if app.sidebar_focused {
                    let is_nav = matches!(
                        key.code,
                        KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k')
                    ) && key.modifiers == KeyModifiers::NONE;
                    let is_enter =
                        key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE;
                    let is_quit = matches!(key.code, KeyCode::Char('q'))
                        || (key.modifiers == KeyModifiers::CONTROL
                            && key.code == KeyCode::Char('c'));
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
                            View::Commit => handle_commit(key, app),
                            View::Pr => handle_pr(key, app),
                            View::Issue => handle_issue(key, app),
                            View::Branch => handle_branch(key, app),
                            View::Tag => handle_tag(key, app),
                            View::Snapshot => handle_snapshot(key, app),
                            View::Log => handle_log(key, app),
                            View::History => handle_history(key, app),
                            View::Remote | View::Mirror => handle_remote(key, app),
                            View::Workspace => handle_workspace(key, app),
                            View::Config => handle_config(key, app),
                            _ => None,
                        });
                    }
                    if is_nav || is_enter || is_quit {
                        return Ok(handle_sidebar(key, app));
                    }
                    // For action keys, delegate to view handler
                    let view_result = match app.view {
                        View::Log => handle_log(key, app),
                        View::Branch => handle_branch(key, app),
                        View::Tag => handle_tag(key, app),
                        View::History => handle_history(key, app),
                        View::Remote => handle_remote(key, app),
                        View::Mirror => handle_mirror(key, app),
                        View::Snapshot => handle_snapshot(key, app),
                        View::Workspace => handle_workspace(key, app),
                        View::Pr => handle_pr(key, app),
                        View::Issue => handle_issue(key, app),
                        _ => None,
                    };
                    if view_result.is_some() {
                        return Ok(view_result);
                    }
                    return Ok(handle_sidebar(key, app));
                }
                // Esc always returns focus to sidebar unless the view handles it specially
                if key.code == KeyCode::Esc
                    && key.modifiers == KeyModifiers::NONE
                    && app.repo_picker_open
                {
                    app.repo_picker_open = false;
                    return Ok(None);
                }
                if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
                    let handled_by_view = match app.view {
                        View::Diff => {
                            app.go_back();
                            true
                        }
                        View::Commit => app.commit_view.focus == CommitFocus::Input,
                        View::Config => app.config_view.editing,
                        View::Settings => false,
                        View::Log => {
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
                        View::Branch => {
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
                        View::Tag => {
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
                        View::History => {
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
                        View::Snapshot => {
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
                        View::Mirror if app.mirror_view.ops_mode => {
                            app.mirror_view.ops_mode = false;
                            true
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
                        View::Remote => {
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
                        // 0.7.40 — Submodule overlays close on Esc.
                        View::Submodule => {
                            use crate::tui::app::SubmoduleFocus;
                            if app.submodule_view.focus != SubmoduleFocus::List {
                                app.submodule_view.focus = SubmoduleFocus::List;
                                true
                            } else {
                                false
                            }
                        }

                        // 0.7.39 — Worktree overlays close on Esc.
                        View::Worktree => {
                            use crate::tui::app::WorktreeFocus;
                            if app.worktree_view.focus != WorktreeFocus::List {
                                app.worktree_view.focus = WorktreeFocus::List;
                                true
                            } else {
                                false
                            }
                        }

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

                        View::Platform => {
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

                        _ => false,
                    };
                    if !handled_by_view {
                        app.sidebar_focused = true;
                    }
                    return Ok(None);
                }
                return Ok(match app.view {
                    View::Dashboard => handle_dashboard(key, app),
                    View::Diff => handle_diff(key, app),
                    View::Log => handle_log(key, app),
                    View::Branch => handle_branch(key, app),
                    View::Commit => handle_commit(key, app),
                    View::Snapshot => handle_snapshot(key, app),
                    View::Sync => handle_sync(key, app),
                    View::Tag => handle_tag(key, app),
                    View::History => handle_log(key, app), // fused into Log
                    View::Remote => handle_remote(key, app),
                    View::Mirror => handle_remote(key, app), // fused into Remote
                    View::Workspace => handle_workspace(key, app),
                    View::Pr => handle_pr(key, app),
                    View::Issue => handle_issue(key, app),
                    // 0.7.2: no custom keybinds yet for the four new views;
                    // they're informative and refresh on entry. ↑/↓ etc. are
                    // handled in the generic list navigation block above.
                    View::Worktree => handle_worktree(key, app),
                    View::Submodule => handle_submodule(key, app),
                    View::Bisect => handle_bisect(key, app),
                    View::Auth => handle_auth(key, app),
                    // 0.7.12 — unified Platform view.
                    View::Platform => handle_platform(key, app),
                    View::Config => handle_config(key, app),
                    View::Settings => handle_config(key, app), // fused into Config
                    View::Help => handle_help(key, app),
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
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }

    match (key.modifiers, key.code) {
        (_, KeyCode::BackTab) => app.prev_panel(),
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.move_down(),

        (KeyModifiers::CONTROL, KeyCode::Char('r')) => return Some(Action::Refresh),

        (_, KeyCode::Char(' ')) => match app.dashboard.selected_panel {
            Panel::Unstaged | Panel::Untracked => return Some(Action::StageFile),
            Panel::Staged => return Some(Action::UnstageFile),
            Panel::Log => {}
        },

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
        (_, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            return Some(Action::Quit)
        }
        (_, KeyCode::Char('?')) => app.go_to(View::Help),
        _ => return None,
    }
    None
}

fn handle_diff(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) | (_, KeyCode::Char('q')) => app.go_back(),
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.diff_scroll_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.diff_scroll_down(),
        (_, KeyCode::PageUp) => app.diff_page_up(),
        (_, KeyCode::PageDown) => app.diff_page_down(),
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
        _ => {}
    }
    None
}

fn handle_sync(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.sync_op_prev(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.sync_op_next(),
        (_, KeyCode::Enter) => return Some(Action::SyncRun),
        _ => {}
    }
    None
}

fn handle_sidebar(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => return Some(Action::SidebarUp),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => return Some(Action::SidebarDown),
        (_, KeyCode::Enter) => return Some(Action::SidebarEnter),
        (_, KeyCode::Char('?')) => {
            app.go_to(View::Help);
        }
        (_, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            return Some(Action::Quit)
        }
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
