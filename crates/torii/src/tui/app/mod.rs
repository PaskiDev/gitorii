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
mod ignore;
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
pub use ignore::*;
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
    /// New in 0.14.1 — the `.toriignore` pair, as editable rules.
    Ignore,
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
    pub ignore_view: IgnoreState,
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
            ignore_view: IgnoreState::default(),
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
            ignore_view: IgnoreState::default(),
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
    ///
    /// Must stay in sync with TABS in src/tui/ui.rs and the sidebar_idx
    /// assignments in `go_to` / `go_back`.
    fn view_for_idx(idx: usize) -> View {
        match idx {
            0 => View::Dashboard,
            1 => View::Commit,
            2 => View::Sync,
            3 => View::Snapshot,
            4 => View::Ignore,
            5 => View::Log,
            6 => View::Branch,
            7 => View::Tag,
            8 => View::Pr,
            9 => View::Issue,
            10 => View::Platform,
            11 => View::Remote,
            12 => View::Workspace,
            13 => View::Worktree,
            14 => View::Submodule,
            15 => View::Bisect,
            16 => View::Auth,
            17 => View::Config,
            _ => View::Dashboard,
        }
    }

    /// Total entries in the sidebar — keep in sync with `view_for_idx`
    /// and TABS in ui.rs.
    const SIDEBAR_LEN: usize = 18;

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
            View::Ignore => self.load_ignore_rules(),
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
            View::Ignore => 4,
            View::Log => 5,
            View::History => 5, // fused into Log
            View::Branch => 6,
            View::Tag => 7,
            View::Pr => 8,
            View::Issue => 9,
            View::Platform => 10,
            View::Remote => 11,
            View::Mirror => 11, // fused into Remote
            View::Workspace => 12,
            View::Worktree => 13,
            View::Submodule => 14,
            View::Bisect => 15,
            View::Auth => 16,
            View::Config => 17,
            View::Settings => 17, // fused into Config
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
                View::Ignore => 4,
                View::Log => 5,
                View::History => 5, // fused into Log
                View::Branch => 6,
                View::Tag => 7,
                View::Pr => 8,
                View::Issue => 9,
                View::Platform => 10,
                View::Remote => 11,
                View::Mirror => 11, // fused into Remote
                View::Workspace => 12,
                View::Worktree => 13,
                View::Submodule => 14,
                View::Bisect => 15,
                View::Auth => 16,
                View::Config => 17,
                View::Settings => 17, // fused into Config
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

    /// Run the secret scanner and put what it found in the event log.
    ///
    /// `full` scans the whole history instead of the staged files. Findings
    /// are the reason the scan was asked for, so a non-empty result also
    /// raises the event log — otherwise the answer sits behind a keypress
    /// the user has no reason to think of.
    pub fn run_secret_scan(&mut self, full: bool) {
        let path = std::path::PathBuf::from(&self.repo_path);
        let scope = if full { "history" } else { "staged files" };

        let result = if full {
            crate::scanner::scan_history(&path).map(|per_commit| {
                per_commit
                    .into_iter()
                    .flat_map(|(commit, findings)| {
                        let short = commit.chars().take(7).collect::<String>();
                        // Which commit it came from matters as much as the
                        // file when the scan covers history.
                        findings.into_iter().map(move |mut f| {
                            f.file = format!("{} {}", short, f.file);
                            f
                        })
                    })
                    .collect::<Vec<_>>()
            })
        } else {
            crate::scanner::scan_staged(&path)
        };

        match result {
            Ok(findings) if findings.is_empty() => {
                self.log_event(format!("scan: no secrets in {scope}"), EventKind::Success);
            }
            Ok(findings) => {
                let total = findings.len();
                // Oldest first, so the summary lands on top of the list.
                for line in Self::scan_event_lines(&findings, 10).into_iter().rev() {
                    self.log_event(line, EventKind::Error);
                }
                self.log_event(
                    format!("scan: {total} finding(s) in {scope}"),
                    EventKind::Error,
                );
                self.show_event_log = true;
            }
            Err(e) => self.log_event(format!("scan failed: {e}"), EventKind::Error),
        }
    }

    /// One line per finding, capped, with a tally for the rest.
    ///
    /// The event log holds `event_log_max` entries and drops the oldest, so
    /// a scan of a repo full of secrets would otherwise push every other
    /// event out of the window on its way past.
    pub fn scan_event_lines(findings: &[crate::scanner::Finding], limit: usize) -> Vec<String> {
        let mut lines: Vec<String> = findings
            .iter()
            .take(limit)
            .map(|f| format!("{}:{} — {}  {}", f.file, f.line, f.pattern_name, f.preview))
            .collect();
        if findings.len() > limit {
            lines.push(format!(
                "…and {} more — run `torii scan` for the full report",
                findings.len() - limit
            ));
        }
        lines
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
        self.resync_log_to_commits();

        Ok(())
    }

    /// Everything the log view remembers about the commit list is an index or
    /// a key into it, and `refresh` has just replaced that list — a checkout
    /// is the usual reason. Re-derive all of it, or a shorter branch leaves
    /// the renderer holding an index past the end.
    fn resync_log_to_commits(&mut self) {
        // A live search is re-run against the new commits rather than
        // dropped: the user typed it, and it still means something here.
        self.log_update_filter();

        let last = self.commits.len().saturating_sub(1);
        self.log.idx = self.log.idx.min(last);
        if !self.log.filtered.is_empty() && !self.log.filtered.contains(&self.log.idx) {
            self.log.idx = self.log.filtered[0];
        }
        self.dashboard.log_idx = self.dashboard.log_idx.min(last);
        self.sync_log_scroll();

        // The files pane and the signature column are caches keyed by the row
        // and by the commit; both belong to the list that just went away.
        self.log.last_files_idx = None;
        self.log.commit_files.clear();
        self.log.signature_cache.clear();
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
            (Some(t), Some(i)) => repo.diff_tree_to_index(Some(&t), Some(&i), Some(&mut opts)),
            (None, Some(i)) => repo.diff_tree_to_index(None, Some(&i), Some(&mut opts)),
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

    /// A checkout replaces the commit list, and everything the log view keeps
    /// about it — the search results, the selection, the files cache, the
    /// signature cache — is an index or a key into the list that just died.
    /// A branch with fewer commits than the one left behind then hands the
    /// renderer an index past the end.
    #[test]
    fn refresh_drops_state_pointing_at_the_old_commit_list() {
        let mut app = App::new().expect("a repository to look at");
        app.refresh().expect("a first load");
        let stale = app.commits.len() + 500;

        app.log.search_query = "feat".to_string();
        app.log.filtered = vec![stale];
        app.log.idx = stale;
        app.log.last_files_idx = Some(stale);
        app.log
            .signature_cache
            .insert("0000000000000000000000000000000000000000".into(), 'G');

        app.refresh().expect("a reload, as a checkout does");

        assert!(
            app.log.filtered.iter().all(|&i| i < app.commits.len()),
            "filtered still indexes the old list: {:?} into {} commits",
            app.log.filtered,
            app.commits.len()
        );
        assert!(
            app.log.idx < app.commits.len().max(1),
            "selection {} is past the end of {} commits",
            app.log.idx,
            app.commits.len()
        );
        assert_eq!(app.log.last_files_idx, None, "files cache kept its old row");
        assert!(
            app.log.signature_cache.is_empty(),
            "signature cache outlived the reload it documents as clearing it"
        );
    }

    /// End to end over a real repo: a staged secret has to come out the other
    /// side as readable events, and the log has to open itself — the whole
    /// point of the old "check event log" message that pointed at nothing.
    ///
    /// The fixture is a staged `.env`, which the scanner flags by name. A
    /// literal key here would be caught by our own pre-commit scan, and
    /// weakening that to make a test pass is the wrong trade.
    #[test]
    fn a_staged_secret_lands_in_the_event_log_and_opens_it() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        // Assembled at runtime so this line is not itself a finding.
        let planted = format!("token={}{}", "z".repeat(8), "9f3a1c");
        std::fs::write(tmp.path().join(".env"), format!("{planted}\n")).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(".env")).unwrap();
        index.write().unwrap();

        let mut app = App::test_blank();
        app.repo_path = tmp.path().to_string_lossy().into_owned();
        assert!(!app.show_event_log);

        app.run_secret_scan(false);

        assert!(app.show_event_log, "findings must raise the log");
        let messages: Vec<&str> = app.event_log.iter().map(|e| e.message.as_str()).collect();
        assert!(
            messages.first().is_some_and(
                |m| m.starts_with("scan: ") && m.contains("finding(s) in staged files")
            ),
            "a summary on top: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains(".env")),
            "the offending path belongs in the log: {messages:?}"
        );
        assert!(
            messages.iter().all(|m| !m.contains(&planted)),
            "the file's contents must never reach the log: {messages:?}"
        );
    }

    /// A clean repo says so once, and does not steal the screen.
    #[test]
    fn a_clean_scan_does_not_open_the_event_log() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();

        let mut app = App::test_blank();
        app.repo_path = tmp.path().to_string_lossy().into_owned();
        app.run_secret_scan(false);

        assert!(!app.show_event_log);
        assert_eq!(app.event_log.len(), 1);
        assert!(app.event_log[0].message.contains("no secrets"));
    }

    /// The view exists to make the difference between the two files visible,
    /// so provenance is what the state must carry — a rule that forgot which
    /// file it came from could be neither labelled nor removed.
    #[test]
    fn the_ignore_view_loads_both_files_and_keeps_them_apart() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".toriignore"),
            "build/
",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".toriignore.local"),
            "internal/
",
        )
        .unwrap();

        let mut app = App::test_blank();
        app.repo_path = tmp.path().to_string_lossy().into_owned();
        app.load_ignore_rules();

        assert_eq!(app.ignore_view.rules.len(), 2);
        assert_eq!(
            app.ignore_view.rules[0].origin,
            crate::ignore_rules::Origin::Public
        );
        assert_eq!(
            app.ignore_view.rules[1].origin,
            crate::ignore_rules::Origin::Local
        );
    }

    /// Choosing "secret" also moves the target to the private file: a pattern
    /// describing your credentials is recon material in a public repo. It can
    /// still be sent public deliberately.
    #[test]
    fn a_secret_rule_aims_at_the_private_file_by_default() {
        let mut app = App::test_blank();
        assert_eq!(
            app.ignore_view.new_origin,
            crate::ignore_rules::Origin::Public
        );

        app.ignore_toggle_kind();
        assert_eq!(app.ignore_view.new_kind, crate::ignore_rules::Kind::Secret);
        assert_eq!(
            app.ignore_view.new_origin,
            crate::ignore_rules::Origin::Local
        );

        app.ignore_toggle_origin();
        assert_eq!(
            app.ignore_view.new_origin,
            crate::ignore_rules::Origin::Public,
            "the deliberate override stays available"
        );
    }

    #[test]
    fn adding_a_rule_writes_it_and_selects_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::test_blank();
        app.repo_path = tmp.path().to_string_lossy().into_owned();
        app.load_ignore_rules();

        app.ignore_view.input = "coverage/".to_string();
        app.ignore_commit_input();

        let written = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert!(written.contains("coverage/"), "{written}");
        assert_eq!(
            app.ignore_selected().map(|r| r.pattern.as_str()),
            Some("coverage/")
        );
        assert_eq!(
            app.ignore_view.focus,
            IgnoreFocus::List,
            "the overlay closes"
        );
        assert!(app.ignore_view.input.is_empty());
    }

    /// A rejected rule must not close the overlay: what was typed is the only
    /// copy, and it is one character away from being right.
    #[test]
    fn a_bad_regex_keeps_the_text_and_says_why() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::test_blank();
        app.repo_path = tmp.path().to_string_lossy().into_owned();
        app.ignore_toggle_kind(); // secret
        app.ignore_view.focus = IgnoreFocus::Input;
        app.ignore_view.input = "AKIA[0-9".to_string();

        app.ignore_commit_input();

        assert_eq!(app.ignore_view.focus, IgnoreFocus::Input);
        assert_eq!(app.ignore_view.input, "AKIA[0-9");
        assert!(
            app.ignore_view
                .status
                .as_deref()
                .is_some_and(|s| s.contains("invalid regex")),
            "{:?}",
            app.ignore_view.status
        );
        assert!(!tmp.path().join(".toriignore.local").exists());
    }

    #[test]
    fn deleting_a_rule_takes_it_out_of_its_own_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".toriignore"),
            "build/
target/
",
        )
        .unwrap();
        let mut app = App::test_blank();
        app.repo_path = tmp.path().to_string_lossy().into_owned();
        app.load_ignore_rules();
        app.ignore_view.idx = 1; // target/

        app.ignore_delete_selected();

        let text = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert_eq!(
            text,
            "build/
"
        );
        assert_eq!(app.ignore_view.rules.len(), 1);
        assert_eq!(app.ignore_view.idx, 0, "the selection stays in range");
        assert_eq!(app.ignore_view.focus, IgnoreFocus::List);
    }

    /// The scanner used to run as a subprocess with its output sent to
    /// /dev/null, so the TUI could say "scan found issues — check event log"
    /// while the event log held nothing but that sentence. The findings are
    /// the point, and the event log is where they go.
    #[test]
    fn a_scan_reports_each_finding_and_says_how_many_it_hid() {
        let findings: Vec<_> = (0..14)
            .map(|i| crate::scanner::Finding {
                file: format!("src/f{i}.rs"),
                line: i + 1,
                pattern_name: "AWS access key".into(),
                preview: "AKIA****".into(),
            })
            .collect();

        let lines = App::scan_event_lines(&findings, 10);
        assert_eq!(lines.len(), 11, "ten findings and one tally: {lines:?}");
        assert_eq!(lines[0], "src/f0.rs:1 — AWS access key  AKIA****");
        assert!(
            lines[10].contains("4 more"),
            "the hidden ones must be counted: {}",
            lines[10]
        );
    }

    /// The preview is masked by the scanner; the event log must pass it
    /// through rather than reconstruct anything.
    #[test]
    fn a_finding_carries_the_masked_preview_and_nothing_more() {
        let finding = crate::scanner::Finding {
            file: "config.env".into(),
            line: 3,
            pattern_name: "Generic API key".into(),
            preview: "sk-1****************".into(),
        };
        let lines = App::scan_event_lines(std::slice::from_ref(&finding), 10);
        assert_eq!(
            lines,
            vec!["config.env:3 — Generic API key  sk-1****************"]
        );
    }

    #[test]
    fn a_clean_scan_produces_no_finding_lines() {
        assert!(App::scan_event_lines(&[], 10).is_empty());
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
