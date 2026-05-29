use std::io;
use std::path::{Path, PathBuf};
use dirs;
use crossterm::{
    execute,
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

const BRAND_COLOR: Color = Color::Rgb(255, 76, 76);
const SELECTED_BG: Color = Color::Rgb(40, 40, 60);
const C_WHITE: Color     = Color::Rgb(220, 220, 220);
const C_SUBTLE: Color    = Color::Rgb(140, 140, 160);
const C_DIM: Color       = Color::Rgb(80, 80, 100);
const C_GREEN: Color     = Color::Rgb(100, 220, 100);
#[allow(dead_code)]
const C_YELLOW: Color    = Color::Rgb(255, 210, 80);
#[allow(dead_code)]
const C_BORDER: Color    = Color::Rgb(60, 60, 80);

pub enum PickerResult {
    SingleRepo(PathBuf),
    Workspace { name: String, repos: Vec<PathBuf> },
    OpenWorkspace(String),
    Cancelled,
}

#[derive(PartialEq)]
enum PickerTab { Repos, Workspaces }

enum PickerMode {
    Selecting,
    NamingWorkspace,
}

struct SavedWorkspace {
    name: String,
    repos: Vec<String>,
}

fn load_saved_workspaces() -> Vec<SavedWorkspace> {
    let path = dirs::home_dir()
        .map(|h| h.join(".torii/workspaces.toml"))
        .unwrap_or_default();
    let Ok(content) = std::fs::read_to_string(&path) else { return vec![] };
    let mut out = Vec::new();
    let mut current: Option<SavedWorkspace> = None;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(ws) = current.take() { out.push(ws); }
            let name = line.trim_matches(|c| c == '[' || c == ']').to_string();
            current = Some(SavedWorkspace { name, repos: vec![] });
        } else if line.starts_with("path") {
            if let Some(ws) = current.as_mut() {
                let p = line.split('=').nth(1).unwrap_or("").trim().trim_matches('"').to_string();
                ws.repos.push(p);
            }
        }
    }
    if let Some(ws) = current { out.push(ws); }
    out
}

fn load_border_type() -> ratatui::widgets::BorderType {
    let path = dirs::home_dir()
        .map(|h| h.join(".torii/tui-settings.toml"))
        .unwrap_or_default();
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == "border_style" && v.trim().trim_matches('"') == "sharp" {
                    return ratatui::widgets::BorderType::Plain;
                }
            }
        }
    }
    ratatui::widgets::BorderType::Rounded
}

pub fn run_picker(start_dir: &Path) -> crate::error::Result<PickerResult> {
    let repos = find_git_repos(start_dir, 3);
    let saved_ws = load_saved_workspaces();
    let border_type = load_border_type();

    if repos.is_empty() && saved_ws.is_empty() {
        return Ok(PickerResult::Cancelled);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Estado pestaña repos
    let mut idx = 0usize;
    let mut selected: Vec<bool> = vec![false; repos.len()];
    let mut mode = PickerMode::Selecting;
    let mut ws_name = default_ws_name(start_dir);
    let mut ws_cursor = ws_name.len();

    // Estado pestaña workspaces
    let mut tab = if repos.is_empty() { PickerTab::Workspaces } else { PickerTab::Repos };
    let mut ws_idx = 0usize;   // workspace seleccionado en lista izquierda
    let mut ws_panel_right = false; // foco en lista de repos del workspace

    let result = loop {
        terminal.draw(|f| {
            let area = f.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(area);

            // ── Header ──────────────────────────────────────────────────────────
            {
                let inner_w = chunks[0].width.saturating_sub(2) as usize;
                let left_str = "⛩  gitorii";
                let right_str = "— select repositories";
                let left_len = left_str.chars().count() + 1;
                let right_len = right_str.chars().count() + 1;
                let pad = inner_w.saturating_sub(left_len + right_len);
                let line = Line::from(vec![
                    Span::raw(" "),
                    Span::styled(left_str, Style::default().fg(BRAND_COLOR).add_modifier(Modifier::BOLD)),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(right_str, Style::default().fg(C_SUBTLE)),
                    Span::raw(" "),
                ]);
                f.render_widget(
                    Paragraph::new(line)
                        .block(Block::default()
                            .borders(Borders::ALL)
                            .border_type(border_type)
                            .border_style(Style::default().fg(BRAND_COLOR))),
                    chunks[0],
                );
            }

            // ── Tabs ────────────────────────────────────────────────────────────
            let tab_repos_active = tab == PickerTab::Repos;
            let tab_ws_active    = tab == PickerTab::Workspaces;
            let tab_line = Line::from(vec![
                Span::raw(" "),
                Span::styled("[1] ", Style::default().fg(if tab_repos_active { C_WHITE } else { BRAND_COLOR }).add_modifier(if tab_repos_active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled(
                    format!("repos ({})", repos.len()),
                    Style::default().fg(if tab_repos_active { C_WHITE } else { BRAND_COLOR }),
                ),
                Span::raw("   "),
                Span::styled("[2] ", Style::default().fg(if tab_ws_active { C_WHITE } else { BRAND_COLOR }).add_modifier(if tab_ws_active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled(
                    format!("recent workspaces ({})", saved_ws.len()),
                    Style::default().fg(if tab_ws_active { C_WHITE } else { BRAND_COLOR }),
                ),
            ]);
            f.render_widget(
                Paragraph::new(tab_line).block(
                    Block::default().borders(Borders::ALL)
                        .border_type(border_type)
                        .border_style(Style::default().fg(BRAND_COLOR))
                ),
                chunks[1],
            );

            // ── Contenido ───────────────────────────────────────────────────────
            match tab {
                PickerTab::Repos => {
                    // Sub-layout vertical: lista + nombre ws + (si nombrando)
                    let sub = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(1), Constraint::Length(3)])
                        .split(chunks[2]);

                    let n_sel = selected.iter().filter(|&&s| s).count();
                    let items: Vec<ListItem> = repos.iter().enumerate().map(|(i, p)| {
                        let is_cur = i == idx;
                        let is_sel = selected[i];
                        let style = if is_cur {
                            Style::default().bg(SELECTED_BG).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        };
                        let check = if is_sel { "◆ " } else { "◇ " };
                        let name = p.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| p.to_string_lossy().to_string());
                        let path_str = p.to_string_lossy().to_string();
                        ListItem::new(Line::from(vec![
                            Span::styled(if is_cur { "▶ " } else { "  " }, Style::default().fg(BRAND_COLOR)),
                            Span::styled(check, Style::default().fg(if is_sel { C_GREEN } else { C_DIM })),
                            Span::styled(format!("{:<24}", name), Style::default().fg(if is_cur { C_WHITE } else { C_SUBTLE })),
                            Span::styled(path_str, Style::default().fg(C_DIM)),
                        ])).style(style)
                    }).collect();

                    let list_title = format!(" repos ({} found, {} selected) ", repos.len(), n_sel);
                    let mut list_state = ListState::default();
                    list_state.select(Some(idx));
                    f.render_stateful_widget(
                        List::new(items).block(Block::default()
                            .title(Span::styled(list_title, Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD)))
                            .borders(Borders::ALL)
                            .border_type(border_type)
                            .border_style(Style::default().fg(C_WHITE))),
                        sub[0],
                        &mut list_state,
                    );

                    let (ws_label, ws_style, ws_border) = match mode {
                        PickerMode::NamingWorkspace => (
                            " workspace name ",
                            Style::default().fg(C_WHITE).add_modifier(Modifier::BOLD),
                            Style::default().fg(C_WHITE),
                        ),
                        PickerMode::Selecting => (
                            " workspace name (auto) ",
                            Style::default().fg(BRAND_COLOR),
                            Style::default().fg(BRAND_COLOR),
                        ),
                    };
                    let name_content = match mode {
                        PickerMode::NamingWorkspace => {
                            // Snap to nearest char boundary so we never slice
                            // through a multi-byte codepoint (ñ, emoji, etc.)
                            // even if ws_cursor lands mid-char by accident.
                            let mut cursor = ws_cursor.min(ws_name.len());
                            while cursor > 0 && !ws_name.is_char_boundary(cursor) {
                                cursor -= 1;
                            }
                            let before = &ws_name[..cursor];
                            let cur_char = ws_name[cursor..].chars().next().unwrap_or(' ');
                            let after = if cursor >= ws_name.len() { "" } else {
                                &ws_name[cursor + cur_char.len_utf8()..]
                            };
                            Line::from(vec![
                                Span::raw(" "),
                                Span::styled(before, Style::default().fg(C_WHITE)),
                                Span::styled(cur_char.to_string(), Style::default().bg(BRAND_COLOR).fg(C_WHITE)),
                                Span::styled(after, Style::default().fg(C_WHITE)),
                            ])
                        }
                        PickerMode::Selecting => Line::from(vec![
                            Span::raw(" "),
                            Span::styled(&ws_name, Style::default().fg(C_DIM)),
                        ]),
                    };
                    f.render_widget(
                        Paragraph::new(name_content).block(Block::default()
                            .title(Span::styled(ws_label, ws_style))
                            .borders(Borders::ALL)
                            .border_type(border_type)
                            .border_style(ws_border)),
                        sub[1],
                    );
                }

                PickerTab::Workspaces => {
                    if saved_ws.is_empty() {
                        f.render_widget(
                            Paragraph::new(Span::styled(
                                "  no saved workspaces",
                                Style::default().fg(C_DIM),
                            )).block(Block::default().borders(Borders::ALL)
                                .border_type(border_type)
                                .border_style(Style::default().fg(BRAND_COLOR))),
                            chunks[2],
                        );
                    } else {
                        let cols = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([Constraint::Length(28), Constraint::Min(1)])
                            .split(chunks[2]);

                        // Lista workspaces izquierda
                        let ws_list_items: Vec<ListItem> = saved_ws.iter().enumerate().map(|(i, ws)| {
                            let is_cur = i == ws_idx;
                            let style = if is_cur {
                                Style::default().bg(SELECTED_BG).add_modifier(Modifier::BOLD)
                            } else { Style::default() };
                            ListItem::new(Line::from(vec![
                                Span::styled(if is_cur && !ws_panel_right { "▶ " } else { "  " }, Style::default().fg(BRAND_COLOR)),
                                Span::styled(format!("{:<20}", ws.name), Style::default().fg(if is_cur { C_WHITE } else { C_SUBTLE })),
                                Span::styled(format!(" {}", ws.repos.len()), Style::default().fg(C_DIM)),
                            ])).style(style)
                        }).collect();

                        let left_border = if !ws_panel_right { C_WHITE } else { BRAND_COLOR };
                        let mut left_state = ListState::default();
                        left_state.select(Some(ws_idx));
                        f.render_stateful_widget(
                            List::new(ws_list_items).block(Block::default()
                                .title(Span::styled(" workspaces ", Style::default().fg(left_border)))
                                .borders(Borders::ALL)
                                .border_type(border_type)
                                .border_style(Style::default().fg(left_border))),
                            cols[0],
                            &mut left_state,
                        );

                        // Lista repos del workspace seleccionado (derecha)
                        let repo_items: Vec<ListItem> = saved_ws.get(ws_idx)
                            .map(|ws| ws.repos.iter().map(|p| {
                                let name = std::path::Path::new(p)
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| p.clone());
                                ListItem::new(Line::from(vec![
                                    Span::styled("  ", Style::default()),
                                    Span::styled(format!("{:<22}", name), Style::default().fg(C_SUBTLE)),
                                    Span::styled(p.as_str(), Style::default().fg(C_DIM)),
                                ]))
                            }).collect())
                            .unwrap_or_default();

                        let ws_title = saved_ws.get(ws_idx)
                            .map(|ws| format!(" {} — repos ", ws.name))
                            .unwrap_or_else(|| " repos ".to_string());
                        let right_border = if ws_panel_right { C_WHITE } else { BRAND_COLOR };
                        f.render_widget(
                            List::new(repo_items).block(Block::default()
                                .title(Span::styled(ws_title, Style::default().fg(right_border)))
                                .borders(Borders::ALL)
                                .border_type(border_type)
                                .border_style(Style::default().fg(right_border))),
                            cols[1],
                        );
                    }
                }
            }

            // ── Hint ────────────────────────────────────────────────────────────
            let hint = match tab {
                PickerTab::Repos => match mode {
                    PickerMode::Selecting => {
                        let n = selected.iter().filter(|&&s| s).count();
                        if n == 1 {
                            Line::from(vec![
                                hint_key("[↑↓]"), hint_txt(" nav  "),
                                hint_key("[space]"), hint_txt(" mark  "),
                                hint_key("[Enter]"), hint_txt(" open repo  "),
                                hint_key("[2]"), hint_txt(" workspaces  "),
                                hint_key("[q]"), hint_txt(" quit"),
                            ])
                        } else if n > 1 {
                            Line::from(vec![
                                hint_key("[space]"), hint_txt(" mark  "),
                                hint_key("[a]"), hint_txt(" all  "),
                                hint_key("[A]"), hint_txt(" none  "),
                                hint_key("[Enter]"), hint_txt(" create workspace  "),
                                hint_key("[q]"), hint_txt(" quit"),
                            ])
                        } else {
                            Line::from(vec![
                                hint_key("[↑↓/jk]"), hint_txt(" nav  "),
                                hint_key("[space]"), hint_txt(" mark  "),
                                hint_key("[a]"), hint_txt(" all  "),
                                hint_key("[2]"), hint_txt(" workspaces  "),
                                hint_key("[q]"), hint_txt(" quit"),
                            ])
                        }
                    }
                    PickerMode::NamingWorkspace => Line::from(vec![
                        hint_key("[Enter]"), hint_txt(" confirm  "),
                        hint_key("[Esc]"), hint_txt(" cancel"),
                    ]),
                },
                PickerTab::Workspaces => Line::from(vec![
                    hint_key("[↑↓/jk]"), hint_txt(" nav  "),
                    hint_key("[→/←]"), hint_txt(" foco  "),
                    hint_key("[Enter]"), hint_txt(" open workspace  "),
                    hint_key("[1]"), hint_txt(" repos  "),
                    hint_key("[q]"), hint_txt(" quit"),
                ]),
            };
            f.render_widget(Paragraph::new(hint), chunks[3]);
        })?;

        if !event::poll(std::time::Duration::from_millis(200))? { continue; }
        if let Event::Key(key) = event::read()? {
            // Cambio de tab global
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('1')) => { tab = PickerTab::Repos; continue; }
                (_, KeyCode::Char('2')) => { tab = PickerTab::Workspaces; continue; }
                _ => {}
            }

            match tab {
                PickerTab::Repos => match mode {
                    PickerMode::Selecting => match (key.modifiers, key.code) {
                        (_, KeyCode::Char('q')) |
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) => break PickerResult::Cancelled,
                        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                            if idx > 0 { idx -= 1; }
                        }
                        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                            if idx + 1 < repos.len() { idx += 1; }
                        }
                        (_, KeyCode::Char(' ')) => { selected[idx] = !selected[idx]; }
                        (_, KeyCode::Char('a')) => {
                            let all = selected.iter().all(|&s| s);
                            selected.iter_mut().for_each(|s| *s = !all);
                        }
                        (KeyModifiers::SHIFT, KeyCode::Char('A')) |
                        (_, KeyCode::Char('A')) => {
                            selected.iter_mut().for_each(|s| *s = false);
                        }
                        (_, KeyCode::Enter) => {
                            let sel_repos: Vec<PathBuf> = repos.iter().enumerate()
                                .filter(|(i, _)| selected[*i])
                                .map(|(_, p)| p.clone())
                                .collect();
                            if sel_repos.len() == 1 {
                                break PickerResult::SingleRepo(sel_repos.into_iter().next().unwrap());
                            } else if sel_repos.len() > 1 {
                                mode = PickerMode::NamingWorkspace;
                                ws_cursor = ws_name.len();
                            }
                        }
                        _ => {}
                    },
                    PickerMode::NamingWorkspace => match (key.modifiers, key.code) {
                        (_, KeyCode::Esc) => { mode = PickerMode::Selecting; }
                        (_, KeyCode::Enter) => {
                            let name = if ws_name.trim().is_empty() {
                                default_ws_name(start_dir)
                            } else {
                                ws_name.trim().to_string()
                            };
                            let sel_repos: Vec<PathBuf> = repos.iter().enumerate()
                                .filter(|(i, _)| selected[*i])
                                .map(|(_, p)| p.clone())
                                .collect();
                            break PickerResult::Workspace { name, repos: sel_repos };
                        }
                        (_, KeyCode::Backspace) => {
                            if ws_cursor > 0 { ws_name.remove(ws_cursor - 1); ws_cursor -= 1; }
                        }
                        (_, KeyCode::Left)  => { if ws_cursor > 0 { ws_cursor -= 1; } }
                        (_, KeyCode::Right) => { if ws_cursor < ws_name.len() { ws_cursor += 1; } }
                        (_, KeyCode::Char(c)) if key.modifiers == KeyModifiers::NONE
                                              || key.modifiers == KeyModifiers::SHIFT => {
                            ws_name.insert(ws_cursor, c);
                            ws_cursor += 1;
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) => break PickerResult::Cancelled,
                        _ => {}
                    },
                },

                PickerTab::Workspaces => match (key.modifiers, key.code) {
                    (_, KeyCode::Char('q')) |
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) => break PickerResult::Cancelled,
                    (_, KeyCode::Up)   | (_, KeyCode::Char('k')) => {
                        if !ws_panel_right && ws_idx > 0 { ws_idx -= 1; }
                    }
                    (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                        if !ws_panel_right && ws_idx + 1 < saved_ws.len() { ws_idx += 1; }
                    }
                    (_, KeyCode::Right) | (_, KeyCode::Char('l')) => { ws_panel_right = true; }
                    (_, KeyCode::Left)  | (_, KeyCode::Char('h')) => { ws_panel_right = false; }
                    (_, KeyCode::Enter) => {
                        if let Some(ws) = saved_ws.get(ws_idx) {
                            break PickerResult::OpenWorkspace(ws.name.clone());
                        }
                    }
                    _ => {}
                },
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(result)
}

fn hint_key(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(BRAND_COLOR))
}
fn hint_txt(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(C_SUBTLE))
}

fn find_git_repos(base: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    scan_dir(base, base, 0, max_depth, &mut repos);
    repos.sort();
    repos
}

const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", ".venv", "venv", "dist", "build", ".next",
    "__pycache__", ".cache", ".parcel-cache",
];

fn load_ignore_patterns(dir: &Path) -> Vec<String> {
    let names = [".toriignore", ".gitignore"];
    for name in &names {
        let p = dir.join(name);
        if let Ok(content) = std::fs::read_to_string(&p) {
            return content
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
        }
    }
    vec![]
}

fn is_ignored(name: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        let pat = pat.trim_end_matches('/');
        if pat == name { return true; }
        if pat.starts_with('!') { continue; }
        // simple glob: *.ext
        if let Some(ext) = pat.strip_prefix("*.") {
            if name.ends_with(&format!(".{}", ext)) { return true; }
        }
    }
    false
}

fn has_git(dir: &Path) -> bool {
    // .git puede ser carpeta (repo normal) o archivo (submodule/worktree gitfile)
    dir.join(".git").exists()
}

fn scan_dir(base: &Path, dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth { return; }
    if has_git(dir) {
        if dir != base {
            out.push(dir.to_path_buf());
        }
        return;
    }
    let ignore = load_ignore_patterns(dir);
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') { continue; }
        if SKIP_DIRS.contains(&name.as_str()) { continue; }
        if is_ignored(&name, &ignore) { continue; }
        scan_dir(base, &path, depth + 1, max_depth, out);
    }
}

fn default_ws_name(dir: &Path) -> String {
    let base = dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    format!("{}-{}", base, date)
}

pub fn save_workspace(name: &str, repos: &[PathBuf]) -> crate::error::Result<()> {
    let path = dirs::home_dir()
        .map(|h| h.join(".torii/workspaces.toml"))
        .unwrap_or_default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    content.push_str(&format!("\n[{}]\n", name));
    for repo in repos {
        content.push_str(&format!("path = \"{}\"\n", repo.display()));
    }
    std::fs::write(&path, content)?;
    Ok(())
}
