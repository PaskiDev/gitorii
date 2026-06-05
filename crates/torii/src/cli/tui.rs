//! `torii tui` — launch the dashboard (with repo picker fallback).

use anyhow::Result;

pub(crate) fn run() -> Result<()> {
    let current = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if git2::Repository::discover(&current).is_ok() {
        // Estamos dentro de un repo — abre directamente
        crate::tui::run()?;
    } else {
        // No hay repo — lanza el picker
        use crate::tui::picker::{run_picker, save_workspace, PickerResult};
        match run_picker(&current)? {
            PickerResult::Cancelled => {}
            PickerResult::SingleRepo(path) => {
                std::env::set_current_dir(&path)?;
                crate::tui::run()?;
            }
            PickerResult::Workspace { name, repos } => {
                save_workspace(&name, &repos)?;
                if let Some(first) = repos.first() {
                    std::env::set_current_dir(first)?;
                }
                crate::tui::run_with_workspace(name)?;
            }
            PickerResult::OpenWorkspace(name) => {
                let ws_path = crate::tui::app::workspaces_toml_path().unwrap_or_default();
                if let Ok(content) = std::fs::read_to_string(&ws_path) {
                    let mut in_ws = false;
                    let mut first_path: Option<std::path::PathBuf> = None;
                    for line in content.lines() {
                        let line = line.trim();
                        if line == format!("[{}]", name) {
                            in_ws = true;
                            continue;
                        }
                        if line.starts_with('[') {
                            in_ws = false;
                        }
                        if in_ws && line.starts_with("path") {
                            let p = line
                                .split('=')
                                .nth(1)
                                .unwrap_or("")
                                .trim()
                                .trim_matches('"');
                            first_path = Some(std::path::PathBuf::from(p));
                            break;
                        }
                    }
                    if let Some(p) = first_path {
                        std::env::set_current_dir(&p)?;
                    }
                }
                crate::tui::run_with_workspace(name)?;
            }
        }
    }
    Ok(())
}
