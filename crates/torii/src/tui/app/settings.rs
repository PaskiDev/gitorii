//! TUI settings (appearance) state.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum BorderStyle {
    Rounded,
    Sharp,
}

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
        if !path.exists() {
            return Self::default();
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let mut s = Self::default();
        for line in content.lines() {
            let line = line.trim();
            let mut parts = line.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim();
            let val = parts.next().unwrap_or("").trim().trim_matches('"');
            match key {
                "border_style" => {
                    s.border_style = if val == "sharp" {
                        BorderStyle::Sharp
                    } else {
                        BorderStyle::Rounded
                    }
                }
                "show_help_view" => s.show_help_view = val != "false",
                "show_history_view" => s.show_history_view = val != "false",
                "show_mirror_view" => s.show_mirror_view = val != "false",
                "show_workspace_view" => s.show_workspace_view = val != "false",
                "show_remote_view" => s.show_remote_view = val != "false",
                "brand_color" => {
                    if let Some(rgb) = parse_rgb(val) {
                        s.brand_color = rgb;
                    }
                }
                "selected_bg" => {
                    if let Some(rgb) = parse_rgb(val) {
                        s.selected_bg = rgb;
                    }
                }
                "event_log_max" => {
                    if let Ok(n) = val.parse::<usize>() {
                        s.event_log_max = n;
                    }
                }
                "graph_style" => {
                    s.graph_style = crate::graph::GraphStyle::from_str(val);
                }
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
    if parts.len() != 3 {
        return None;
    }
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
    fn default() -> Self {
        Self {
            idx: 0,
            status: None,
        }
    }
}

// -- Worktree view ---------------------------------------------------------

impl App {
    #[allow(dead_code)]
    pub fn settings_move_up(&mut self) {
        if self.settings_view.idx > 0 {
            self.settings_view.idx -= 1;
        }
    }

    #[allow(dead_code)]
    pub fn settings_move_down(&mut self) {
        if self.settings_view.idx < 19 {
            self.settings_view.idx += 1;
        }
    }

    // ── Platform view (0.7.12) ───────────────────────────────────────────────
    //
    // load_platform_enter is called from `go_to(View::Platform)`. It
    // discovers remotes, picks one if the current selection is invalid,
    // and triggers the loader for the active sub-tab. Each loader runs
    // on its own thread and writes back through a per-channel receiver.
}
