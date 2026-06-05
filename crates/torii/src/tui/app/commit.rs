//! Commit (save) view state + ops.

use super::*;

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
        Self {
            message: String::new(),
            cursor: 0,
            focus: CommitFocus::List,
            type_idx: 0,
            amend: false,
        }
    }
}

// ── Snapshot state ───────────────────────────────────────────────────────────

impl App {
    // The cursor is a CHAR index; `String::insert`/`remove` take BYTE
    // offsets. Convert through `char_to_byte_idx` (same scheme as the
    // config editor) so multibyte input (ñ, á, emoji) can't land on a
    // non-boundary and panic.
    pub fn commit_type_char(&mut self, c: char) {
        let byte = Self::char_to_byte_idx(&self.commit_view.message, self.commit_view.cursor);
        self.commit_view.message.insert(byte, c);
        self.commit_view.cursor += 1;
    }

    pub fn commit_backspace(&mut self) {
        let cur = self.commit_view.cursor;
        if cur > 0 {
            let byte = Self::char_to_byte_idx(&self.commit_view.message, cur - 1);
            self.commit_view.message.remove(byte);
            self.commit_view.cursor -= 1;
        }
    }

    pub fn commit_cursor_left(&mut self) {
        if self.commit_view.cursor > 0 {
            self.commit_view.cursor -= 1;
        }
    }

    pub fn commit_cursor_right(&mut self) {
        let len = self.commit_view.message.chars().count();
        if self.commit_view.cursor < len {
            self.commit_view.cursor += 1;
        }
    }

    // ── Snapshot helpers ─────────────────────────────────────────────────────
}
