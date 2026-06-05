use crate::error::Result;
use git2::Repository;

mod auth;
mod bisect;
mod branch;
mod commit;
mod config;
mod dashboard;
mod diff;
mod history;
mod issue;
mod log;
mod platform;
mod pr;
mod remote;
mod settings;
mod shared;
mod snapshot;
mod submodule;
mod sync;
mod tag;
mod workspace;
mod worktree;
pub use auth::*;
pub use bisect::*;
pub use branch::*;
pub use commit::*;
pub use config::*;
pub use dashboard::*;
pub use diff::*;
pub use history::*;
pub use issue::*;
pub use log::*;
pub use platform::*;
pub use pr::*;
pub use remote::*;
pub use settings::*;
pub use shared::*;
pub use snapshot::*;
pub use submodule::*;
pub use sync::*;
pub use tag::*;
pub use workspace::*;
pub use worktree::*;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Dashboard,
    Diff,
    Log,
    Branch,
    Commit,
    Snapshot,
    Sync,
    Tag,
    /// Deprecated in 0.7.2 — merged into `Log`. Kept for back-compat with
    /// any code that still references it; the dispatcher redirects to Log.
    #[allow(dead_code)]
    History,
    Remote,
    /// Deprecated in 0.7.2 — merged into `Remote` as a panel. Dispatcher
    /// redirects to `Remote`.
    #[allow(dead_code)]
    Mirror,
    Workspace,
    Pr,
    Issue,
    /// New in 0.7.2 — per-repo and global worktrees.
    Worktree,
    /// New in 0.7.2 — submodule management.
    Submodule,
    /// New in 0.7.2 — `git bisect` state machine.
    Bisect,
    /// New in 0.7.2 — credentials (cloud key + platform tokens).
    Auth,
    /// New in 0.7.12 — unified CI/CD surface: pipelines, jobs, releases,
    /// packages across the active remote (and `--remote all` aggregations).
    Platform,
    Config,
    /// Deprecated in 0.7.2 — merged into `Config` as the "TUI" tab.
    #[allow(dead_code)]
    Settings,
    Help,
}

#[derive(Clone, PartialEq)]
pub enum EventKind {
    Error,
    Success,
    Info,
}

#[derive(Clone)]
pub struct EventEntry {
    pub timestamp: String,
    pub message: String,
    pub kind: EventKind,
}

// ── Main App ─────────────────────────────────────────────────────────────────

pub struct App {
    pub should_quit: bool,
    pub view: View,
    pub sidebar_idx: usize,
    pub sidebar_focused: bool,
    pub prev_view: Option<View>,
    pub status_msg: Option<String>,
    pub tick: usize,

    // Repo state (shared across views)
    pub repo_path: String,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,

    // File lists (shared)
    pub staged: Vec<FileEntry>,
    pub unstaged: Vec<FileEntry>,
    pub untracked: Vec<FileEntry>,
    pub commits: Vec<CommitEntry>,

    // Per-view state
    pub dashboard: DashboardState,
    pub diff: DiffState,
    pub log: LogState,
    pub branch_view: BranchState,
    pub commit_view: CommitState,
    pub snapshot_view: SnapshotState,
    pub sync_view: SyncState,
    pub tag_view: TagState,
    pub history_view: HistoryState,
    pub remote_view: RemoteState,
    pub mirror_view: MirrorState,
    pub workspace_view: WorkspaceState,
    pub pr_view: PrState,
    pub issue_view: IssueState,
    pub config_view: ConfigState,
    pub settings_view: SettingsState,
    pub settings: TuiSettings,

    // 0.7.2: views added on the TUI side
    pub worktree_view: WorktreeState,
    pub submodule_view: SubmoduleState,
    pub bisect_view: BisectState,
    pub auth_view: AuthState,

    /// 0.7.12 — unified Platform view (pipelines/jobs/releases/packages).
    pub platform_view: PlatformState,

    pub event_log: Vec<EventEntry>,
    pub show_event_log: bool,
    pub sync_rx: Option<std::sync::mpsc::Receiver<Result<String>>>,
    pub pr_rx: Option<std::sync::mpsc::Receiver<Result<Vec<PrEntry>>>>,
    pub issue_rx: Option<std::sync::mpsc::Receiver<Result<Vec<IssueEntry>>>>,

    /// 0.7.12 — background loaders for the Platform view.
    pub platform_pipelines_rx:
        Option<std::sync::mpsc::Receiver<Result<Vec<crate::pipeline::Pipeline>>>>,
    pub platform_jobs_rx: Option<std::sync::mpsc::Receiver<Result<Vec<crate::pipeline::Job>>>>,
    pub platform_releases_rx:
        Option<std::sync::mpsc::Receiver<Result<Vec<crate::release::Release>>>>,
    pub platform_packages_rx:
        Option<std::sync::mpsc::Receiver<Result<Vec<crate::package::Package>>>>,
    pub platform_runners_rx: Option<std::sync::mpsc::Receiver<Result<Vec<crate::runner::Runner>>>>,
    pub platform_job_log_rx: Option<std::sync::mpsc::Receiver<Result<String>>>,
    /// 0.7.24 — contextual actions (cancel/retry/artifacts). Sends a single
    /// `Result<message, error>` from the worker thread. The main loop pumps
    /// it into `platform_view.action_msg` and triggers a list reload.
    pub platform_action_rx: Option<std::sync::mpsc::Receiver<std::result::Result<String, String>>>,

    /// 0.7.32 — OAuth worker progress. Each tick the worker may emit
    /// a new `OauthStatus`; the main loop pumps the receiver into
    /// `auth_view.oauth_flow.status`.
    pub auth_oauth_rx: Option<std::sync::mpsc::Receiver<crate::tui::app::OauthStatus>>,

    /// 0.7.36 — armor overlay worker. The handler kicks one off when
    /// the user presses `S` over a signed commit; the worker extracts
    /// the gpgsig + verifies it; the main loop pumps the result into
    /// `log.signature_overlay`.
    pub log_signature_rx: Option<std::sync::mpsc::Receiver<crate::tui::app::SignatureOverlay>>,

    pub repo_picker_open: bool,
    pub repo_picker_idx: usize,
    pub active_workspace: Option<String>, // nombre del workspace activo, None si llegó por picker/carpeta

    /// New version available on crates.io (set asynchronously after launch)
    pub update_available: Option<String>,
    pub update_rx: Option<std::sync::mpsc::Receiver<String>>,
}

impl App {
    pub fn new() -> Result<Self> {
        let mut app = Self {
            should_quit: false,
            view: View::Dashboard,
            sidebar_idx: 0,
            sidebar_focused: true,
            prev_view: None,
            status_msg: None,
            tick: 0,
            repo_path: ".".to_string(),
            branch: String::new(),
            ahead: 0,
            behind: 0,
            staged: vec![],
            unstaged: vec![],
            untracked: vec![],
            commits: vec![],
            dashboard: DashboardState::default(),
            diff: DiffState::default(),
            log: LogState::default(),
            branch_view: BranchState::default(),
            commit_view: CommitState::default(),
            snapshot_view: SnapshotState::default(),
            sync_view: SyncState::default(),
            tag_view: TagState::default(),
            history_view: HistoryState::default(),
            remote_view: RemoteState::default(),
            mirror_view: MirrorState::default(),
            workspace_view: WorkspaceState::default(),
            pr_view: PrState::default(),
            issue_view: IssueState::default(),
            config_view: ConfigState::default(),
            settings_view: SettingsState::default(),
            settings: TuiSettings::load(),
            worktree_view: WorktreeState::default(),
            submodule_view: SubmoduleState::default(),
            bisect_view: BisectState::default(),
            auth_view: AuthState::default(),
            platform_view: PlatformState::default(),
            event_log: vec![],
            show_event_log: false,
            sync_rx: None,
            pr_rx: None,
            issue_rx: None,
            platform_pipelines_rx: None,
            platform_jobs_rx: None,
            platform_releases_rx: None,
            platform_packages_rx: None,
            platform_runners_rx: None,
            platform_job_log_rx: None,
            platform_action_rx: None,
            auth_oauth_rx: None,
            log_signature_rx: None,
            repo_picker_open: false,
            repo_picker_idx: 0,
            active_workspace: None,
            update_available: None,
            update_rx: None,
        };
        app.refresh()?;
        app.load_workspaces();
        app.spawn_update_check();
        Ok(app)
    }

    /// Blank App for unit tests — same field defaults as `new()` but
    /// without touching the repo, workspaces file, or update check.
    #[cfg(test)]
    pub(crate) fn test_blank() -> Self {
        Self {
            should_quit: false,
            view: View::Dashboard,
            sidebar_idx: 0,
            sidebar_focused: true,
            prev_view: None,
            status_msg: None,
            tick: 0,
            repo_path: ".".to_string(),
            branch: String::new(),
            ahead: 0,
            behind: 0,
            staged: vec![],
            unstaged: vec![],
            untracked: vec![],
            commits: vec![],
            dashboard: DashboardState::default(),
            diff: DiffState::default(),
            log: LogState::default(),
            branch_view: BranchState::default(),
            commit_view: CommitState::default(),
            snapshot_view: SnapshotState::default(),
            sync_view: SyncState::default(),
            tag_view: TagState::default(),
            history_view: HistoryState::default(),
            remote_view: RemoteState::default(),
            mirror_view: MirrorState::default(),
            workspace_view: WorkspaceState::default(),
            pr_view: PrState::default(),
            issue_view: IssueState::default(),
            config_view: ConfigState::default(),
            settings_view: SettingsState::default(),
            settings: TuiSettings::default(),
            worktree_view: WorktreeState::default(),
            submodule_view: SubmoduleState::default(),
            bisect_view: BisectState::default(),
            auth_view: AuthState::default(),
            platform_view: PlatformState::default(),
            event_log: vec![],
            show_event_log: false,
            sync_rx: None,
            pr_rx: None,
            issue_rx: None,
            platform_pipelines_rx: None,
            platform_jobs_rx: None,
            platform_releases_rx: None,
            platform_packages_rx: None,
            platform_runners_rx: None,
            platform_job_log_rx: None,
            platform_action_rx: None,
            auth_oauth_rx: None,
            log_signature_rx: None,
            repo_picker_open: false,
            repo_picker_idx: 0,
            active_workspace: None,
            update_available: None,
            update_rx: None,
        }
    }

    /// Run the update check on a background thread so it never blocks the TUI.
    /// Result (if any) is delivered via `update_rx` and polled in the main loop.
    fn spawn_update_check(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.update_rx = Some(rx);
        std::thread::spawn(move || {
            if let Some(v) = crate::updater::check() {
                let _ = tx.send(v);
            }
        });
    }

    /// Sidebar order. Reorganised in 0.7.26 by user flow:
    ///   - entry:          files
    ///   - local action:   save, sync, snapshot
    ///   - navigation:     log, branch, tags
    ///   - broadcast:      pr/mr, issues, platform
    ///   - multi-platform: remote, workspace, worktrees, submodules
    ///   - admin:          bisect, auth, config
    /// Must stay in sync with TABS in src/tui/ui.rs and the sidebar_idx
    /// assignments in `go_to` / `go_back`.
    fn view_for_idx(idx: usize) -> View {
        match idx {
            0 => View::Dashboard,
            1 => View::Commit,
            2 => View::Sync,
            3 => View::Snapshot,
            4 => View::Log,
            5 => View::Branch,
            6 => View::Tag,
            7 => View::Pr,
            8 => View::Issue,
            9 => View::Platform,
            10 => View::Remote,
            11 => View::Workspace,
            12 => View::Worktree,
            13 => View::Submodule,
            14 => View::Bisect,
            15 => View::Auth,
            16 => View::Config,
            _ => View::Dashboard,
        }
    }

    /// Total entries in the sidebar — keep in sync with `view_for_idx`
    /// and TABS in ui.rs.
    const SIDEBAR_LEN: usize = 17;

    pub fn sidebar_up(&mut self) {
        if self.sidebar_idx > 0 {
            self.sidebar_idx -= 1;
            let view = Self::view_for_idx(self.sidebar_idx);
            self.go_to(view);
            self.sidebar_focused = true;
        }
    }

    pub fn sidebar_down(&mut self) {
        if self.sidebar_idx + 1 < Self::SIDEBAR_LEN {
            self.sidebar_idx += 1;
            let view = Self::view_for_idx(self.sidebar_idx);
            self.go_to(view);
            self.sidebar_focused = true;
        }
    }

    pub fn sidebar_enter(&mut self) {
        let view = Self::view_for_idx(self.sidebar_idx);
        self.go_to(view);
    }

    pub fn go_to(&mut self, view: View) {
        match &view {
            View::Diff => {
                self.prev_view = Some(self.view.clone());
                self.load_diff();
            }
            View::Branch => self.load_branches(),
            View::Snapshot => self.load_snapshots(),
            View::Sync => {
                self.sync_view.status = SyncStatus::Idle;
                self.sync_view.selected_op = SyncOp::PullPush;
            }
            View::Log | View::History => {
                self.log.idx = self.dashboard.log_idx;
                self.log.scroll = 0;
                self.log.last_files_idx = None;
                self.log_load_commit_files();
            }
            View::Tag => self.load_tags(),
            View::Remote | View::Mirror => self.load_remotes(),
            View::Workspace => self.load_workspaces(),
            View::Pr => self.load_prs(),
            View::Issue => self.load_issues(),
            View::Config | View::Settings => self.load_config(),
            // 0.7.2: refresh the four new informative views on entry.
            View::Worktree => crate::tui::views::worktree::refresh(self),
            View::Submodule => crate::tui::views::submodule::refresh(self),
            View::Bisect => crate::tui::views::bisect::refresh(self),
            View::Auth => crate::tui::views::auth::refresh(self),
            // 0.7.12 — unified Platform view: discover remotes + load the
            // current sub-tab in the background.
            View::Platform => self.load_platform_enter(),
            _ => {}
        }
        // Sidebar order in 0.7.2 (16 entries, see TABS in ui.rs).
        // History / Mirror / Settings have no sidebar entry; we map them
        // to their fused destination so `go_to` from old call sites still
        // highlights something sensible.
        self.sidebar_idx = match &view {
            View::Dashboard => 0,
            View::Commit => 1,
            View::Sync => 2,
            View::Snapshot => 3,
            View::Log => 4,
            View::History => 4, // fused into Log
            View::Branch => 5,
            View::Tag => 6,
            View::Pr => 7,
            View::Issue => 8,
            View::Platform => 9,
            View::Remote => 10,
            View::Mirror => 10, // fused into Remote
            View::Workspace => 11,
            View::Worktree => 12,
            View::Submodule => 13,
            View::Bisect => 14,
            View::Auth => 15,
            View::Config => 16,
            View::Settings => 16, // fused into Config
            _ => self.sidebar_idx,
        };
        self.view = view;
        self.status_msg = None;
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.prev_view.take() {
            // Mapping must mirror `view_for_idx` + `go_to`'s sidebar_idx
            // assignments. Keep them aligned when re-ordering the sidebar.
            let idx = match &prev {
                View::Dashboard => 0,
                View::Commit => 1,
                View::Sync => 2,
                View::Snapshot => 3,
                View::Log => 4,
                View::History => 4, // fused into Log
                View::Branch => 5,
                View::Tag => 6,
                View::Pr => 7,
                View::Issue => 8,
                View::Platform => 9,
                View::Remote => 10,
                View::Mirror => 10, // fused into Remote
                View::Workspace => 11,
                View::Worktree => 12,
                View::Submodule => 13,
                View::Bisect => 14,
                View::Auth => 15,
                View::Config => 16,
                View::Settings => 16, // fused into Config
                _ => 0,
            };
            // If returning to a view with its own content, keep focus in the view
            self.sidebar_focused = matches!(prev, View::Dashboard);
            self.view = prev;
            self.sidebar_idx = idx;
        } else {
            self.view = View::Dashboard;
            self.sidebar_idx = 0;
            self.sidebar_focused = true;
        }
        self.status_msg = None;
    }

    pub fn border_type(&self) -> ratatui::widgets::BorderType {
        if self.settings.border_style == BorderStyle::Rounded {
            ratatui::widgets::BorderType::Rounded
        } else {
            ratatui::widgets::BorderType::Plain
        }
    }

    pub fn brand_color(&self) -> ratatui::style::Color {
        let (r, g, b) = self.settings.brand_color;
        ratatui::style::Color::Rgb(r, g, b)
    }

    pub fn selected_bg(&self) -> ratatui::style::Color {
        let (r, g, b) = self.settings.selected_bg;
        ratatui::style::Color::Rgb(r, g, b)
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = Some(msg.into());
    }

    pub fn log_event(&mut self, msg: impl Into<String>, kind: EventKind) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let hh = (secs % 86400) / 3600;
        let mm = (secs % 3600) / 60;
        let ss = secs % 60;
        self.event_log.insert(
            0,
            EventEntry {
                timestamp: format!("{:02}:{:02}:{:02}", hh, mm, ss),
                message: msg.into(),
                kind,
            },
        );
        let max = self.settings.event_log_max;
        if self.event_log.len() > max {
            self.event_log.truncate(max);
        }
    }

    pub fn refresh(&mut self) -> Result<()> {
        let repo = Repository::discover(&self.repo_path).map_err(crate::error::ToriiError::Git)?;

        self.branch = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .unwrap_or_else(|| "detached".to_string());

        let (ahead, behind) = ahead_behind(&repo, &self.branch).unwrap_or((0, 0));
        self.ahead = ahead;
        self.behind = behind;

        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        let statuses = repo
            .statuses(Some(&mut opts))
            .map_err(crate::error::ToriiError::Git)?;

        self.staged.clear();
        self.unstaged.clear();
        self.untracked.clear();

        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let s = entry.status();

            if s.intersects(
                git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_DELETED
                    | git2::Status::INDEX_RENAMED,
            ) {
                self.staged.push(FileEntry {
                    path: path.clone(),
                    status: FileStatus::Staged,
                });
            }
            if s.intersects(
                git2::Status::WT_MODIFIED | git2::Status::WT_DELETED | git2::Status::WT_RENAMED,
            ) {
                self.unstaged.push(FileEntry {
                    path: path.clone(),
                    status: FileStatus::Unstaged,
                });
            }
            if s.contains(git2::Status::WT_NEW) {
                self.untracked.push(FileEntry {
                    path,
                    status: FileStatus::Untracked,
                });
            }
        }

        self.commits.clear();
        let mut revwalk = repo.revwalk().map_err(crate::error::ToriiError::Git)?;
        let _ = revwalk.push_head();
        let limit = self.log.page_size + 1;
        let mut count = 0;
        for oid in revwalk.take(limit) {
            let oid = match oid {
                Ok(o) => o,
                Err(_) => continue,
            };
            count += 1;
            if count > self.log.page_size {
                break;
            }
            let commit = match repo.find_commit(oid) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let full_hash = oid.to_string();
            let hash = full_hash[..7].to_string();
            let message = commit.summary().unwrap_or("").to_string();
            let author = commit.author().name().unwrap_or("").to_string();
            let time = format_age(commit.time().seconds());
            self.commits.push(CommitEntry {
                hash,
                full_hash,
                message,
                author,
                time,
            });
        }
        self.log.all_loaded = count <= self.log.page_size;

        // Graph is always-on in Log view — recompute every reload.
        self.recompute_graph_rows();

        Ok(())
    }

    // Tab cycle: sidebar → view panels → sidebar
    // Returns true if we wrapped back to sidebar
    pub fn tab_cycle(&mut self) -> bool {
        if self.sidebar_focused {
            self.sidebar_focused = false;
            // Enter first panel of current view
            match self.view {
                View::Dashboard => self.dashboard.selected_panel = Panel::Unstaged,
                View::Workspace => self.workspace_view.focus = WorkspaceFocus::Workspaces,
                View::Commit => self.commit_view.focus = CommitFocus::List,
                _ => {}
            }
            return false;
        }
        // Cycle within view, wrap to sidebar when exhausted
        match self.view {
            View::Dashboard => {
                self.dashboard.selected_panel = match self.dashboard.selected_panel {
                    Panel::Unstaged => Panel::Untracked,
                    Panel::Untracked => Panel::Staged,
                    Panel::Staged => Panel::Log,
                    Panel::Log => {
                        self.sidebar_focused = true;
                        return true;
                    }
                };
            }
            View::Workspace => match self.workspace_view.focus {
                WorkspaceFocus::Workspaces => self.workspace_view.focus = WorkspaceFocus::Repos,
                WorkspaceFocus::Repos => {
                    self.sidebar_focused = true;
                    return true;
                }
            },
            View::Commit => match self.commit_view.focus {
                CommitFocus::List => self.commit_view.focus = CommitFocus::TypeSelector,
                CommitFocus::TypeSelector => self.commit_view.focus = CommitFocus::Input,
                CommitFocus::Input => {
                    self.sidebar_focused = true;
                    return true;
                }
            },
            _ => {
                self.sidebar_focused = true;
                return true;
            }
        }
        false
    }

    #[allow(dead_code)]
    pub fn next_panel(&mut self) {
        self.dashboard.selected_panel = match self.dashboard.selected_panel {
            Panel::Staged => Panel::Unstaged,
            Panel::Unstaged => Panel::Untracked,
            Panel::Untracked => Panel::Log,
            Panel::Log => Panel::Staged,
        };
    }

    pub fn prev_panel(&mut self) {
        self.dashboard.selected_panel = match self.dashboard.selected_panel {
            Panel::Staged => Panel::Log,
            Panel::Unstaged => Panel::Staged,
            Panel::Untracked => Panel::Unstaged,
            Panel::Log => Panel::Untracked,
        };
    }

    pub fn move_up(&mut self) {
        let d = &mut self.dashboard;
        match d.selected_panel {
            Panel::Staged => {
                if d.staged_idx > 0 {
                    d.staged_idx -= 1;
                }
            }
            Panel::Unstaged => {
                if d.unstaged_idx > 0 {
                    d.unstaged_idx -= 1;
                }
            }
            Panel::Untracked => {
                if d.untracked_idx > 0 {
                    d.untracked_idx -= 1;
                }
            }
            Panel::Log => {
                if d.log_idx > 0 {
                    d.log_idx -= 1;
                }
            }
        }
    }

    pub fn move_down(&mut self) {
        let staged_len = self.staged.len();
        let unstaged_len = self.unstaged.len();
        let untracked_len = self.untracked.len();
        let commits_len = self.commits.len();
        let d = &mut self.dashboard;
        match d.selected_panel {
            Panel::Staged => {
                if d.staged_idx + 1 < staged_len {
                    d.staged_idx += 1;
                }
            }
            Panel::Unstaged => {
                if d.unstaged_idx + 1 < unstaged_len {
                    d.unstaged_idx += 1;
                }
            }
            Panel::Untracked => {
                if d.untracked_idx + 1 < untracked_len {
                    d.untracked_idx += 1;
                }
            }
            Panel::Log => {
                if d.log_idx + 1 < commits_len {
                    d.log_idx += 1;
                }
            }
        }
    }

    // ── Diff helpers ─────────────────────────────────────────────────────────

    fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
        s.char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(s.len())
    }
}

/// List all remote names declared in the repo at `repo_path`. Empty if
/// the repo isn't discoverable. Order is whatever libgit2 returns.
fn discover_remotes(repo_path: &str) -> Vec<String> {
    let Ok(repo) = git2::Repository::discover(repo_path) else {
        return vec![];
    };
    let Ok(names) = repo.remotes() else {
        return vec![];
    };
    names.iter().flatten().map(|s| s.to_string()).collect()
}

// ── Git helpers ───────────────────────────────────────────────────────────────

fn ahead_behind(repo: &Repository, branch: &str) -> Option<(usize, usize)> {
    let local = repo
        .find_reference(&format!("refs/heads/{}", branch))
        .ok()?
        .target()?;
    let remote = repo
        .find_reference(&format!("refs/remotes/origin/{}", branch))
        .ok()?
        .target()?;
    repo.graph_ahead_behind(local, remote).ok()
}

fn read_file_diff(repo_path: &str, file_path: &str, staged: bool) -> Vec<DiffLine> {
    let Ok(repo) = Repository::discover(repo_path) else {
        return vec![];
    };
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file_path);

    let diff = if staged {
        let head = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let tree = head.as_ref().and_then(|c| c.tree().ok());
        let index = repo.index().ok();
        match (tree, index) {
            (Some(t), Some(mut i)) => {
                repo.diff_tree_to_index(Some(&t), Some(&mut i), Some(&mut opts))
            }
            (None, Some(mut i)) => repo.diff_tree_to_index(None, Some(&mut i), Some(&mut opts)),
            _ => return vec![],
        }
    } else {
        repo.diff_index_to_workdir(None, Some(&mut opts))
    };

    let Ok(diff) = diff else { return vec![] };
    diff_to_lines(&diff)
}

fn read_commit_diff(repo_path: &str, hash: &str) -> Vec<DiffLine> {
    let Ok(repo) = Repository::discover(repo_path) else {
        return vec![];
    };
    let Ok(oid) = git2::Oid::from_str(hash) else {
        return vec![];
    };
    let Ok(commit) = repo.find_commit(oid) else {
        return vec![];
    };
    let Ok(tree) = commit.tree() else {
        return vec![];
    };
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) else {
        return vec![];
    };
    diff_to_lines(&diff)
}

fn diff_to_lines(diff: &git2::Diff) -> Vec<DiffLine> {
    let mut lines = vec![];
    let _ = diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let content = String::from_utf8_lossy(line.content())
            .trim_end_matches('\n')
            .to_string();
        let (kind, line_no) = match line.origin() {
            '+' => (DiffLineKind::Added, line.new_lineno()),
            '-' => (DiffLineKind::Removed, line.old_lineno()),
            'F' => (DiffLineKind::Header, None),
            'H' => (DiffLineKind::HunkHeader, line.new_lineno()),
            _ => (DiffLineKind::Context, line.new_lineno()),
        };
        lines.push(DiffLine {
            kind,
            content,
            line_no,
        });
        true
    });
    lines
}

fn format_age(ts: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - ts;
    if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

fn shorten_remote_name(name: &str, platform: &str) -> String {
    match platform {
        "GitHub" if name.starts_with("github") => "gh".to_string(),
        "GitLab" if name.starts_with("gitlab") => "gl".to_string(),
        _ => name.to_string(),
    }
}

fn detect_platform(url: &str) -> String {
    if url.contains("github.com") {
        "GitHub".into()
    } else if url.contains("gitlab.com") {
        "GitLab".into()
    } else if url.contains("bitbucket.org") {
        "Bitbucket".into()
    } else if url.contains("codeberg.org") {
        "Codeberg".into()
    } else {
        "git".into()
    }
}

/// 0.7.39 — return the on-disk path to `workspaces.toml`. Prefers
/// the canonical XDG-style `~/.config/torii/workspaces.toml` (where
/// `torii workspace add` writes), falls back to the legacy
/// `~/.torii/workspaces.toml` for installs that pre-date the move.
pub fn workspaces_toml_path() -> Option<std::path::PathBuf> {
    let canonical = dirs::config_dir().map(|d| d.join("torii/workspaces.toml"));
    let legacy = dirs::home_dir().map(|h| h.join(".torii/workspaces.toml"));
    match (canonical.clone(), legacy.clone()) {
        (Some(p), _) if p.exists() => Some(p),
        (_, Some(p)) if p.exists() => Some(p),
        // No file yet — return the canonical path so callers that
        // want to *write* land in the right place.
        (Some(p), _) => Some(p),
        (_, Some(p)) => Some(p),
        _ => None,
    }
}

/// 0.8.1 — query `docker ps -a --filter name=torii-runner-` and
/// return one synthetic `Runner` per container. Synthetic ids use
/// the container *name* so the TUI's Detail/Ops paths (which key on
/// `id`) can map back to a `docker` command. Returns an empty vec
/// when the docker binary isn't installed or the daemon isn't up —
/// the runners list silently degrades to "platform only" instead of
/// failing the whole load.
fn list_local_runner_containers() -> Vec<crate::runner::Runner> {
    let out = std::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=torii-runner-",
            "--format",
            "{{.Names}}\t{{.State}}\t{{.Image}}",
        ])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }

    let body = String::from_utf8_lossy(&out.stdout);
    body.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            let name = cols.first().copied().unwrap_or("").to_string();
            let state = cols.get(1).copied().unwrap_or("").to_string();
            let image = cols.get(2).copied().unwrap_or("").to_string();
            // Bucket the docker state into the same labels the
            // platform reports so the existing colour table just
            // works. "running" stays running, "exited" maps onto
            // "offline" so it dims, "paused" onto our paused.
            let status = match state.as_str() {
                "running" => "online",
                "exited" => "offline",
                "paused" => "paused",
                "restarting" => "running",
                _ => "offline",
            }
            .to_string();
            crate::runner::Runner {
                id: name.clone(),
                description: name,
                status,
                paused: state == "paused",
                ip_address: String::new(),
                os: String::new(),
                tags: Vec::new(),
                version: image,
                runner_type: "local-docker".to_string(),
                web_url: String::new(),
            }
        })
        .collect()
}

fn repo_quick_status(path: &str) -> (String, usize, usize, bool) {
    let Ok(repo) = Repository::discover(path) else {
        return ("?".into(), 0, 0, false);
    };
    let branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "detached".to_string());
    let (ahead, behind) = ahead_behind(&repo, &branch).unwrap_or((0, 0));
    let dirty = repo.statuses(None).map(|s| !s.is_empty()).unwrap_or(false);
    (branch, ahead, behind, dirty)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, message: &str, author: &str) -> CommitEntry {
        CommitEntry {
            hash: hash.into(),
            full_hash: format!("{hash}000000000000000000000000000000000"),
            message: message.into(),
            author: author.into(),
            time: "now".into(),
        }
    }

    #[test]
    fn move_up_down_never_panics_on_empty_state() {
        // Every list view must tolerate up/down with zero rows.
        let views = [
            View::Dashboard,
            View::Diff,
            View::Log,
            View::Branch,
            View::Commit,
            View::Snapshot,
            View::Sync,
            View::Tag,
            View::History,
            View::Remote,
            View::Mirror,
            View::Workspace,
            View::Pr,
            View::Issue,
            View::Config,
            View::Settings,
            View::Worktree,
            View::Submodule,
            View::Bisect,
            View::Auth,
            View::Platform,
            View::Help,
        ];
        for v in views {
            let mut app = App::test_blank();
            app.view = v.clone();
            app.move_down();
            app.move_up();
            app.move_down();
        }
    }

    #[test]
    fn log_filter_matches_message_author_and_hash() {
        let mut app = App::test_blank();
        app.commits = vec![
            commit("abc1234", "feat: add login", "Alice"),
            commit("def5678", "fix: null check", "Bob"),
            commit("0099aab", "chore: bump deps", "alice"),
        ];
        app.log.search_query = "alice".into();
        app.log_update_filter();
        assert_eq!(app.log.filtered, vec![0, 2]);

        app.log.search_query = "def5678".into();
        app.log_update_filter();
        assert_eq!(app.log.filtered, vec![1]);
        // selection snapped to the first match
        assert_eq!(app.log.idx, 1);

        app.log.search_query.clear();
        app.log_update_filter();
        assert!(app.log.filtered.is_empty());
    }

    #[test]
    fn go_to_and_go_back_restore_previous_view() {
        let mut app = App::test_blank();
        app.view = View::Log;
        app.go_to(View::Diff);
        assert_eq!(app.view, View::Diff);
        app.go_back();
        assert_eq!(app.view, View::Log);
    }

    #[test]
    fn sidebar_navigation_stays_in_bounds() {
        let mut app = App::test_blank();
        for _ in 0..200 {
            app.sidebar_down();
        }
        let after_down = app.sidebar_idx;
        for _ in 0..200 {
            app.sidebar_up();
        }
        // No panic and indices stayed inside the sidebar size.
        assert!(after_down < 64);
        assert!(app.sidebar_idx < 64);
    }

    #[test]
    fn char_to_byte_idx_handles_multibyte() {
        assert_eq!(App::char_to_byte_idx("ñoño", 0), 0);
        assert_eq!(App::char_to_byte_idx("ñoño", 1), 2); // ñ = 2 bytes
        assert_eq!(App::char_to_byte_idx("ñoño", 3), 5);
        assert_eq!(App::char_to_byte_idx("ñoño", 4), 6); // end
        assert_eq!(App::char_to_byte_idx("abc", 99), 3); // clamped to len
    }

    #[test]
    fn commit_editor_handles_multibyte_input() {
        let mut app = App::test_blank();
        // "ñ" then "a" — must not panic on a non-char-boundary insert.
        app.commit_type_char('ñ');
        app.commit_type_char('a');
        assert_eq!(app.commit_view.message, "ña");
        app.commit_backspace();
        assert_eq!(app.commit_view.message, "ñ");
        app.commit_cursor_left();
        app.commit_cursor_right();
        app.commit_backspace();
        assert_eq!(app.commit_view.message, "");
    }
}
