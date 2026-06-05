//! Worktree view state.

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

    /// 0.7.39 — interactive ops state. Same shape as the Auth /
    /// Bisect views: `focus` says what overlay is up; `dropdown_idx`
    /// is the selection inside the ops menu; `input_buffer` collects
    /// free-form text for the ops that need it (Add branch, Lock
    /// reason, Move new path); `pending_op` says what to do with
    /// the buffer when the user hits Enter.
    pub focus: WorktreeFocus,
    pub dropdown_idx: usize,
    pub input_buffer: String,
    pub input_prompt: String,
    pub pending_op: WorktreePendingOp,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum WorktreeFocus {
    #[default]
    List,
    OpsDropdown,
    InputArgs,
    ConfirmRemove,
    ConfirmPrune,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum WorktreePendingOp {
    #[default]
    None,
    AddBranch,
    LockReason,
    MoveNewPath,
}

impl Default for WorktreeState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            idx: 0,
            status: None,
            focus: WorktreeFocus::List,
            dropdown_idx: 0,
            input_buffer: String::new(),
            input_prompt: String::new(),
            pending_op: WorktreePendingOp::None,
        }
    }
}

// -- Submodule view --------------------------------------------------------
