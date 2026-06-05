//! Config view state + inline editor ops.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigScope {
    Global,
    Local,
}

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

impl App {
    pub(crate) fn load_config(&mut self) {
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
            "git.gpg_key",
            "git.gpg_program",
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
        let mut values: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut list_args = vec!["config", "list"];
        if self.config_view.scope == ConfigScope::Local {
            list_args.push("--local");
        }
        if let Ok(out) = std::process::Command::new(crate::tui::torii_exe())
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
        if self.config_view.idx > 0 {
            self.config_view.idx -= 1;
        }
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

    pub fn config_type_char(&mut self, c: char) {
        let byte_idx =
            Self::char_to_byte_idx(&self.config_view.edit_buf, self.config_view.edit_cursor);
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
        if self.config_view.edit_cursor > 0 {
            self.config_view.edit_cursor -= 1;
        }
    }

    pub fn config_cursor_right(&mut self) {
        let len = self.config_view.edit_buf.chars().count();
        if self.config_view.edit_cursor < len {
            self.config_view.edit_cursor += 1;
        }
    }

    // ── Settings helpers ─────────────────────────────────────────────────────
}
