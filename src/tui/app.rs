use git2::Repository;
use crate::error::Result;

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

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub status: FileStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub hash: String,       // short (7 chars) for display
    pub full_hash: String,  // full 40-char hash for git ops
    pub message: String,
    pub author: String,
    pub time: String,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub line_no: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiffLineKind {
    Added,
    Removed,
    Context,
    Header,
    HunkHeader,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Panel {
    Staged,
    Unstaged,
    Untracked,
    Log,
}

// ── Dashboard state ──────────────────────────────────────────────────────────

pub struct DashboardState {
    pub selected_panel: Panel,
    pub staged_idx: usize,
    pub unstaged_idx: usize,
    pub untracked_idx: usize,
    pub log_idx: usize,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            selected_panel: Panel::Unstaged,
            staged_idx: 0,
            unstaged_idx: 0,
            untracked_idx: 0,
            log_idx: 0,
        }
    }
}

// ── Diff state ───────────────────────────────────────────────────────────────

pub struct DiffState {
    pub title: String,
    pub lines: Vec<DiffLine>,
    pub scroll: usize,
}

impl Default for DiffState {
    fn default() -> Self {
        Self { title: String::new(), lines: vec![], scroll: 0 }
    }
}

// ── Log state ────────────────────────────────────────────────────────────────

pub struct CommitFileEntry {
    pub path: String,
    pub status: char, // 'A' added, 'M' modified, 'D' deleted, 'R' renamed
}

pub struct LogState {
    pub idx: usize,
    pub scroll: usize,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
    pub page_size: usize,
    pub all_loaded: bool,
    pub commit_files: Vec<CommitFileEntry>,
    pub last_files_idx: Option<usize>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    /// Per-commit graph rows aligned with `App.commits`. Always populated in
    /// the Log view to differentiate it visually from the History view.
    pub graph_rows: Vec<crate::graph::GraphRow>,
}

impl Default for LogState {
    fn default() -> Self {
        Self {
            idx: 0,
            scroll: 0,
            search_mode: false,
            search_query: String::new(),
            filtered: vec![],
            page_size: 50,
            all_loaded: false,
            commit_files: vec![],
            last_files_idx: None,
            ops_mode: false,
            ops_idx: 0,
            graph_rows: vec![],
        }
    }
}

// ── Branch state ─────────────────────────────────────────────────────────────

pub struct BranchEntry {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BranchConfirm {
    None,
    Delete,
    NewBranch,
}

pub struct BranchState {
    pub branches: Vec<BranchEntry>,
    pub idx: usize,
    pub confirm: BranchConfirm,
    pub new_name: String,
    pub status: Option<String>,
    pub current_has_upstream: bool,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
}

impl Default for BranchState {
    fn default() -> Self {
        Self {
            branches: vec![],
            idx: 0,
            confirm: BranchConfirm::None,
            new_name: String::new(),
            status: None,
            current_has_upstream: false,
            ops_mode: false,
            ops_idx: 0,
            search_mode: false,
            search_query: String::new(),
            filtered: vec![],
        }
    }
}

// ── Commit state ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum CommitFocus {
    List,
    TypeSelector,
    Input,
}

pub struct CommitState {
    pub message: String,
    pub cursor: usize,
    pub focus: CommitFocus,
    pub type_idx: usize,
    pub amend: bool,
}

impl Default for CommitState {
    fn default() -> Self {
        Self { message: String::new(), cursor: 0, focus: CommitFocus::List, type_idx: 0, amend: false }
    }
}

// ── Snapshot state ───────────────────────────────────────────────────────────

pub struct SnapshotEntry {
    pub id: String,
    pub name: String,
    pub time: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotFocus {
    List,
    Create,
    AutoConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoSnapshotInterval {
    Off,
    Min5,
    Min15,
    Min30,
    Hour1,
}

impl AutoSnapshotInterval {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Off   => "off",
            Self::Min5  => "every 5 min",
            Self::Min15 => "every 15 min",
            Self::Min30 => "every 30 min",
            Self::Hour1 => "every 1 hour",
        }
    }
    pub fn secs(&self) -> Option<u64> {
        match self {
            Self::Off   => None,
            Self::Min5  => Some(300),
            Self::Min15 => Some(900),
            Self::Min30 => Some(1800),
            Self::Hour1 => Some(3600),
        }
    }
    pub fn all() -> &'static [AutoSnapshotInterval] {
        &[Self::Off, Self::Min5, Self::Min15, Self::Min30, Self::Hour1]
    }
}

pub struct SnapshotState {
    pub snapshots: Vec<SnapshotEntry>,
    pub idx: usize,
    pub focus: SnapshotFocus,
    pub create_name: String,
    pub auto_interval: AutoSnapshotInterval,
    pub auto_interval_idx: usize,
    pub last_auto_snapshot: u64,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
}

impl Default for SnapshotState {
    fn default() -> Self {
        Self {
            snapshots: vec![],
            idx: 0,
            focus: SnapshotFocus::List,
            create_name: String::new(),
            auto_interval: AutoSnapshotInterval::Off,
            auto_interval_idx: 0,
            last_auto_snapshot: 0,
            ops_mode: false,
            ops_idx: 0,
            search_mode: false,
            search_query: String::new(),
            filtered: vec![],
        }
    }
}

// ── Sync state ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SyncOp {
    PullPush,
    PullOnly,
    PushOnly,
    ForcePush,
    Fetch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    Idle,
    Running,
    Done(String),
    Error(String),
}

pub struct SyncState {
    pub selected_op: SyncOp,
    pub status: SyncStatus,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            selected_op: SyncOp::PullPush,
            status: SyncStatus::Idle,
        }
    }
}

// ── Tag state ────────────────────────────────────────────────────────────────

pub struct TagEntry {
    pub name: String,
    pub message: String,
    pub hash: String,
    pub time: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TagConfirm {
    None,
    Delete,
    CreateName,
    CreateMessage,
}

pub struct TagState {
    pub tags: Vec<TagEntry>,
    pub idx: usize,
    pub confirm: TagConfirm,
    pub new_name: String,
    pub new_message: String,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub search_mode: bool,
    pub search_query: String,
    pub filtered: Vec<usize>,
}

impl Default for TagState {
    fn default() -> Self {
        Self {
            tags: vec![],
            idx: 0,
            confirm: TagConfirm::None,
            new_name: String::new(),
            new_message: String::new(),
            ops_mode: false,
            ops_idx: 0,
            search_mode: false,
            search_query: String::new(),
            filtered: vec![],
        }
    }
}

// ── History state ─────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub struct ReflogEntry {
    pub id: String,
    /// Reflog `update message` (e.g. "commit: feat: …"). Surfaced in the
    /// reflog panel when the history view gains its interactive sweep
    /// (0.7.3 just renders the id column for now).
    pub message: String,
    /// Wall-clock time of the reflog entry, rendered relative.
    pub time: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HistoryConfirm {
    None,
    CherryPick,
    Clean,
    RemoveFile,
    Rebase,
    RewriteStart,
    RewriteEnd,
    Blame,
    Scan,
}

pub struct HistoryState {
    pub reflog: Vec<ReflogEntry>,
    pub idx: usize,
    pub confirm: HistoryConfirm,
    pub input: String,
    pub input2: String,
    pub scan_full: bool,
    pub ops_mode: bool,
    pub ops_idx: usize,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            reflog: vec![],
            idx: 0,
            confirm: HistoryConfirm::None,
            input: String::new(),
            input2: String::new(),
            scan_full: false,
            ops_mode: false,
            ops_idx: 0,
        }
    }
}

// ── Remote state ──────────────────────────────────────────────────────────────

pub struct RemoteEntry {
    pub name: String,
    pub git_name: String,
    pub url: String,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteConfirm {
    None,
    Remove,
    AddName,
    AddUrl,
    Rename,
    EditUrl,
    MirrorRename,
    MirrorAddPlatform,
    MirrorAddAccount,
    MirrorAddRepo,
    MirrorAddType,
}

pub struct RemoteState {
    pub remotes: Vec<RemoteEntry>,
    pub mirrors: Vec<MirrorEntry>,
    pub idx: usize,
    pub status: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub confirm: RemoteConfirm,
    pub new_name: String,
    pub new_url: String,
    pub new_mirror_platform: String,
    pub new_mirror_account: String,
    pub new_mirror_repo: String,
    pub new_mirror_type: usize, // 0=replica, 1=primary
}

impl RemoteState {
    pub fn selected_is_mirror(&self) -> bool {
        self.idx >= self.remotes.len()
    }
    pub fn selected_remote(&self) -> Option<&RemoteEntry> {
        if self.selected_is_mirror() { return None; }
        self.remotes.get(self.idx)
    }
    pub fn selected_mirror(&self) -> Option<&MirrorEntry> {
        if !self.selected_is_mirror() { return None; }
        self.mirrors.get(self.idx - self.remotes.len())
    }
    pub fn total_len(&self) -> usize {
        self.remotes.len() + self.mirrors.len()
    }
}

impl Default for RemoteState {
    fn default() -> Self {
        Self {
            remotes: vec![],
            mirrors: vec![],
            idx: 0,
            status: None,
            ops_mode: false,
            ops_idx: 0,
            confirm: RemoteConfirm::None,
            new_name: String::new(),
            new_url: String::new(),
            new_mirror_platform: String::new(),
            new_mirror_account: String::new(),
            new_mirror_repo: String::new(),
            new_mirror_type: 0,
        }
    }
}

// ── Mirror state ──────────────────────────────────────────────────────────────

pub struct MirrorEntry {
    pub name: String,
    pub platform: String,
    pub url: String,
    pub kind: String,
    pub account: String,
    pub repo: String,
}

pub struct MirrorState {
    pub mirrors: Vec<MirrorEntry>,
    pub idx: usize,
    pub status: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
}

impl Default for MirrorState {
    fn default() -> Self { Self { mirrors: vec![], idx: 0, status: None, ops_mode: false, ops_idx: 0 } }
}

// ── PR state ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PrEntry {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub head: String,
    pub base: String,
    pub author: String,
    pub url: String,
    pub draft: bool,
    pub mergeable: Option<bool>,
    pub created_at: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrStateFilter { Open, Closed, All }

#[derive(Debug, Clone, PartialEq)]
pub enum PrConfirm {
    None,
    Merge,
    Close,
    CreateTitle,
    CreateHead,
    CreateBase,
    CreateDesc,
    CreatePlatforms,
    EditTitle,
    EditDesc,
    EditBase,
    SwitchPlatform,
}

#[derive(Debug, Clone)]
pub struct PrPlatformEntry {
    pub platform: String,  // "github" / "gitlab"
    pub owner: String,
    pub repo: String,
    pub label: String,     // display: "github — paskidev/gitorii"
}

pub struct PrState {
    pub prs: Vec<PrEntry>,
    pub idx: usize,
    pub filter: PrStateFilter,
    pub loading: bool,
    pub error: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub confirm: PrConfirm,
    pub merge_method: usize, // 0=merge, 1=squash, 2=rebase
    pub platform: String,
    pub owner: String,
    pub repo_name: String,
    // create flow
    pub create_title: String,
    pub create_head: String,
    pub create_base: String,
    pub create_desc: String,
    pub create_draft: bool,
    pub create_input: String,
    // edit flow
    pub edit_input: String,
    pub edit_desc: String,
    // branch dropdown (edit base)
    pub branches: Vec<String>,
    pub branch_idx: usize,
    // platform switcher
    pub available_platforms: Vec<PrPlatformEntry>,
    pub platform_idx: usize,
    // create — platform multi-select
    pub create_platform_idx: usize,
    pub create_platform_selected: Vec<bool>,
}

impl Default for PrState {
    fn default() -> Self {
        Self {
            prs: vec![],
            idx: 0,
            filter: PrStateFilter::Open,
            loading: false,
            error: None,
            ops_mode: false,
            ops_idx: 0,
            confirm: PrConfirm::None,
            merge_method: 0,
            platform: String::new(),
            owner: String::new(),
            repo_name: String::new(),
            create_title: String::new(),
            create_head: String::new(),
            create_base: String::new(),
            create_desc: String::new(),
            create_draft: false,
            create_input: String::new(),
            edit_input: String::new(),
            edit_desc: String::new(),
            branches: vec![],
            branch_idx: 0,
            available_platforms: vec![],
            platform_idx: 0,
            create_platform_idx: 0,
            create_platform_selected: vec![],
        }
    }
}

// ── Issue state ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IssueEntry {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub url: String,
    pub labels: Vec<String>,
    pub comments: u64,
    pub created_at: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueConfirm {
    None,
    Close,
    CreateTitle,
    CreateDesc,
    Comment,
}

pub struct IssueState {
    pub issues: Vec<IssueEntry>,
    pub idx: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub confirm: IssueConfirm,
    pub platform: String,
    pub owner: String,
    pub repo_name: String,
    pub create_title: String,
    pub create_desc: String,
    pub create_input: String,
    pub comment_input: String,
}

impl Default for IssueState {
    fn default() -> Self {
        Self {
            issues: vec![],
            idx: 0,
            loading: false,
            error: None,
            ops_mode: false,
            ops_idx: 0,
            confirm: IssueConfirm::None,
            platform: String::new(),
            owner: String::new(),
            repo_name: String::new(),
            create_title: String::new(),
            create_desc: String::new(),
            create_input: String::new(),
            comment_input: String::new(),
        }
    }
}

// ── Workspace state ───────────────────────────────────────────────────────────

pub struct WorkspaceRepo {
    pub path: String,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub dirty: bool,
}

pub struct WorkspaceEntry {
    pub name: String,
    pub repos: Vec<WorkspaceRepo>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceFocus { Workspaces, Repos }

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceConfirm {
    None,
    DeleteWorkspace,
    RemoveRepo,
    SaveMessage,
    AddRepoPath,
    RenameWorkspace,
}

pub struct WorkspaceState {
    pub workspaces: Vec<WorkspaceEntry>,
    pub ws_idx: usize,
    pub repo_idx: usize,
    pub focus: WorkspaceFocus,
    pub status: Option<String>,
    pub ops_mode: bool,
    pub ops_idx: usize,
    pub confirm: WorkspaceConfirm,
    pub input: String,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            workspaces: vec![],
            ws_idx: 0,
            repo_idx: 0,
            focus: WorkspaceFocus::Workspaces,
            status: None,
            ops_mode: false,
            ops_idx: 0,
            confirm: WorkspaceConfirm::None,
            input: String::new(),
        }
    }
}

// ── Config state ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigScope { Global, Local }

#[allow(dead_code)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub scope: ConfigScope,
    pub section: String,
}

pub struct ConfigState {
    pub entries: Vec<ConfigEntry>,
    pub idx: usize,
    pub editing: bool,
    pub edit_buf: String,
    pub edit_cursor: usize,
    pub scope: ConfigScope,
    pub status: Option<String>,
}

impl Default for ConfigState {
    fn default() -> Self {
        Self {
            entries: vec![],
            idx: 0,
            editing: false,
            edit_buf: String::new(),
            edit_cursor: 0,
            scope: ConfigScope::Global,
            status: None,
        }
    }
}

// ── Settings state ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum BorderStyle { Rounded, Sharp }

#[derive(Debug, Clone)]
pub struct TuiSettings {
    pub border_style: BorderStyle,
    pub show_help_view: bool,
    pub show_history_view: bool,
    pub show_mirror_view: bool,
    pub show_workspace_view: bool,
    pub show_remote_view: bool,
    pub brand_color: (u8, u8, u8),
    pub selected_bg: (u8, u8, u8),
    pub event_log_max: usize,
    pub graph_style: crate::graph::GraphStyle,
}

impl Default for TuiSettings {
    fn default() -> Self {
        Self {
            border_style: BorderStyle::Rounded,
            show_help_view: true,
            show_history_view: true,
            show_mirror_view: true,
            show_workspace_view: true,
            show_remote_view: true,
            brand_color: (255, 76, 76),
            selected_bg: (40, 40, 60),
            event_log_max: 50,
            graph_style: crate::graph::GraphStyle::Curves,
        }
    }
}

impl TuiSettings {
    pub fn load() -> Self {
        let path = dirs::home_dir()
            .map(|h| h.join(".torii/tui-settings.toml"))
            .unwrap_or_default();
        if !path.exists() { return Self::default(); }
        let Ok(content) = std::fs::read_to_string(&path) else { return Self::default(); };
        let mut s = Self::default();
        for line in content.lines() {
            let line = line.trim();
            let mut parts = line.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim();
            let val = parts.next().unwrap_or("").trim().trim_matches('"');
            match key {
                "border_style"       => s.border_style = if val == "sharp" { BorderStyle::Sharp } else { BorderStyle::Rounded },
                "show_help_view"     => s.show_help_view = val != "false",
                "show_history_view"  => s.show_history_view = val != "false",
                "show_mirror_view"   => s.show_mirror_view = val != "false",
                "show_workspace_view"=> s.show_workspace_view = val != "false",
                "show_remote_view"   => s.show_remote_view = val != "false",
                "brand_color"        => { if let Some(rgb) = parse_rgb(val) { s.brand_color = rgb; } }
                "selected_bg"        => { if let Some(rgb) = parse_rgb(val) { s.selected_bg = rgb; } }
                "event_log_max"      => { if let Ok(n) = val.parse::<usize>() { s.event_log_max = n; } }
                "graph_style"        => { s.graph_style = crate::graph::GraphStyle::from_str(val); }
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let path = dirs::home_dir()
            .map(|h| h.join(".torii/tui-settings.toml"))
            .unwrap_or_default();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = format!(
            "border_style = \"{}\"\nshow_help_view = {}\nshow_history_view = {}\nshow_mirror_view = {}\nshow_workspace_view = {}\nshow_remote_view = {}\nbrand_color = \"{},{},{}\"\nselected_bg = \"{},{},{}\"\nevent_log_max = {}\ngraph_style = \"{}\"\n",
            if self.border_style == BorderStyle::Rounded { "rounded" } else { "sharp" },
            self.show_help_view, self.show_history_view, self.show_mirror_view,
            self.show_workspace_view, self.show_remote_view,
            self.brand_color.0, self.brand_color.1, self.brand_color.2,
            self.selected_bg.0, self.selected_bg.1, self.selected_bg.2,
            self.event_log_max,
            self.graph_style.as_str(),
        );
        let _ = std::fs::write(path, content);
    }
}

fn parse_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 { return None; }
    Some((
        parts[0].trim().parse().ok()?,
        parts[1].trim().parse().ok()?,
        parts[2].trim().parse().ok()?,
    ))
}

pub struct SettingsState {
    pub idx: usize,
    pub status: Option<String>,
}

impl Default for SettingsState {
    fn default() -> Self { Self { idx: 0, status: None } }
}

// -- Worktree view ---------------------------------------------------------

#[derive(Clone)]
pub struct WorktreeEntry {
    pub name: String,
    pub path: String,
    pub branch: String,
    pub state: String, // "clean" / "N change(s)" / "locked: <reason>"
    pub is_main: bool,
}

pub struct WorktreeState {
    pub items: Vec<WorktreeEntry>,
    pub idx: usize,
    pub status: Option<String>,
}

impl Default for WorktreeState {
    fn default() -> Self { Self { items: Vec::new(), idx: 0, status: None } }
}

// -- Submodule view --------------------------------------------------------

#[derive(Clone)]
pub struct SubmoduleEntry {
    pub name: String,
    pub path: String,
    pub url: String,
    pub head_oid: String,
    pub workdir_oid: String,
    pub state: String,
}

pub struct SubmoduleState {
    pub items: Vec<SubmoduleEntry>,
    pub idx: usize,
    pub status: Option<String>,
}

impl Default for SubmoduleState {
    fn default() -> Self { Self { items: Vec::new(), idx: 0, status: None } }
}

// -- Bisect view -----------------------------------------------------------

#[derive(Clone, Default)]
pub struct BisectState {
    /// Is a bisect session currently in flight (`.git/BISECT_START` exists).
    pub in_progress: bool,
    pub current_hash: Option<String>,
    pub good_refs: Vec<String>,
    pub bad_refs: Vec<String>,
    /// How many steps git estimates remain. libgit2 doesn't expose this
    /// so we leave it `None` for now; populated once we compute it from
    /// the reachable revwalk between good/bad in 0.7.3+.
    #[allow(dead_code)]
    pub steps_left_estimate: Option<usize>,
    pub status: Option<String>,
}

// -- Auth view -------------------------------------------------------------

#[derive(Clone)]
pub struct AuthEntry {
    pub provider: String,
    pub masked: Option<String>,
    pub source: String, // "global" / "local" / "env: $VAR" / "(not set)"
}

pub struct AuthState {
    pub items: Vec<AuthEntry>,
    pub idx: usize,
    pub status: Option<String>,
    pub cloud_key_set: bool,
    pub cloud_endpoint: String,

    /// 0.7.30 — interactive ops state. `focus` says what overlay (if
    /// any) is currently active; `dropdown_idx` is the selection in
    /// the ops menu; `input_buffer` holds the pasted token while in
    /// `InputToken`; `pending_provider` captures which provider the
    /// in-flight operation applies to.
    pub focus: AuthFocus,
    pub dropdown_idx: usize,
    pub input_buffer: String,
    pub input_prompt: String,
    pub pending_provider: String,
    pub pending_op: AuthPendingOp,

    /// 0.7.32 — in-flight OAuth modal. Some when an OAuth/rotate flow
    /// is running or just finished and waiting for the user to close
    /// the dialog.
    pub oauth_flow: Option<OauthFlowState>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthFocus {
    List,
    OpsDropdown,
    InputToken,
    ConfirmRemove,
    /// 0.7.32 — OAuth flow modal driven from inside the TUI.
    /// Shows the verification URL + user code while a background
    /// worker polls the platform.
    OauthFlow,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthPendingOp {
    None,
    SetToken,
    Login,
}

/// 0.7.32 — visible state of the in-TUI OAuth flow modal. Updated
/// from the background worker over a channel and read by the
/// `auth` view's renderer + the global hint bar.
#[derive(Debug, Clone)]
pub enum OauthStatus {
    /// Doing the initial POST that asks the platform for a device
    /// code (typically <1s). Nothing for the user to copy yet.
    Starting,
    /// User should open `display_uri` and (if needed) paste `user_code`.
    /// We are polling the token endpoint every `interval` seconds in
    /// the background.
    Waiting { display_uri: String, user_code: String },
    /// Token in hand; storing it now + (for rotate) revoking the old.
    Saving,
    /// All done; modal shows the success and the next keystroke closes
    /// it.
    Done(String),
    /// Something blew up; same idea — keystroke closes.
    Error(String),
}

#[derive(Debug, Clone)]
pub struct OauthFlowState {
    pub provider: String,
    /// When false this is a plain re-auth; when true the worker has
    /// captured the old token and will best-effort revoke it after
    /// the new one is saved. The old value lives only inside the
    /// worker's closure, never in this struct.
    pub rotate: bool,
    pub status: OauthStatus,
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            idx: 0,
            status: None,
            cloud_key_set: false,
            cloud_endpoint: String::new(),
            focus: AuthFocus::List,
            dropdown_idx: 0,
            input_buffer: String::new(),
            input_prompt: String::new(),
            pending_provider: String::new(),
            pending_op: AuthPendingOp::None,
            oauth_flow: None,
        }
    }
}

// -- Platform view (0.7.12) ------------------------------------------------
//
// Unified surface for the per-platform CI/CD objects exposed by the CLI
// (`torii pipeline|job|release|package`). The view groups the four into
// horizontal sub-tabs and lets you drill from a pipeline into its jobs
// and from a job into its trace.

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformSubTab { Pipelines, Jobs, Releases, Packages, Runners }

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformFocus {
    /// Browsing the active sub-tab list.
    List,
    /// Drill-down inside Jobs (entered from a pipeline). The list now
    /// shows jobs of `active_pipeline_id`; Esc returns to Pipelines.
    JobsOfPipeline,
    /// Drill-down inside a single job's log/trace.
    JobLog,
    /// Remote-selector popup is open over the view.
    RemotePopup,
    /// 0.7.26: contextual-actions dropdown (cancel/retry/pause/etc.)
    /// opened with `o`. Replaces the individual c/x/a/t/d keybinds
    /// from 0.7.24 — those collided across sub-tabs (c=cancel in
    /// Pipelines vs c=pause in Runners) and weren't discoverable.
    OpsDropdown,
    /// 0.7.26: filter dropdown (status + branch) opened with `f`.
    FilterDropdown,
}

pub struct PlatformState {
    pub sub_tab: PlatformSubTab,
    pub focus: PlatformFocus,

    /// Remote name currently consulted (e.g. "origin", "github", "upstream").
    /// Always one concrete remote in 0.7.12; `--remote all` is CLI-only.
    pub remote: String,
    /// Auto-discovered remote list (populated on view enter).
    pub remotes: Vec<String>,
    pub remote_popup_idx: usize,

    /// Resolved platform/owner/repo for `remote`. Updated when remote changes.
    pub platform: String,
    pub owner: String,
    pub repo_name: String,

    pub pipelines: Vec<crate::pipeline::Pipeline>,
    pub pipelines_idx: usize,
    pub jobs: Vec<crate::pipeline::Job>,
    pub jobs_idx: usize,
    pub releases: Vec<crate::release::Release>,
    pub releases_idx: usize,
    pub packages: Vec<crate::package::Package>,
    pub packages_idx: usize,
    pub runners: Vec<crate::runner::Runner>,
    pub runners_idx: usize,

    /// Set when we drilled from a pipeline row into its jobs.
    pub active_pipeline_id: Option<u64>,
    /// Job trace text + scroll, when focus == JobLog.
    pub job_log: Option<String>,
    pub job_log_scroll: u16,

    pub loading: bool,
    pub error: Option<String>,

    /// 0.7.24 — gate for contextual actions. While true, ops keys are
    /// ignored so the user can't fire five retries by mashing Enter.
    /// 0.7.27: the result/feedback no longer lives here — it goes to the
    /// app-wide `status_msg` (the single source of "what just happened")
    /// and to the event log, like every other view does.
    pub action_in_flight: bool,

    /// 0.7.24 — auto-refresh of the active list while in List focus.
    /// Off by default; the user toggles with `p`. Interval is 10s by
    /// default, tunable later via settings.
    pub auto_refresh: bool,
    pub last_poll_at: Option<std::time::Instant>,

    /// 0.7.24 — live tail of the job log. Enabled automatically when
    /// drilling into a running/pending job; stops when the job reaches
    /// a terminal status. `job_log_user_scrolled` blocks auto-bottom
    /// so the user can read past lines without being yanked forward.
    pub job_log_live: bool,
    pub job_log_last_poll_at: Option<std::time::Instant>,
    pub job_log_user_scrolled: bool,
    /// Status of the job the log belongs to (for the "● live" indicator
    /// and for deciding when to stop polling).
    pub job_log_status: String,

    /// 0.7.24 — list filters. `filter_status` is set from a dropdown
    /// (None / running / failed / success / pending) and passed to the
    /// platform API. `filter_branch` toggles client-side filtering by
    /// the local repo's current branch.
    pub filter_status: Option<String>,
    pub filter_branch_only: bool,

    /// 0.7.26 — index of the currently highlighted dropdown row when
    /// `focus == OpsDropdown` or `FilterDropdown`. Reset to 0 on open.
    pub dropdown_idx: usize,
}

impl Default for PlatformState {
    fn default() -> Self {
        Self {
            sub_tab: PlatformSubTab::Pipelines,
            focus: PlatformFocus::List,
            remote: "origin".to_string(),
            remotes: vec![],
            remote_popup_idx: 0,
            platform: String::new(),
            owner: String::new(),
            repo_name: String::new(),
            pipelines: vec![],
            pipelines_idx: 0,
            jobs: vec![],
            jobs_idx: 0,
            releases: vec![],
            releases_idx: 0,
            packages: vec![],
            packages_idx: 0,
            runners: vec![],
            runners_idx: 0,
            active_pipeline_id: None,
            job_log: None,
            job_log_scroll: 0,
            loading: false,
            error: None,
            action_in_flight: false,
            auto_refresh: false,
            last_poll_at: None,
            job_log_live: false,
            job_log_last_poll_at: None,
            job_log_user_scrolled: false,
            job_log_status: String::new(),
            filter_status: None,
            filter_branch_only: false,
            dropdown_idx: 0,
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum EventKind { Error, Success, Info }

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
    pub platform_pipelines_rx: Option<std::sync::mpsc::Receiver<Result<Vec<crate::pipeline::Pipeline>>>>,
    pub platform_jobs_rx: Option<std::sync::mpsc::Receiver<Result<Vec<crate::pipeline::Job>>>>,
    pub platform_releases_rx: Option<std::sync::mpsc::Receiver<Result<Vec<crate::release::Release>>>>,
    pub platform_packages_rx: Option<std::sync::mpsc::Receiver<Result<Vec<crate::package::Package>>>>,
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

    /// 0.7.32 — start an in-TUI OAuth flow for `provider`. Worker
    /// thread does init → emits `Waiting{url, code}` → polls every
    /// `interval` → emits `Saving` → calls `auth::set_token` →
    /// (optionally) revokes `old_token` → emits `Done(masked)` /
    /// `Error(msg)`. The view stays open while this runs; the user
    /// can copy the URL/code and authorise in the browser without
    /// the TUI ever leaving the alt screen.
    pub fn start_oauth_flow(&mut self, provider: String, rotate: bool, old_token: Option<String>) {
        self.auth_view.oauth_flow = Some(crate::tui::app::OauthFlowState {
            provider: provider.clone(),
            rotate,
            status: crate::tui::app::OauthStatus::Starting,
        });
        self.auth_view.focus = crate::tui::app::AuthFocus::OauthFlow;

        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_oauth_rx = Some(rx);

        std::thread::spawn(move || {
            use crate::tui::app::OauthStatus;

            // 1. Init.
            let mut session = match crate::oauth::start_device_flow(&provider) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(OauthStatus::Error(e.to_string()));
                    return;
                }
            };
            let _ = tx.send(OauthStatus::Waiting {
                display_uri: session.display_uri.clone(),
                user_code: session.user_code.clone(),
            });

            // 2. Poll until done or error.
            let token = loop {
                std::thread::sleep(crate::oauth::device_flow_interval(&session));
                match crate::oauth::poll_device_flow(&mut session) {
                    Ok(crate::oauth::DeviceFlowStep::Pending)
                    | Ok(crate::oauth::DeviceFlowStep::SlowDown) => continue,
                    Ok(crate::oauth::DeviceFlowStep::Done(t)) => break t,
                    Err(e) => {
                        let _ = tx.send(OauthStatus::Error(e.to_string()));
                        return;
                    }
                }
            };

            // 3. Save + optional revoke.
            let _ = tx.send(OauthStatus::Saving);
            if let Err(e) = crate::auth::set_token(&provider, &token, None) {
                let _ = tx.send(OauthStatus::Error(format!("save: {}", e)));
                return;
            }
            if rotate {
                if let Some(old) = old_token {
                    let _ = crate::oauth::revoke_token(&provider, &old);
                }
            }
            // Drop the in-process cache so the rest of the TUI picks
            // up the new value on the next resolve_token (the parent
            // process didn't see set_token invalidate it).
            crate::auth::drop_token_cache();

            let chars: Vec<char> = token.chars().collect();
            let masked = if chars.len() < 12 {
                "****".to_string()
            } else {
                let head: String = chars.iter().take(6).collect();
                let tail: String = chars.iter().skip(chars.len() - 4).collect();
                format!("{}…{}", head, tail)
            };
            let _ = tx.send(OauthStatus::Done(masked));
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
            0  => View::Dashboard,
            1  => View::Commit,
            2  => View::Sync,
            3  => View::Snapshot,
            4  => View::Log,
            5  => View::Branch,
            6  => View::Tag,
            7  => View::Pr,
            8  => View::Issue,
            9  => View::Platform,
            10 => View::Remote,
            11 => View::Workspace,
            12 => View::Worktree,
            13 => View::Submodule,
            14 => View::Bisect,
            15 => View::Auth,
            16 => View::Config,
            _  => View::Dashboard,
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

    pub fn go_to_diff_from_log(&mut self) {
        self.prev_view = Some(self.view.clone());
        self.load_commit_diff_from_log();
        self.view = View::Diff;
        self.status_msg = None;
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
            View::Tag       => self.load_tags(),
            View::Remote | View::Mirror => self.load_remotes(),
            View::Workspace => self.load_workspaces(),
            View::Pr        => self.load_prs(),
            View::Issue     => self.load_issues(),
            View::Config | View::Settings => self.load_config(),
            // 0.7.2: refresh the four new informative views on entry.
            View::Worktree  => crate::tui::views::worktree::refresh(self),
            View::Submodule => crate::tui::views::submodule::refresh(self),
            View::Bisect    => crate::tui::views::bisect::refresh(self),
            View::Auth      => crate::tui::views::auth::refresh(self),
            // 0.7.12 — unified Platform view: discover remotes + load the
            // current sub-tab in the background.
            View::Platform  => self.load_platform_enter(),
            _ => {}
        }
        // Sidebar order in 0.7.2 (16 entries, see TABS in ui.rs).
        // History / Mirror / Settings have no sidebar entry; we map them
        // to their fused destination so `go_to` from old call sites still
        // highlights something sensible.
        self.sidebar_idx = match &view {
            View::Dashboard => 0,
            View::Commit    => 1,
            View::Sync      => 2,
            View::Snapshot  => 3,
            View::Log       => 4,
            View::History   => 4, // fused into Log
            View::Branch    => 5,
            View::Tag       => 6,
            View::Pr        => 7,
            View::Issue     => 8,
            View::Platform  => 9,
            View::Remote    => 10,
            View::Mirror    => 10, // fused into Remote
            View::Workspace => 11,
            View::Worktree  => 12,
            View::Submodule => 13,
            View::Bisect    => 14,
            View::Auth      => 15,
            View::Config    => 16,
            View::Settings  => 16, // fused into Config
            _               => self.sidebar_idx,
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
                View::Commit    => 1,
                View::Sync      => 2,
                View::Snapshot  => 3,
                View::Log       => 4,
                View::History   => 4, // fused into Log
                View::Branch    => 5,
                View::Tag       => 6,
                View::Pr        => 7,
                View::Issue     => 8,
                View::Platform  => 9,
                View::Remote    => 10,
                View::Mirror    => 10, // fused into Remote
                View::Workspace => 11,
                View::Worktree  => 12,
                View::Submodule => 13,
                View::Bisect    => 14,
                View::Auth      => 15,
                View::Config    => 16,
                View::Settings  => 16, // fused into Config
                _               => 0,
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
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let hh = (secs % 86400) / 3600;
        let mm = (secs % 3600) / 60;
        let ss = secs % 60;
        self.event_log.insert(0, EventEntry {
            timestamp: format!("{:02}:{:02}:{:02}", hh, mm, ss),
            message: msg.into(),
            kind,
        });
        let max = self.settings.event_log_max;
        if self.event_log.len() > max {
            self.event_log.truncate(max);
        }
    }

    pub fn refresh(&mut self) -> Result<()> {
        let repo = Repository::discover(&self.repo_path)
            .map_err(crate::error::ToriiError::Git)?;

        self.branch = repo.head().ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .unwrap_or_else(|| "detached".to_string());

        let (ahead, behind) = ahead_behind(&repo, &self.branch).unwrap_or((0, 0));
        self.ahead = ahead;
        self.behind = behind;

        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        let statuses = repo.statuses(Some(&mut opts))
            .map_err(crate::error::ToriiError::Git)?;

        self.staged.clear();
        self.unstaged.clear();
        self.untracked.clear();

        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let s = entry.status();

            if s.intersects(
                git2::Status::INDEX_NEW | git2::Status::INDEX_MODIFIED |
                git2::Status::INDEX_DELETED | git2::Status::INDEX_RENAMED
            ) {
                self.staged.push(FileEntry { path: path.clone(), status: FileStatus::Staged });
            }
            if s.intersects(
                git2::Status::WT_MODIFIED | git2::Status::WT_DELETED | git2::Status::WT_RENAMED
            ) {
                self.unstaged.push(FileEntry { path: path.clone(), status: FileStatus::Unstaged });
            }
            if s.contains(git2::Status::WT_NEW) {
                self.untracked.push(FileEntry { path, status: FileStatus::Untracked });
            }
        }

        self.commits.clear();
        let mut revwalk = repo.revwalk().map_err(crate::error::ToriiError::Git)?;
        let _ = revwalk.push_head();
        let limit = self.log.page_size + 1;
        let mut count = 0;
        for oid in revwalk.take(limit) {
            let oid = match oid { Ok(o) => o, Err(_) => continue };
            count += 1;
            if count > self.log.page_size { break; }
            let commit = match repo.find_commit(oid) { Ok(c) => c, Err(_) => continue };
            let full_hash = oid.to_string();
            let hash = full_hash[..7].to_string();
            let message = commit.summary().unwrap_or("").to_string();
            let author = commit.author().name().unwrap_or("").to_string();
            let time = format_age(commit.time().seconds());
            self.commits.push(CommitEntry { hash, full_hash, message, author, time });
        }
        self.log.all_loaded = count <= self.log.page_size;

        // Graph is always-on in Log view — recompute every reload.
        self.recompute_graph_rows();

        Ok(())
    }

    /// Recompute graph rows from `self.commits`. Cheap (≤ a few hundred
    /// commits in TUI). No-op if commits empty.
    pub fn recompute_graph_rows(&mut self) {
        use crate::graph::{render_with, GraphCommit};
        if self.commits.is_empty() {
            self.log.graph_rows.clear();
            return;
        }
        let repo = match git2::Repository::open(&self.repo_path) {
            Ok(r) => r,
            Err(_) => {
                self.log.graph_rows.clear();
                return;
            }
        };
        let input: Vec<GraphCommit> = self
            .commits
            .iter()
            .map(|c| {
                let parents = git2::Oid::from_str(&c.full_hash)
                    .ok()
                    .and_then(|oid| repo.find_commit(oid).ok())
                    .map(|commit| commit.parent_ids().map(|p| p.to_string()).collect())
                    .unwrap_or_default();
                GraphCommit {
                    id: c.full_hash.clone(),
                    parents,
                }
            })
            .collect();
        // BubblesX is a CLI-only expanded layout (one extra padding line per
        // commit) that breaks the TUI's 1:1 commit→ListItem indexing. Degrade
        // to its compact sibling for TUI rendering.
        let style = match self.settings.graph_style {
            crate::graph::GraphStyle::BubblesX => crate::graph::GraphStyle::Bubbles,
            other => other,
        };
        self.log.graph_rows = render_with(&input, style);
    }

    // ── Dashboard helpers ────────────────────────────────────────────────────

    // Tab cycle: sidebar → view panels → sidebar
    // Returns true if we wrapped back to sidebar
    pub fn tab_cycle(&mut self) -> bool {
        if self.sidebar_focused {
            self.sidebar_focused = false;
            // Enter first panel of current view
            match self.view {
                View::Dashboard => self.dashboard.selected_panel = Panel::Unstaged,
                View::Workspace => self.workspace_view.focus = WorkspaceFocus::Workspaces,
                View::Commit    => self.commit_view.focus = CommitFocus::List,
                _ => {}
            }
            return false;
        }
        // Cycle within view, wrap to sidebar when exhausted
        match self.view {
            View::Dashboard => {
                self.dashboard.selected_panel = match self.dashboard.selected_panel {
                    Panel::Unstaged  => Panel::Untracked,
                    Panel::Untracked => Panel::Staged,
                    Panel::Staged    => Panel::Log,
                    Panel::Log       => { self.sidebar_focused = true; return true; }
                };
            }
            View::Workspace => {
                match self.workspace_view.focus {
                    WorkspaceFocus::Workspaces => self.workspace_view.focus = WorkspaceFocus::Repos,
                    WorkspaceFocus::Repos      => { self.sidebar_focused = true; return true; }
                }
            }
            View::Commit => {
                match self.commit_view.focus {
                    CommitFocus::List         => self.commit_view.focus = CommitFocus::TypeSelector,
                    CommitFocus::TypeSelector => self.commit_view.focus = CommitFocus::Input,
                    CommitFocus::Input        => { self.sidebar_focused = true; return true; }
                }
            }
            _ => { self.sidebar_focused = true; return true; }
        }
        false
    }

    #[allow(dead_code)]
    pub fn next_panel(&mut self) {
        self.dashboard.selected_panel = match self.dashboard.selected_panel {
            Panel::Staged    => Panel::Unstaged,
            Panel::Unstaged  => Panel::Untracked,
            Panel::Untracked => Panel::Log,
            Panel::Log       => Panel::Staged,
        };
    }

    pub fn prev_panel(&mut self) {
        self.dashboard.selected_panel = match self.dashboard.selected_panel {
            Panel::Staged    => Panel::Log,
            Panel::Unstaged  => Panel::Staged,
            Panel::Untracked => Panel::Unstaged,
            Panel::Log       => Panel::Untracked,
        };
    }

    pub fn move_up(&mut self) {
        let d = &mut self.dashboard;
        match d.selected_panel {
            Panel::Staged    => { if d.staged_idx > 0    { d.staged_idx -= 1; } }
            Panel::Unstaged  => { if d.unstaged_idx > 0  { d.unstaged_idx -= 1; } }
            Panel::Untracked => { if d.untracked_idx > 0 { d.untracked_idx -= 1; } }
            Panel::Log       => { if d.log_idx > 0       { d.log_idx -= 1; } }
        }
    }

    pub fn move_down(&mut self) {
        let staged_len    = self.staged.len();
        let unstaged_len  = self.unstaged.len();
        let untracked_len = self.untracked.len();
        let commits_len   = self.commits.len();
        let d = &mut self.dashboard;
        match d.selected_panel {
            Panel::Staged    => { if d.staged_idx + 1 < staged_len       { d.staged_idx += 1; } }
            Panel::Unstaged  => { if d.unstaged_idx + 1 < unstaged_len   { d.unstaged_idx += 1; } }
            Panel::Untracked => { if d.untracked_idx + 1 < untracked_len { d.untracked_idx += 1; } }
            Panel::Log       => { if d.log_idx + 1 < commits_len         { d.log_idx += 1; } }
        }
    }

    // ── Diff helpers ─────────────────────────────────────────────────────────

    fn load_diff(&mut self) {
        let panel = &self.dashboard.selected_panel;
        let idx = match panel {
            Panel::Staged    => self.dashboard.staged_idx,
            Panel::Unstaged  => self.dashboard.unstaged_idx,
            Panel::Untracked => self.dashboard.untracked_idx,
            Panel::Log       => { self.load_commit_diff(); return; }
        };

        let files = match panel {
            Panel::Staged    => &self.staged,
            Panel::Unstaged  => &self.unstaged,
            Panel::Untracked => &self.untracked,
            Panel::Log       => unreachable!(),
        };

        if let Some(entry) = files.get(idx) {
            self.diff.title = entry.path.clone();
            self.diff.lines = read_file_diff(&self.repo_path, &entry.path, entry.status == FileStatus::Staged);
            self.diff.scroll = 0;
        }
    }

    fn load_commit_diff_from_log(&mut self) {
        let idx = self.log.idx;
        if let Some(commit) = self.commits.get(idx) {
            self.diff.title = format!("{} {}", commit.hash, commit.message);
            self.diff.lines = read_commit_diff(&self.repo_path, &commit.full_hash);
            self.diff.scroll = 0;
        }
    }

    fn load_commit_diff(&mut self) {
        let idx = self.dashboard.log_idx;
        if let Some(commit) = self.commits.get(idx) {
            self.diff.title = format!("{} {}", commit.hash, commit.message);
            self.diff.lines = read_commit_diff(&self.repo_path, &commit.full_hash);
            self.diff.scroll = 0;
        }
    }

    pub fn diff_scroll_up(&mut self) {
        if self.diff.scroll > 0 { self.diff.scroll -= 1; }
    }

    pub fn diff_scroll_down(&mut self) {
        let max = self.diff.lines.len().saturating_sub(1);
        if self.diff.scroll < max { self.diff.scroll += 1; }
    }

    pub fn diff_page_up(&mut self) {
        self.diff.scroll = self.diff.scroll.saturating_sub(20);
    }

    pub fn diff_page_down(&mut self) {
        let max = self.diff.lines.len().saturating_sub(1);
        self.diff.scroll = (self.diff.scroll + 20).min(max);
    }

    // ── Log helpers ──────────────────────────────────────────────────────────

    pub fn log_move_up(&mut self) {
        if self.log.filtered.is_empty() {
            if self.log.idx > 0 { self.log.idx -= 1; }
        } else {
            let pos = self.log.filtered.iter().position(|&i| i == self.log.idx).unwrap_or(0);
            if pos > 0 { self.log.idx = self.log.filtered[pos - 1]; }
        }
        self.sync_log_scroll();
        self.log_load_commit_files();
    }

    pub fn log_move_down(&mut self) {
        if self.log.filtered.is_empty() {
            if self.log.idx + 1 < self.commits.len() {
                self.log.idx += 1;
            } else {
                self.log_load_more();
            }
        } else {
            let pos = self.log.filtered.iter().position(|&i| i == self.log.idx).unwrap_or(0);
            if pos + 1 < self.log.filtered.len() { self.log.idx = self.log.filtered[pos + 1]; }
        }
        self.sync_log_scroll();
        self.log_load_commit_files();
    }

    pub fn log_load_commit_files(&mut self) {
        if self.log.last_files_idx == Some(self.log.idx) { return; }
        self.log.last_files_idx = Some(self.log.idx);
        self.log.commit_files.clear();
        let Some(commit) = self.commits.get(self.log.idx) else { return };
        let hash = commit.full_hash.clone();
        let Ok(repo) = git2::Repository::discover(&self.repo_path) else { return };
        let Ok(oid) = git2::Oid::from_str(&hash) else { return };
        let Ok(commit) = repo.find_commit(oid) else { return };
        let Ok(tree) = commit.tree() else { return };
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) else { return };
        let _ = diff.foreach(
            &mut |delta, _| {
                let status = match delta.status() {
                    git2::Delta::Added    => 'A',
                    git2::Delta::Deleted  => 'D',
                    git2::Delta::Modified => 'M',
                    git2::Delta::Renamed  => 'R',
                    _                     => 'M',
                };
                let path = delta.new_file().path()
                    .or_else(|| delta.old_file().path())
                    .and_then(|p| p.to_str())
                    .unwrap_or("")
                    .to_string();
                self.log.commit_files.push(CommitFileEntry { path, status });
                true
            },
            None, None, None,
        );
    }

    pub fn log_load_more(&mut self) {
        if !self.log.all_loaded {
            self.log.page_size += 50;
            let _ = self.refresh();
        }
    }

    pub fn log_update_filter(&mut self) {
        let q = self.log.search_query.to_lowercase();
        if q.is_empty() {
            self.log.filtered.clear();
            return;
        }
        self.log.filtered = self.commits.iter().enumerate()
            .filter(|(_, c)| {
                c.message.to_lowercase().contains(&q) ||
                c.author.to_lowercase().contains(&q) ||
                c.hash.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        // Move selection to first match if current isn't in results
        if !self.log.filtered.contains(&self.log.idx) {
            if let Some(&first) = self.log.filtered.first() {
                self.log.idx = first;
                self.sync_log_scroll();
            }
        }
    }

    fn sync_log_scroll(&mut self) {
        let page = 20usize;
        if self.log.idx < self.log.scroll {
            self.log.scroll = self.log.idx;
        } else if self.log.idx >= self.log.scroll + page {
            self.log.scroll = self.log.idx + 1 - page;
        }
    }

    // ── Branch helpers ───────────────────────────────────────────────────────

    fn load_branches(&mut self) {
        let Ok(repo) = Repository::discover(&self.repo_path) else { return };
        let Ok(branches) = repo.branches(None) else { return };

        self.branch_view.branches.clear();
        for branch in branches.flatten() {
            let (b, btype) = branch;
            let Ok(name) = b.name() else { continue };
            let Some(name) = name else { continue };
            let is_current = b.is_head();
            let is_remote = btype == git2::BranchType::Remote;
            self.branch_view.branches.push(BranchEntry {
                name: name.to_string(),
                is_current,
                is_remote,
            });
        }
        self.branch_view.idx = self.branch_view.branches
            .iter().position(|b| b.is_current).unwrap_or(0);

        self.branch_view.current_has_upstream = repo.branches(Some(git2::BranchType::Local))
            .ok()
            .map(|branches| branches.flatten().any(|(b, _)| {
                b.is_head() && b.upstream().is_ok()
            }))
            .unwrap_or(false);
    }

    pub fn branch_move_up(&mut self) {
        if self.branch_view.idx > 0 { self.branch_view.idx -= 1; }
    }

    pub fn branch_move_down(&mut self) {
        if self.branch_view.idx + 1 < self.branch_view.branches.len() {
            self.branch_view.idx += 1;
        }
    }

    // ── Commit helpers ───────────────────────────────────────────────────────

    pub fn commit_type_char(&mut self, c: char) {
        let cur = self.commit_view.cursor;
        self.commit_view.message.insert(cur, c);
        self.commit_view.cursor += 1;
    }

    pub fn commit_backspace(&mut self) {
        let cur = self.commit_view.cursor;
        if cur > 0 {
            self.commit_view.message.remove(cur - 1);
            self.commit_view.cursor -= 1;
        }
    }

    pub fn commit_cursor_left(&mut self) {
        if self.commit_view.cursor > 0 { self.commit_view.cursor -= 1; }
    }

    pub fn commit_cursor_right(&mut self) {
        let len = self.commit_view.message.len();
        if self.commit_view.cursor < len { self.commit_view.cursor += 1; }
    }

    // ── Snapshot helpers ─────────────────────────────────────────────────────

    pub fn load_snapshots(&mut self) {
        // Snapshots stored in .git/torii-snapshots/ — read metadata
        self.snapshot_view.snapshots.clear();
        let snap_dir = std::path::Path::new(&self.repo_path)
            .join(".git/torii-snapshots");
        if !snap_dir.exists() { return; }
        if let Ok(entries) = std::fs::read_dir(&snap_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".meta") {
                    let id = name.trim_end_matches(".meta").to_string();
                    let timestamp = entry.metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64)
                        .unwrap_or(0);
                    let time = if timestamp > 0 { format_age(timestamp) } else { String::new() };
                    let label = std::fs::read_to_string(entry.path())
                        .unwrap_or_else(|_| id.clone())
                        .trim().to_string();
                    self.snapshot_view.snapshots.push(SnapshotEntry {
                        id: id.clone(),
                        name: label,
                        time,
                        timestamp,
                    });
                }
            }
        }
        self.snapshot_view.snapshots.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.snapshot_view.idx = 0;
    }

    pub fn snapshot_move_up(&mut self) {
        if self.snapshot_view.idx > 0 { self.snapshot_view.idx -= 1; }
    }

    pub fn snapshot_move_down(&mut self) {
        let len = if self.snapshot_view.filtered.is_empty() && self.snapshot_view.search_query.is_empty() {
            self.snapshot_view.snapshots.len()
        } else {
            self.snapshot_view.filtered.len()
        };
        if self.snapshot_view.idx + 1 < len { self.snapshot_view.idx += 1; }
    }

    // ── Sync helpers ─────────────────────────────────────────────────────────

    pub fn sync_op_next(&mut self) {
        self.sync_view.selected_op = match self.sync_view.selected_op {
            SyncOp::PullPush  => SyncOp::PullOnly,
            SyncOp::PullOnly  => SyncOp::PushOnly,
            SyncOp::PushOnly  => SyncOp::ForcePush,
            SyncOp::ForcePush => SyncOp::Fetch,
            SyncOp::Fetch     => SyncOp::PullPush,
        };
    }

    pub fn sync_op_prev(&mut self) {
        self.sync_view.selected_op = match self.sync_view.selected_op {
            SyncOp::PullPush  => SyncOp::Fetch,
            SyncOp::PullOnly  => SyncOp::PullPush,
            SyncOp::PushOnly  => SyncOp::PullOnly,
            SyncOp::ForcePush => SyncOp::PushOnly,
            SyncOp::Fetch     => SyncOp::ForcePush,
        };
    }

    // ── Tag helpers ──────────────────────────────────────────────────────────

    fn load_tags(&mut self) {
        self.tag_view.tags.clear();
        let Ok(repo) = Repository::discover(&self.repo_path) else { return };
        let _ = repo.tag_foreach(|oid, name| {
            let name = String::from_utf8_lossy(name).to_string();
            let name = name.trim_start_matches("refs/tags/").to_string();
            let commit = repo.find_object(oid, None).ok()
                .and_then(|obj| obj.peel_to_commit().ok());
            let (message, hash, time, timestamp) = commit.map(|c| (
                c.summary().unwrap_or("").to_string(),
                format!("{:.7}", c.id()),
                format_age(c.time().seconds()),
                c.time().seconds(),
            )).unwrap_or_default();
            self.tag_view.tags.push(TagEntry { name, message, hash, time, timestamp });
            true
        });
        self.tag_view.tags.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.tag_view.idx = self.tag_view.idx.min(self.tag_view.tags.len().saturating_sub(1));
    }

    pub fn tag_move_up(&mut self) {
        if self.tag_view.idx > 0 { self.tag_view.idx -= 1; }
    }

    pub fn tag_move_down(&mut self) {
        if self.tag_view.idx + 1 < self.tag_view.tags.len() { self.tag_view.idx += 1; }
    }

    // ── History helpers ──────────────────────────────────────────────────────

    #[allow(dead_code)]
    fn load_reflog(&mut self) {
        self.history_view.reflog.clear();
        let Ok(repo) = Repository::discover(&self.repo_path) else { return };
        let Ok(reflog) = repo.reflog("HEAD") else { return };
        for entry in reflog.iter() {
            let id = entry.id_new().to_string()[..7].to_string();
            let message = entry.message().unwrap_or("").to_string();
            let time = format_age(entry.committer().when().seconds());
            self.history_view.reflog.push(ReflogEntry { id, message, time });
        }
        self.history_view.idx = 0;
    }

    pub fn history_move_up(&mut self) {
        if self.history_view.idx > 0 { self.history_view.idx -= 1; }
    }

    pub fn history_move_down(&mut self) {
        if self.history_view.idx + 1 < self.history_view.reflog.len() {
            self.history_view.idx += 1;
        }
    }

    // ── Remote helpers ───────────────────────────────────────────────────────

    fn load_remotes(&mut self) {
        self.remote_view.remotes.clear();
        self.remote_view.mirrors.clear();
        // git remotes
        if let Ok(repo) = Repository::discover(&self.repo_path) {
            if let Ok(remotes) = repo.remotes() {
                for name in remotes.iter().flatten() {
                    let url = repo.find_remote(name)
                        .ok()
                        .and_then(|r| r.url().map(|u| u.to_string()))
                        .unwrap_or_default();
                    let platform = detect_platform(&url);
                    let display_name = shorten_remote_name(name, &platform);
                    self.remote_view.remotes.push(RemoteEntry { name: display_name, git_name: name.to_string(), url, platform });
                }
            }
        }
        // torii mirrors
        let mirrors_path = std::path::Path::new(&self.repo_path).join(".torii/mirrors.json");
        if mirrors_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&mirrors_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(arr) = json["mirrors"].as_array() {
                        for m in arr {
                            let name     = m["name"].as_str().unwrap_or("").to_string();
                            let platform = m["platform"].as_str().unwrap_or("").to_string();
                            let url      = m["url"].as_str().unwrap_or("").to_string();
                            let kind     = match m["mirror_type"].as_str().unwrap_or("Replica") {
                                "Primary" | "Master" => "primary",
                                _                   => "replica",
                            }.to_string();
                            let account  = m["account_name"].as_str().unwrap_or("").to_string();
                            let repo     = m["repo_name"].as_str().unwrap_or("").to_string();
                            self.remote_view.mirrors.push(MirrorEntry { name, platform, url, kind, account, repo });
                        }
                    }
                }
            }
        }
        self.remote_view.idx = 0;
    }

    pub fn reload_remotes(&mut self) {
        self.load_remotes();
    }

    pub fn load_prs(&mut self) {
        use crate::pr::{detect_platform_from_remote, get_pr_client};

        self.pr_view.prs.clear();
        self.pr_view.error = None;
        self.pr_view.loading = true;
        self.pr_rx = None;

        let Some((platform, owner, repo_name)) = detect_platform_from_remote(&self.repo_path)
        else {
            self.pr_view.loading = false;
            self.pr_view.error = Some("no github / gitlab / codeberg remote detected".to_string());
            return;
        };
        self.pr_view.platform  = platform.clone();
        self.pr_view.owner     = owner.clone();
        self.pr_view.repo_name = repo_name.clone();

        let state = match self.pr_view.filter {
            PrStateFilter::Open   => "open".to_string(),
            PrStateFilter::Closed => "closed".to_string(),
            PrStateFilter::All    => "all".to_string(),
        };

        let client = match get_pr_client(&platform) {
            Err(e) => {
                self.pr_view.loading = false;
                self.pr_view.error = Some(e.to_string());
                return;
            }
            Ok(c) => c,
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.pr_rx = Some(rx);

        std::thread::spawn(move || {
            let result = client.list(&owner, &repo_name, &state).map(|prs| {
                prs.into_iter().map(|p| PrEntry {
                    number:     p.number,
                    title:      p.title,
                    state:      p.state,
                    head:       p.head,
                    base:       p.base,
                    author:     p.author,
                    url:        p.url,
                    draft:      p.draft,
                    mergeable:  p.mergeable,
                    created_at: p.created_at,
                    body:       p.body,
                }).collect()
            });
            let _ = tx.send(result);
        });
    }

    pub fn load_issues(&mut self) {
        use crate::pr::detect_platform_from_remote;
        use crate::issue::get_issue_client;

        self.issue_view.issues.clear();
        self.issue_view.error = None;
        self.issue_view.loading = true;
        self.issue_rx = None;

        let Some((platform, owner, repo_name)) = detect_platform_from_remote(&self.repo_path)
        else {
            self.issue_view.loading = false;
            self.issue_view.error = Some("no github / gitlab / codeberg remote detected".to_string());
            return;
        };
        self.issue_view.platform  = platform.clone();
        self.issue_view.owner     = owner.clone();
        self.issue_view.repo_name = repo_name.clone();

        let client = match get_issue_client(&platform) {
            Err(e) => {
                self.issue_view.loading = false;
                self.issue_view.error = Some(e.to_string());
                return;
            }
            Ok(c) => c,
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.issue_rx = Some(rx);

        std::thread::spawn(move || {
            let result = client.list(&owner, &repo_name, "open").map(|issues| {
                issues.into_iter().map(|i| IssueEntry {
                    number:     i.number,
                    title:      i.title,
                    state:      i.state,
                    author:     i.author,
                    url:        i.url,
                    labels:     i.labels,
                    comments:   i.comments,
                    created_at: i.created_at,
                    body:       i.body,
                }).collect()
            });
            let _ = tx.send(result);
        });
    }

    pub fn load_pr_platforms(&mut self) {
        use crate::pr::detect_platform_from_remote;
        let Ok(repo) = git2::Repository::discover(&self.repo_path) else { return };
        let Ok(remotes) = repo.remotes() else { return };
        let mut seen = std::collections::HashSet::new();
        self.pr_view.available_platforms = remotes.iter()
            .filter_map(|name| {
                let name = name?;
                let remote = repo.find_remote(name).ok()?;
                let url = remote.url()?.to_string();
                let platform = if url.contains("github.com") { "github" }
                    else if url.contains("gitlab.com") { "gitlab" }
                    else { return None };
                // parse owner/repo from url
                let path = if url.contains('@') {
                    url.splitn(2, ':').nth(1)?
                } else {
                    url.trim_start_matches("https://")
                        .trim_start_matches("http://")
                        .splitn(2, '/').nth(1)?
                };
                let path = path.trim_end_matches(".git");
                let mut parts = path.splitn(2, '/');
                let owner = parts.next()?.to_string();
                let repo_name = parts.next()?.to_string();
                let key = format!("{}/{}/{}", platform, owner, repo_name);
                if !seen.insert(key) { return None; }
                Some(PrPlatformEntry {
                    label: format!("{} — {}/{}", platform, owner, repo_name),
                    platform: platform.to_string(),
                    owner,
                    repo: repo_name,
                })
            })
            .collect();
        // set platform_idx to current active platform
        let current = &self.pr_view.platform;
        let current_owner = &self.pr_view.owner;
        self.pr_view.platform_idx = self.pr_view.available_platforms.iter()
            .position(|p| &p.platform == current && &p.owner == current_owner)
            .unwrap_or(0);
        // also try detect_platform_from_remote as fallback if list empty
        if self.pr_view.available_platforms.is_empty() {
            if let Some((platform, owner, repo_name)) = detect_platform_from_remote(&self.repo_path) {
                self.pr_view.available_platforms.push(PrPlatformEntry {
                    label: format!("{} — {}/{}", platform, owner, repo_name),
                    platform,
                    owner,
                    repo: repo_name,
                });
            }
        }
    }

    pub fn load_pr_branches(&mut self) {
        let Ok(repo) = git2::Repository::discover(&self.repo_path) else { return };
        let Ok(branches) = repo.branches(None) else { return };
        self.pr_view.branches = branches
            .filter_map(|b| b.ok())
            .filter_map(|(b, _)| b.name().ok().flatten().map(|s| s.to_string()))
            .collect();
        self.pr_view.branches.sort();
    }

    pub fn pr_move_up(&mut self) {
        if self.pr_view.idx > 0 { self.pr_view.idx -= 1; }
    }

    pub fn pr_move_down(&mut self) {
        if self.pr_view.idx + 1 < self.pr_view.prs.len() {
            self.pr_view.idx += 1;
        }
    }

    pub fn branch_update_filter(&mut self) {
        let q = self.branch_view.search_query.to_lowercase();
        self.branch_view.filtered = self.branch_view.branches.iter().enumerate()
            .filter(|(_, b)| b.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.branch_view.idx = self.branch_view.filtered.first().copied().unwrap_or(0);
    }

    pub fn tag_update_filter(&mut self) {
        let q = self.tag_view.search_query.to_lowercase();
        self.tag_view.filtered = self.tag_view.tags.iter().enumerate()
            .filter(|(_, t)| t.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.tag_view.idx = self.tag_view.filtered.first().copied().unwrap_or(0);
    }

    pub fn workspace_repo_paths(&self) -> Vec<String> {
        let name = match &self.active_workspace { Some(n) => n, None => return vec![] };
        if let Some(ws) = self.workspace_view.workspaces.iter().find(|ws| &ws.name == name) {
            return ws.repos.iter().map(|r| r.path.clone()).collect();
        }
        vec![]
    }

    pub fn workspace_has_siblings(&self) -> bool {
        self.workspace_repo_paths().len() > 1
    }

    pub fn open_repo_picker(&mut self) {
        let paths = self.workspace_repo_paths();
        if paths.len() <= 1 { return; }
        let current = std::fs::canonicalize(&self.repo_path).ok();
        self.repo_picker_idx = paths.iter().position(|p| {
            std::fs::canonicalize(p).ok() == current
        }).unwrap_or(0);
        self.repo_picker_open = true;
    }

    pub fn remote_move_up(&mut self) {
        if self.remote_view.idx > 0 { self.remote_view.idx -= 1; }
    }

    pub fn remote_move_down(&mut self) {
        if self.remote_view.idx + 1 < self.remote_view.total_len() {
            self.remote_view.idx += 1;
        }
    }

    pub fn mirror_move_up(&mut self) {
        if self.mirror_view.idx > 0 { self.mirror_view.idx -= 1; }
    }

    pub fn mirror_move_down(&mut self) {
        if self.mirror_view.idx + 1 < self.mirror_view.mirrors.len() {
            self.mirror_view.idx += 1;
        }
    }

    // ── Workspace helpers ────────────────────────────────────────────────────

    fn load_workspaces(&mut self) {
        self.workspace_view.workspaces.clear();
        let ws_path = dirs::home_dir()
            .map(|h| h.join(".torii/workspaces.toml"))
            .unwrap_or_default();
        if !ws_path.exists() { return; }
        let Ok(content) = std::fs::read_to_string(&ws_path) else { return };
        let mut current_ws: Option<WorkspaceEntry> = None;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                if let Some(ws) = current_ws.take() {
                    self.workspace_view.workspaces.push(ws);
                }
                let name = line.trim_matches(|c| c == '[' || c == ']').to_string();
                current_ws = Some(WorkspaceEntry { name, repos: vec![] });
            } else if line.starts_with("path") {
                if let Some(ws) = current_ws.as_mut() {
                    let path = line.split('=').nth(1).unwrap_or("").trim().trim_matches('"').to_string();
                    let (branch, ahead, behind, dirty) = repo_quick_status(&path);
                    ws.repos.push(WorkspaceRepo { path, branch, ahead, behind, dirty });
                }
            }
        }
        if let Some(ws) = current_ws.take() {
            self.workspace_view.workspaces.push(ws);
        }
        self.workspace_view.ws_idx = 0;
        self.workspace_view.repo_idx = 0;
    }

    pub fn workspace_move_up(&mut self) {
        match self.workspace_view.focus {
            WorkspaceFocus::Workspaces => {
                if self.workspace_view.ws_idx > 0 { self.workspace_view.ws_idx -= 1; }
                self.workspace_view.repo_idx = 0;
            }
            WorkspaceFocus::Repos => {
                if self.workspace_view.repo_idx > 0 { self.workspace_view.repo_idx -= 1; }
            }
        }
    }

    pub fn workspace_move_down(&mut self) {
        match self.workspace_view.focus {
            WorkspaceFocus::Workspaces => {
                if self.workspace_view.ws_idx + 1 < self.workspace_view.workspaces.len() {
                    self.workspace_view.ws_idx += 1;
                }
                self.workspace_view.repo_idx = 0;
            }
            WorkspaceFocus::Repos => {
                let repo_len = self.workspace_view.workspaces
                    .get(self.workspace_view.ws_idx)
                    .map(|ws| ws.repos.len())
                    .unwrap_or(0);
                if self.workspace_view.repo_idx + 1 < repo_len {
                    self.workspace_view.repo_idx += 1;
                }
            }
        }
    }

    pub fn workspace_focus_repos(&mut self) {
        self.workspace_view.focus = WorkspaceFocus::Repos;
        self.workspace_view.repo_idx = 0;
    }

    pub fn workspace_focus_workspaces(&mut self) {
        self.workspace_view.focus = WorkspaceFocus::Workspaces;
    }

    // ── Config helpers ───────────────────────────────────────────────────────

    fn load_config(&mut self) {
        // All known torii config keys in order. `auth.*` entries were
        // removed from this list in 0.7.5 — credentials live in the
        // dedicated Auth view (sidebar key `a`) since 0.7.2; showing
        // them in two places confused users and required the masking
        // shim below. The Auth view handles its own masking via the
        // `crate::auth` resolver.
        const ALL_KEYS: &[&str] = &[
            "user.name",
            "user.email",
            "user.editor",
            "git.default_branch",
            "git.sign_commits",
            "git.pull_rebase",
            "mirror.default_protocol",
            "mirror.autofetch_enabled",
            "snapshot.auto_enabled",
            "snapshot.auto_interval_minutes",
            "ui.colors",
            "ui.emoji",
            "ui.verbose",
            "ui.date_format",
            "worktree.base_dir",
            "worktree.inherit_paths",
        ];

        // No sensitive keys anymore in this view — tokens live in Auth.
        const SENSITIVE: &[&str] = &[];

        self.config_view.entries.clear();

        // Fetch all current values from torii config list
        let mut values: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut list_args = vec!["config", "list"];
        if self.config_view.scope == ConfigScope::Local { list_args.push("--local"); }
        if let Ok(out) = std::process::Command::new(super::torii_exe())
            .args(&list_args)
            .output()
        {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let line = line.trim();
                if let Some((k, v)) = line.split_once('=') {
                    values.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }

        for &key in ALL_KEYS {
            let section = key.split('.').next().unwrap_or("").to_string();
            let is_sensitive = SENSITIVE.contains(&key);
            let value = match values.get(key) {
                Some(v) if v.is_empty() => "[not set]".to_string(),
                Some(_v) if is_sensitive => "[set]".to_string(),
                Some(v) => v.clone(),
                None => "[not set]".to_string(),
            };
            self.config_view.entries.push(ConfigEntry {
                key: key.to_string(),
                value,
                scope: self.config_view.scope.clone(),
                section,
            });
        }
        self.config_view.idx = 0;
    }

    pub fn config_move_up(&mut self) {
        if self.config_view.idx > 0 { self.config_view.idx -= 1; }
    }

    pub fn config_move_down(&mut self) {
        if self.config_view.idx + 1 < self.config_view.entries.len() {
            self.config_view.idx += 1;
        }
    }

    pub fn config_start_edit(&mut self) {
        if let Some(entry) = self.config_view.entries.get(self.config_view.idx) {
            let initial = if entry.value == "[not set]" || entry.value == "[set]" {
                String::new()
            } else {
                entry.value.clone()
            };
            self.config_view.edit_buf = initial.clone();
            self.config_view.edit_cursor = initial.chars().count();
            self.config_view.editing = true;
        }
    }

    fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
        s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
    }

    pub fn config_type_char(&mut self, c: char) {
        let byte_idx = Self::char_to_byte_idx(&self.config_view.edit_buf, self.config_view.edit_cursor);
        self.config_view.edit_buf.insert(byte_idx, c);
        self.config_view.edit_cursor += 1;
    }

    pub fn config_backspace(&mut self) {
        let cur = self.config_view.edit_cursor;
        if cur > 0 {
            let byte_idx = Self::char_to_byte_idx(&self.config_view.edit_buf, cur - 1);
            self.config_view.edit_buf.remove(byte_idx);
            self.config_view.edit_cursor -= 1;
        }
    }

    pub fn config_cursor_left(&mut self) {
        if self.config_view.edit_cursor > 0 { self.config_view.edit_cursor -= 1; }
    }

    pub fn config_cursor_right(&mut self) {
        let len = self.config_view.edit_buf.chars().count();
        if self.config_view.edit_cursor < len { self.config_view.edit_cursor += 1; }
    }

    // ── Settings helpers ─────────────────────────────────────────────────────

    #[allow(dead_code)]
    pub fn settings_move_up(&mut self) {
        if self.settings_view.idx > 0 { self.settings_view.idx -= 1; }
    }

    #[allow(dead_code)]
    pub fn settings_move_down(&mut self) {
        if self.settings_view.idx < 19 { self.settings_view.idx += 1; }
    }

    // ── Platform view (0.7.12) ───────────────────────────────────────────────
    //
    // load_platform_enter is called from `go_to(View::Platform)`. It
    // discovers remotes, picks one if the current selection is invalid,
    // and triggers the loader for the active sub-tab. Each loader runs
    // on its own thread and writes back through a per-channel receiver.

    pub fn load_platform_enter(&mut self) {
        self.platform_view.remotes = discover_remotes(&self.repo_path);
        // If `remote` isn't in the discovered list, fall back to the
        // first remote that points to a supported platform.
        if !self.platform_view.remotes.contains(&self.platform_view.remote) {
            let pick = self.platform_view.remotes.first().cloned()
                .unwrap_or_else(|| "origin".to_string());
            self.platform_view.remote = pick;
        }
        self.load_platform_active_sub_tab();
    }

    pub fn load_platform_active_sub_tab(&mut self) {
        match self.platform_view.sub_tab {
            PlatformSubTab::Pipelines => self.load_platform_pipelines(),
            PlatformSubTab::Jobs      => {
                if let Some(pid) = self.platform_view.active_pipeline_id {
                    self.load_platform_jobs_for_pipeline(pid.to_string());
                } else {
                    // No drill-down context: fall back to Pipelines.
                    self.platform_view.sub_tab = PlatformSubTab::Pipelines;
                    self.load_platform_pipelines();
                }
            }
            PlatformSubTab::Releases  => self.load_platform_releases(),
            PlatformSubTab::Packages  => self.load_platform_packages(),
            PlatformSubTab::Runners   => self.load_platform_runners(),
        }
    }

    pub fn load_platform_runners(&mut self) {
        self.platform_view.runners.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_runners_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::runner::get_runner_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_runners_rx = Some(rx);

        std::thread::spawn(move || {
            let _ = tx.send(client.list(&owner, &repo));
        });
    }

    // ── 0.7.25 runner actions (pause/resume/remove/reset-token) ─────────────

    fn spawn_runner_action<F>(&mut self, op: F)
    where
        F: FnOnce(&str, &str, Box<dyn crate::runner::RunnerClient>)
            -> std::result::Result<String, String>
            + Send + 'static,
    {
        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.set_status("✗ no remote/platform resolved");
            return;
        };
        let client = match crate::runner::get_runner_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("✗ {}", e));
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_action_rx = Some(rx);
        self.platform_view.action_in_flight = true;
        std::thread::spawn(move || {
            let _ = tx.send(op(&owner, &repo, client));
        });
    }

    pub fn action_runner_pause(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_runner_action(move |owner, repo, client| {
            client.pause(owner, repo, &id)
                .map(|_| format!("✓ runner #{} paused", id_disp))
                .map_err(|e| format!("✗ pause runner #{}: {}", id_disp, e))
        });
    }

    pub fn action_runner_resume(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_runner_action(move |owner, repo, client| {
            client.resume(owner, repo, &id)
                .map(|_| format!("✓ runner #{} resumed", id_disp))
                .map_err(|e| format!("✗ resume runner #{}: {}", id_disp, e))
        });
    }

    pub fn action_runner_remove(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_runner_action(move |owner, repo, client| {
            client.remove(owner, repo, &id)
                .map(|_| format!("✓ runner #{} removed", id_disp))
                .map_err(|e| format!("✗ remove runner #{}: {}", id_disp, e))
        });
    }

    pub fn action_runner_reset_token(&mut self, id: String) {
        // Reset-token is special: it returns a credential we have to
        // surface back to the user. The action_msg lane is a one-line
        // status bar — wrong place for a long secret. Instead we
        // route the new value through the event log (toggle with `e`).
        // The action_msg just announces success and points there.
        let id_disp = id.clone();
        self.spawn_runner_action(move |owner, repo, client| {
            client.reset_token(owner, repo, &id)
                .map(|new_token| {
                    // Embed the token in the success message itself
                    // with a recognisable prefix; the main loop's
                    // post-action hook pulls it back out and pushes
                    // it to the event log.
                    format!("✓ runner #{} token reset|token={}", id_disp, new_token)
                })
                .map_err(|e| format!("✗ reset runner #{} token: {}", id_disp, e))
        });
    }

    fn resolve_platform_target(&mut self) -> Option<(String, String, String)> {
        let res = crate::pr::detect_platform_from_remote_named(
            &self.repo_path,
            &self.platform_view.remote,
        );
        match res {
            Some((p, o, r)) => {
                self.platform_view.platform  = p.clone();
                self.platform_view.owner     = o.clone();
                self.platform_view.repo_name = r.clone();
                Some((p, o, r))
            }
            None => {
                self.platform_view.error = Some(format!(
                    "remote '{}' is not a github/gitlab URL",
                    self.platform_view.remote,
                ));
                None
            }
        }
    }

    pub fn load_platform_pipelines(&mut self) {
        self.platform_view.pipelines.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_pipelines_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::pipeline::get_pipeline_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_pipelines_rx = Some(rx);

        let status = self.platform_view.filter_status.clone();
        let branch_filter = if self.platform_view.filter_branch_only {
            Some(self.branch.clone())
        } else {
            None
        };

        std::thread::spawn(move || {
            let filters = crate::pipeline::ListFilters { status, per_page: 50 };
            let result = client.list(&owner, &repo, &filters);
            // Branch filter is client-side because not every platform
            // exposes a ref filter cleanly (Bitbucket, sourcehut).
            let result = match (result, branch_filter) {
                (Ok(mut items), Some(b)) => {
                    items.retain(|p| p.branch == b);
                    Ok(items)
                }
                (other, _) => other,
            };
            let _ = tx.send(result);
        });
    }

    pub fn load_platform_jobs_for_pipeline(&mut self, pipeline_id: String) {
        self.platform_view.jobs.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_jobs_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::pipeline::get_pipeline_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_jobs_rx = Some(rx);

        std::thread::spawn(move || {
            let _ = tx.send(client.list_jobs(&owner, &repo, &pipeline_id, None));
        });
    }

    pub fn load_platform_releases(&mut self) {
        self.platform_view.releases.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_releases_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::release::get_release_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_releases_rx = Some(rx);

        std::thread::spawn(move || {
            let _ = tx.send(client.list(&owner, &repo, 50));
        });
    }

    pub fn load_platform_packages(&mut self) {
        self.platform_view.packages.clear();
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_packages_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        // Packages are GitLab-only in 0.7.12 (GitHub Packages API requires
        // package_type+username scoping that doesn't map cleanly to the
        // CI surface — the CLI returns an error pointing at `release` for
        // GitHub, we mirror that here so the view doesn't appear broken).
        if platform != "gitlab" {
            self.platform_view.loading = false;
            self.platform_view.error = Some(
                "Packages are GitLab-only here. For GitHub, see Releases (assets).".to_string()
            );
            return;
        }

        let client = match crate::package::get_package_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_packages_rx = Some(rx);

        std::thread::spawn(move || {
            let filters = crate::package::PackageListFilters::default();
            let _ = tx.send(client.list(&owner, &repo, &filters));
        });
    }

    // ── 0.7.24: contextual actions on pipelines/jobs ────────────────────────
    //
    // All four spawn a background thread that calls the platform API and
    // pipes a `Result<message, error>` back through `platform_action_rx`.
    // The main loop pumps the receiver into `platform_view.action_msg` and
    // triggers a list reload so the new status shows up. `action_in_flight`
    // gates the keybinds in events.rs so the user can't fire 5 retries by
    // mashing the key.
    fn spawn_platform_action<F>(&mut self, op: F)
    where
        F: FnOnce(&str, &str, Box<dyn crate::pipeline::PipelineClient>)
            -> std::result::Result<String, String>
            + Send + 'static,
    {
        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.set_status("✗ no remote/platform resolved");
            return;
        };
        let client = match crate::pipeline::get_pipeline_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("✗ {}", e));
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_action_rx = Some(rx);
        self.platform_view.action_in_flight = true;
        std::thread::spawn(move || {
            let _ = tx.send(op(&owner, &repo, client));
        });
    }

    pub fn action_pipeline_cancel(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_platform_action(move |owner, repo, client| {
            client.cancel(owner, repo, &id)
                .map(|_| format!("✓ pipeline #{} canceled", id_disp))
                .map_err(|e| format!("✗ cancel pipeline #{}: {}", id_disp, e))
        });
    }

    pub fn action_pipeline_retry(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_platform_action(move |owner, repo, client| {
            client.retry(owner, repo, &id)
                .map(|_| format!("✓ pipeline #{} retried", id_disp))
                .map_err(|e| format!("✗ retry pipeline #{}: {}", id_disp, e))
        });
    }

    pub fn action_job_cancel(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_platform_action(move |owner, repo, client| {
            client.job_cancel(owner, repo, &id)
                .map(|_| format!("✓ job #{} canceled", id_disp))
                .map_err(|e| format!("✗ cancel job #{}: {}", id_disp, e))
        });
    }

    pub fn action_job_retry(&mut self, id: String) {
        let id_disp = id.clone();
        self.spawn_platform_action(move |owner, repo, client| {
            client.job_retry(owner, repo, &id)
                .map(|_| format!("✓ job #{} retried", id_disp))
                .map_err(|e| format!("✗ retry job #{}: {}", id_disp, e))
        });
    }

    pub fn action_job_artifacts(&mut self, id: String) {
        let id_disp = id.clone();
        // Write to <repo_path>/artifacts/job-<id>.zip — same convention as
        // the CLI's `torii job artifacts <id>` (matches user muscle memory
        // if they've used the non-TUI command before).
        let out_dir = std::path::PathBuf::from(&self.repo_path).join("artifacts");
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            self.set_status(format!("✗ mkdir {}: {}", out_dir.display(), e));
            return;
        }
        let out_path = out_dir.join(format!("job-{}.zip", id));
        let out_disp = out_path.display().to_string();
        self.spawn_platform_action(move |owner, repo, client| {
            client.job_artifacts_download(owner, repo, &id, &out_path)
                .map(|_| format!("✓ job #{} artifacts → {}", id_disp, out_disp))
                .map_err(|e| format!("✗ artifacts job #{}: {}", id_disp, e))
        });
    }

    pub fn load_platform_job_log(&mut self, job_id: String) {
        self.platform_view.job_log = None;
        self.platform_view.job_log_scroll = 0;
        self.platform_view.error = None;
        self.platform_view.loading = true;
        self.platform_job_log_rx = None;

        let Some((platform, owner, repo)) = self.resolve_platform_target() else {
            self.platform_view.loading = false;
            return;
        };

        let client = match crate::pipeline::get_pipeline_client(&platform) {
            Ok(c) => c,
            Err(e) => {
                self.platform_view.loading = false;
                self.platform_view.error = Some(e.to_string());
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.platform_job_log_rx = Some(rx);

        std::thread::spawn(move || {
            let _ = tx.send(client.job_log(&owner, &repo, &job_id));
        });
    }
}

/// List all remote names declared in the repo at `repo_path`. Empty if
/// the repo isn't discoverable. Order is whatever libgit2 returns.
fn discover_remotes(repo_path: &str) -> Vec<String> {
    let Ok(repo) = git2::Repository::discover(repo_path) else { return vec![] };
    let Ok(names) = repo.remotes() else { return vec![] };
    names.iter().flatten().map(|s| s.to_string()).collect()
}

// ── Git helpers ───────────────────────────────────────────────────────────────

fn ahead_behind(repo: &Repository, branch: &str) -> Option<(usize, usize)> {
    let local  = repo.find_reference(&format!("refs/heads/{}", branch)).ok()?.target()?;
    let remote = repo.find_reference(&format!("refs/remotes/origin/{}", branch)).ok()?.target()?;
    repo.graph_ahead_behind(local, remote).ok()
}

fn read_file_diff(repo_path: &str, file_path: &str, staged: bool) -> Vec<DiffLine> {
    let Ok(repo) = Repository::discover(repo_path) else { return vec![] };
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file_path);

    let diff = if staged {
        let head = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let tree = head.as_ref().and_then(|c| c.tree().ok());
        let index = repo.index().ok();
        match (tree, index) {
            (Some(t), Some(mut i)) => repo.diff_tree_to_index(Some(&t), Some(&mut i), Some(&mut opts)),
            (None, Some(mut i))    => repo.diff_tree_to_index(None, Some(&mut i), Some(&mut opts)),
            _ => return vec![],
        }
    } else {
        repo.diff_index_to_workdir(None, Some(&mut opts))
    };

    let Ok(diff) = diff else { return vec![] };
    diff_to_lines(&diff)
}

fn read_commit_diff(repo_path: &str, hash: &str) -> Vec<DiffLine> {
    let Ok(repo) = Repository::discover(repo_path) else { return vec![] };
    let Ok(oid) = git2::Oid::from_str(hash) else { return vec![] };
    let Ok(commit) = repo.find_commit(oid) else { return vec![] };
    let Ok(tree) = commit.tree() else { return vec![] };
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) else { return vec![] };
    diff_to_lines(&diff)
}

fn diff_to_lines(diff: &git2::Diff) -> Vec<DiffLine> {
    let mut lines = vec![];
    let _ = diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let content = String::from_utf8_lossy(line.content()).trim_end_matches('\n').to_string();
        let (kind, line_no) = match line.origin() {
            '+' => (DiffLineKind::Added,   line.new_lineno()),
            '-' => (DiffLineKind::Removed, line.old_lineno()),
            'F' => (DiffLineKind::Header,  None),
            'H' => (DiffLineKind::HunkHeader, line.new_lineno()),
            _   => (DiffLineKind::Context, line.new_lineno()),
        };
        lines.push(DiffLine { kind, content, line_no });
        true
    });
    lines
}

fn format_age(ts: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - ts;
    if diff < 60        { format!("{}s ago", diff) }
    else if diff < 3600 { format!("{}m ago", diff / 60) }
    else if diff < 86400 { format!("{}h ago", diff / 3600) }
    else                { format!("{}d ago", diff / 86400) }
}

fn shorten_remote_name(name: &str, platform: &str) -> String {
    match platform {
        "GitHub" if name.starts_with("github") => "gh".to_string(),
        "GitLab" if name.starts_with("gitlab") => "gl".to_string(),
        _ => name.to_string(),
    }
}

fn detect_platform(url: &str) -> String {
    if url.contains("github.com")    { "GitHub".into() }
    else if url.contains("gitlab.com") { "GitLab".into() }
    else if url.contains("bitbucket.org") { "Bitbucket".into() }
    else if url.contains("codeberg.org")  { "Codeberg".into() }
    else { "git".into() }
}

fn repo_quick_status(path: &str) -> (String, usize, usize, bool) {
    let Ok(repo) = Repository::discover(path) else { return ("?".into(), 0, 0, false) };
    let branch = repo.head().ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "detached".to_string());
    let (ahead, behind) = ahead_behind(&repo, &branch).unwrap_or((0, 0));
    let dirty = repo.statuses(None)
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    (branch, ahead, behind, dirty)
}
