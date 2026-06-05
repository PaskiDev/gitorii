//! Bisect view + ref-picker state.

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

    /// 0.7.33 — interactive ops state. Same shape as the Auth view:
    /// `focus` says which overlay (if any) is up; `dropdown_idx` is the
    /// selection inside the ops menu; `input_buffer` collects free-form
    /// text (Start args, Run command); `pending_op` says what to do
    /// with that buffer when the user hits Enter.
    pub focus: BisectFocus,
    pub dropdown_idx: usize,
    pub input_buffer: String,
    pub input_prompt: String,
    pub pending_op: BisectPendingOp,

    /// 0.7.34 — populated while `focus == RefPicker`.
    pub picker: RefPickerState,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum BisectFocus {
    #[default]
    List,
    OpsDropdown,
    InputArgs,
    /// 0.7.34 — searchable ref picker for Start / Mark / Skip. Shows
    /// HEAD, local + remote branches, tags, and the recent log.
    RefPicker,
    ConfirmReset,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum BisectPendingOp {
    #[default]
    None,
    /// Run command — the only op that still uses the freeform input
    /// overlay (it's a command line, not a ref). Start moved to the
    /// ref picker in 0.7.34.
    Run,
}

/// 0.7.34 — which bisect operation is currently using the ref picker.
/// Start drives the two-tab dance (Bad → Good); Mark/Skip just pick
/// one ref and execute.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum RefPickerOp {
    #[default]
    Start,
    MarkGood,
    MarkBad,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum RefPickerTab {
    #[default]
    Bad,
    Good,
}

#[derive(Debug, Clone)]
pub enum RefKind {
    Head,
    Branch,
    Tag,
    Remote,
    Commit,
}

#[derive(Debug, Clone)]
pub struct RefEntry {
    /// What the user reads in the list.
    pub display: String,
    /// What we hand to `crate::bisect::*` (branch / tag name or full
    /// OID; HEAD becomes the literal string "HEAD").
    pub target: String,
    pub kind: RefKind,
    /// Optional subject line for log entries — kept so a future
    /// detail panel can show the commit subject for the highlighted
    /// row, but not rendered today.
    #[allow(dead_code)]
    pub subject: Option<String>,
}

#[derive(Default, Clone)]
pub struct RefPickerState {
    pub op: RefPickerOp,
    pub tab: RefPickerTab,
    /// Full list as loaded from the repo. Filtering happens at render
    /// time so backspacing the filter doesn't need to refetch.
    pub all: Vec<RefEntry>,
    /// Index inside the *filtered* slice.
    pub idx: usize,
    pub filter: String,
    pub bad_pick: Option<RefEntry>,
    pub good_picks: Vec<RefEntry>,
}

// -- Auth view -------------------------------------------------------------
