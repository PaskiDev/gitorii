//! Shared row/line types used across views.

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
    pub hash: String,      // short (7 chars) for display
    pub full_hash: String, // full 40-char hash for git ops
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
