//! Submodule view state.

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

    /// 0.7.39 — same interactive ops scaffold as the Worktree view.
    pub focus: SubmoduleFocus,
    pub dropdown_idx: usize,
    pub input_buffer: String,
    pub input_prompt: String,
    pub pending_op: SubmodulePendingOp,
    /// Two-step Add: first URL, then path. `pending_url` stashes the
    /// URL between the two prompts.
    pub pending_url: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SubmoduleFocus {
    #[default]
    List,
    OpsDropdown,
    InputArgs,
    ConfirmRemove,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SubmodulePendingOp {
    #[default]
    None,
    /// Two-step Add: URL first, then path.
    AddUrl,
    AddPath,
    Foreach,
}

impl Default for SubmoduleState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            idx: 0,
            status: None,
            focus: SubmoduleFocus::List,
            dropdown_idx: 0,
            input_buffer: String::new(),
            input_prompt: String::new(),
            pending_op: SubmodulePendingOp::None,
            pending_url: String::new(),
        }
    }
}

// -- Bisect view -----------------------------------------------------------
