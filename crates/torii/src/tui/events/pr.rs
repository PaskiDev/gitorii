//! PR/MR view key handling.

use super::{handle_global_nav, Action};
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

pub(super) fn handle_pr(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::PrConfirm;

    // Create flow — multi-step text input
    if matches!(
        app.pr_view.confirm,
        PrConfirm::CreateTitle | PrConfirm::CreateDesc
    ) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.create_input.clear();
                app.pr_view.create_desc.clear();
            }
            (_, KeyCode::Backspace) => {
                if app.pr_view.create_input.is_empty()
                    && app.pr_view.confirm == PrConfirm::CreateDesc
                {
                    // remove last char from accumulated desc
                    app.pr_view.create_desc.pop();
                } else {
                    app.pr_view.create_input.pop();
                }
            }
            (_, KeyCode::Enter) => {
                match app.pr_view.confirm.clone() {
                    PrConfirm::CreateTitle => {
                        app.pr_view.create_title = app.pr_view.create_input.trim().to_string();
                        app.pr_view.create_input.clear();
                        // load branches for head dropdown
                        app.load_pr_branches();
                        // pre-select current branch as head
                        let current = git2::Repository::discover(&app.repo_path)
                            .ok()
                            .and_then(|r| {
                                r.head()
                                    .ok()
                                    .and_then(|h| h.shorthand().map(|s| s.to_string()))
                            })
                            .unwrap_or_default();
                        app.pr_view.create_head = current.clone();
                        app.pr_view.branch_idx = app
                            .pr_view
                            .branches
                            .iter()
                            .position(|b| *b == current)
                            .unwrap_or(0);
                        app.pr_view.confirm = PrConfirm::CreateHead;
                    }
                    PrConfirm::CreateBase => {}
                    PrConfirm::CreateDesc => {
                        // Enter adds a newline to description
                        if !app.pr_view.create_input.is_empty() {
                            if !app.pr_view.create_desc.is_empty() {
                                app.pr_view.create_desc.push('\n');
                            }
                            app.pr_view.create_desc.push_str(&app.pr_view.create_input);
                            app.pr_view.create_input.clear();
                        } else {
                            app.pr_view.create_desc.push('\n');
                        }
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Tab) if app.pr_view.confirm == PrConfirm::CreateDesc => {
                app.pr_view.create_draft = !app.pr_view.create_draft;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('s'))
                if app.pr_view.confirm == PrConfirm::CreateDesc =>
            {
                // Ctrl+S submits — flush current line first
                if !app.pr_view.create_input.is_empty() {
                    if !app.pr_view.create_desc.is_empty() {
                        app.pr_view.create_desc.push('\n');
                    }
                    app.pr_view.create_desc.push_str(&app.pr_view.create_input);
                    app.pr_view.create_input.clear();
                }
                // advance to platform selection
                app.load_pr_platforms();
                // pre-select current platform
                let n = app.pr_view.available_platforms.len();
                app.pr_view.create_platform_selected = vec![false; n];
                if n > 0 {
                    let cur_platform = app.pr_view.platform.clone();
                    let cur_owner = app.pr_view.owner.clone();
                    let idx = app
                        .pr_view
                        .available_platforms
                        .iter()
                        .position(|p| p.platform == cur_platform && p.owner == cur_owner)
                        .unwrap_or(0);
                    if let Some(s) = app.pr_view.create_platform_selected.get_mut(idx) {
                        *s = true;
                    }
                }
                app.pr_view.create_platform_idx = 0;
                app.pr_view.confirm = PrConfirm::CreatePlatforms;
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                // enforce title limit
                if app.pr_view.confirm == PrConfirm::CreateTitle
                    && app.pr_view.create_input.chars().count() >= 255
                {
                    return None;
                }
                app.pr_view.create_input.push(c);
                // auto-wrap at 56 chars (overlay inner width)
                if app.pr_view.create_input.chars().count() >= 56 {
                    if !app.pr_view.create_desc.is_empty() {
                        app.pr_view.create_desc.push('\n');
                    }
                    app.pr_view.create_desc.push_str(&app.pr_view.create_input);
                    app.pr_view.create_input.clear();
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Create head branch — dropdown
    if app.pr_view.confirm == PrConfirm::CreateHead {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.create_input.clear();
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.pr_view.branch_idx > 0 => {
                app.pr_view.branch_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.pr_view.branch_idx + 1 < app.pr_view.branches.len() =>
            {
                app.pr_view.branch_idx += 1;
            }
            (_, KeyCode::Enter) => {
                if let Some(branch) = app.pr_view.branches.get(app.pr_view.branch_idx) {
                    app.pr_view.create_head = branch.clone();
                }
                // load branches again for base dropdown, pre-select main/master
                app.load_pr_branches();
                let base = app.pr_view.create_base.clone();
                app.pr_view.branch_idx = app
                    .pr_view
                    .branches
                    .iter()
                    .position(|b| b == &base)
                    .or_else(|| app.pr_view.branches.iter().position(|b| b == "main"))
                    .or_else(|| app.pr_view.branches.iter().position(|b| b == "master"))
                    .unwrap_or(0);
                app.pr_view.confirm = PrConfirm::CreateBase;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Create base branch — dropdown
    if app.pr_view.confirm == PrConfirm::CreateBase {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.create_input.clear();
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.pr_view.branch_idx > 0 => {
                app.pr_view.branch_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.pr_view.branch_idx + 1 < app.pr_view.branches.len() =>
            {
                app.pr_view.branch_idx += 1;
            }
            (_, KeyCode::Enter) => {
                if let Some(branch) = app.pr_view.branches.get(app.pr_view.branch_idx) {
                    app.pr_view.create_base = branch.clone();
                }
                app.pr_view.create_input.clear();
                app.pr_view.confirm = PrConfirm::CreateDesc;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Create — platform multi-select
    if app.pr_view.confirm == PrConfirm::CreatePlatforms {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.pr_view.create_platform_idx > 0 => {
                app.pr_view.create_platform_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                let n = app.pr_view.available_platforms.len();
                if app.pr_view.create_platform_idx + 1 < n {
                    app.pr_view.create_platform_idx += 1;
                }
            }
            (_, KeyCode::Char(' ')) => {
                let idx = app.pr_view.create_platform_idx;
                if let Some(s) = app.pr_view.create_platform_selected.get_mut(idx) {
                    *s = !*s;
                }
            }
            (_, KeyCode::Char('a')) if key.modifiers == KeyModifiers::NONE => {
                let all = app.pr_view.create_platform_selected.iter().all(|&s| s);
                app.pr_view
                    .create_platform_selected
                    .iter_mut()
                    .for_each(|s| *s = !all);
            }
            (_, KeyCode::Enter) => {
                app.pr_view.confirm = PrConfirm::None;
                return Some(Action::PrCreateMulti);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Switch platform dropdown
    if app.pr_view.confirm == PrConfirm::SwitchPlatform {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.pr_view.platform_idx > 0 => {
                app.pr_view.platform_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.pr_view.platform_idx + 1 < app.pr_view.available_platforms.len() =>
            {
                app.pr_view.platform_idx += 1;
            }
            (_, KeyCode::Enter) => {
                return Some(Action::PrSwitchPlatform);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Edit title
    if app.pr_view.confirm == PrConfirm::EditTitle {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.edit_input.clear();
                app.pr_view.edit_desc.clear();
            }
            (_, KeyCode::Enter) => {
                app.pr_view.confirm = PrConfirm::EditDesc;
            }
            (_, KeyCode::Backspace) => {
                app.pr_view.edit_input.pop();
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.pr_view.edit_input.push(c);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Edit description
    if app.pr_view.confirm == PrConfirm::EditDesc {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.edit_input.clear();
                app.pr_view.edit_desc.clear();
            }
            (_, KeyCode::Enter) => {
                app.pr_view.edit_desc.push('\n');
            }
            (_, KeyCode::Backspace) => {
                app.pr_view.edit_desc.pop();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                // Ctrl+S advances to base branch selection
                app.pr_view.confirm = PrConfirm::EditBase;
            }
            (_, KeyCode::Char(c))
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.pr_view.edit_desc.push(c);
                if app
                    .pr_view
                    .edit_desc
                    .chars()
                    .rev()
                    .take_while(|&ch| ch != '\n')
                    .count()
                    >= 56
                {
                    app.pr_view.edit_desc.push('\n');
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Edit base branch — dropdown
    if app.pr_view.confirm == PrConfirm::EditBase {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
                app.pr_view.edit_input.clear();
                app.pr_view.edit_desc.clear();
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.pr_view.branch_idx > 0 => {
                app.pr_view.branch_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                if app.pr_view.branch_idx + 1 < app.pr_view.branches.len() =>
            {
                app.pr_view.branch_idx += 1;
            }
            (_, KeyCode::Enter) => {
                app.pr_view.confirm = PrConfirm::None;
                return Some(Action::PrUpdate);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Merge method selector
    if app.pr_view.confirm == PrConfirm::Merge {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.pr_view.confirm = PrConfirm::None;
            }
            (_, KeyCode::Left) | (_, KeyCode::Char('h')) if app.pr_view.merge_method > 0 => {
                app.pr_view.merge_method -= 1;
            }
            (_, KeyCode::Right) | (_, KeyCode::Char('l')) if app.pr_view.merge_method < 2 => {
                app.pr_view.merge_method += 1;
            }
            (_, KeyCode::Enter) => {
                app.pr_view.confirm = PrConfirm::None;
                return Some(Action::PrMerge);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {}
        }
        return None;
    }

    // Close confirmation
    if app.pr_view.confirm == PrConfirm::Close {
        match (key.modifiers, key.code) {
            (_, KeyCode::Char('y')) => {
                app.pr_view.confirm = PrConfirm::None;
                return Some(Action::PrClose);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(Action::Quit),
            _ => {
                app.pr_view.confirm = PrConfirm::None;
            }
        }
        return None;
    }

    // Ops dropdown
    if app.pr_view.ops_mode {
        match (key.modifiers, key.code) {
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.pr_view.ops_idx > 0 => {
                app.pr_view.ops_idx -= 1;
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) if app.pr_view.ops_idx < 6 => {
                app.pr_view.ops_idx += 1;
            }
            (_, KeyCode::Enter) => {
                let idx = app.pr_view.ops_idx;
                app.pr_view.ops_mode = false;
                match idx {
                    0 => {
                        // create new PR/MR
                        app.pr_view.create_title.clear();
                        app.pr_view.create_base = "main".to_string();
                        app.pr_view.create_desc.clear();
                        app.pr_view.create_draft = false;
                        app.pr_view.create_input.clear();
                        app.pr_view.confirm = PrConfirm::CreateTitle;
                    }
                    1 => {
                        // edit PR/MR — pre-fill from selected
                        let (title, desc, base) = app
                            .pr_view
                            .prs
                            .get(app.pr_view.idx)
                            .map(|pr| {
                                (
                                    pr.title.clone(),
                                    pr.body.clone().unwrap_or_default(),
                                    pr.base.clone(),
                                )
                            })
                            .unwrap_or_default();
                        app.pr_view.edit_input = title;
                        app.pr_view.edit_desc = desc;
                        app.load_pr_branches();
                        app.pr_view.branch_idx = app
                            .pr_view
                            .branches
                            .iter()
                            .position(|b| *b == base)
                            .unwrap_or(0);
                        app.pr_view.confirm = PrConfirm::EditTitle;
                    }
                    2 => {
                        app.pr_view.merge_method = 0;
                        app.pr_view.confirm = PrConfirm::Merge;
                    }
                    3 => {
                        app.pr_view.confirm = PrConfirm::Close;
                    }
                    4 => return Some(Action::PrCheckout),
                    5 => return Some(Action::PrOpenBrowser),
                    6 => {
                        app.load_pr_platforms();
                        app.pr_view.confirm = PrConfirm::SwitchPlatform;
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                app.pr_view.ops_mode = false;
            }
            _ => {}
        }
        return None;
    }

    // Intercept ^r before global_nav steals it
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('r') {
        return Some(Action::PrRefresh);
    }
    if let Some(a) = handle_global_nav(key, app) {
        return Some(a);
    }
    match (key.modifiers, key.code) {
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => app.pr_move_up(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => app.pr_move_down(),
        (_, KeyCode::Tab) => {
            app.pr_view.filter = match app.pr_view.filter {
                crate::tui::app::PrStateFilter::Open => crate::tui::app::PrStateFilter::Closed,
                crate::tui::app::PrStateFilter::Closed => crate::tui::app::PrStateFilter::All,
                crate::tui::app::PrStateFilter::All => crate::tui::app::PrStateFilter::Open,
            };
            app.load_prs();
        }
        (_, KeyCode::Char('o')) => {
            app.pr_view.ops_mode = true;
            app.pr_view.ops_idx = 0;
        }
        _ => {}
    }
    None
}
