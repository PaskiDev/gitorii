//! Terminal resolver for merge/rebase/cherry-pick conflicts: a one-key
//! prompt (local / remote / file by file / manual) and a per-file selector.
//! The state machine is pure; only `TerminalResolver` touches the terminal.

#![allow(dead_code)]

use std::io::{IsTerminal, Write};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Color, Stylize};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, queue};
use torii_lib::vcs::conflict::{
    ConflictContext, ConflictFile, ConflictResolver, OpKind, Resolution, Side,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileState {
    Conflict,
    Remote,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKey {
    Up,
    Down,
    Space,
    Backspace,
    Enter,
    Cancel,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PickerAction {
    Continue,
    Apply(Resolution),
    Cancel,
}

pub struct PickerState {
    pub files: Vec<(String, FileState)>,
    pub cursor: usize,
}

impl PickerState {
    pub fn new(files: &[ConflictFile]) -> Self {
        Self {
            files: files.iter().map(|f| (f.path.clone(), FileState::Conflict)).collect(),
            cursor: 0,
        }
    }

    pub fn handle(&mut self, key: PickerKey) -> PickerAction {
        match key {
            PickerKey::Up => self.cursor = self.cursor.saturating_sub(1),
            PickerKey::Down => {
                if self.cursor + 1 < self.files.len() {
                    self.cursor += 1;
                }
            }
            PickerKey::Space => {
                if let Some((_, st)) = self.files.get_mut(self.cursor) {
                    *st = match *st {
                        FileState::Conflict | FileState::Local => FileState::Remote,
                        FileState::Remote => FileState::Local,
                    };
                }
            }
            PickerKey::Backspace => {
                if let Some((_, st)) = self.files.get_mut(self.cursor) {
                    *st = FileState::Conflict;
                }
            }
            PickerKey::Enter => {
                let picks = self
                    .files
                    .iter()
                    .filter_map(|(p, st)| match st {
                        FileState::Conflict => None,
                        FileState::Remote => Some((p.clone(), Side::Remote)),
                        FileState::Local => Some((p.clone(), Side::Local)),
                    })
                    .collect();
                return PickerAction::Apply(Resolution::PerFile(picks));
            }
            PickerKey::Cancel => return PickerAction::Cancel,
        }
        PickerAction::Continue
    }
}

pub struct TerminalResolver {
    pub forced: Option<Side>,
}

impl ConflictResolver for TerminalResolver {
    fn choose(&self, ctx: &ConflictContext, files: &[ConflictFile]) -> Result<Resolution> {
        if let Some(side) = self.forced {
            return Ok(Resolution::All(side));
        }
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return Ok(Resolution::Manual);
        }
        let mut err = std::io::stderr();
        let _raw = RawGuard::enter()?;
        print_prompt(&mut err, ctx, files.len())?;
        loop {
            let Event::Key(k) = event::read()? else { continue };
            match prompt_key(&k) {
                Some(PromptKey::Local) => return Ok(Resolution::All(Side::Local)),
                Some(PromptKey::Remote) => return Ok(Resolution::All(Side::Remote)),
                Some(PromptKey::Manual) => return Ok(Resolution::Manual),
                Some(PromptKey::ByFile) => return run_selector(&mut err, files),
                None => {}
            }
        }
    }
}

/// Raw mode on creation, restored on drop (errors and panics included).
struct RawGuard;

impl RawGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(RawGuard)
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let mut err = std::io::stderr();
        let _ = queue!(err, cursor::Show);
        let _ = err.flush();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromptKey {
    Local,
    Remote,
    Manual,
    ByFile,
}

fn is_press(ev: &KeyEvent) -> bool {
    ev.kind != KeyEventKind::Release
}

fn is_ctrl_c(ev: &KeyEvent) -> bool {
    ev.modifiers.contains(KeyModifiers::CONTROL) && ev.code == KeyCode::Char('c')
}

fn prompt_key(ev: &KeyEvent) -> Option<PromptKey> {
    if !is_press(ev) {
        return None;
    }
    if is_ctrl_c(ev) {
        return Some(PromptKey::Manual);
    }
    match ev.code {
        KeyCode::Char('l') => Some(PromptKey::Local),
        KeyCode::Char('r') => Some(PromptKey::Remote),
        KeyCode::Char('m') | KeyCode::Esc => Some(PromptKey::Manual),
        KeyCode::Char('f') => Some(PromptKey::ByFile),
        _ => None,
    }
}

fn picker_key(ev: &KeyEvent) -> Option<PickerKey> {
    if !is_press(ev) {
        return None;
    }
    if is_ctrl_c(ev) {
        return Some(PickerKey::Cancel);
    }
    match ev.code {
        KeyCode::Up | KeyCode::Char('k') => Some(PickerKey::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(PickerKey::Down),
        KeyCode::Char(' ') => Some(PickerKey::Space),
        KeyCode::Backspace => Some(PickerKey::Backspace),
        KeyCode::Enter => Some(PickerKey::Enter),
        KeyCode::Esc | KeyCode::Char('q') => Some(PickerKey::Cancel),
        _ => None,
    }
}

fn op_name(op: OpKind) -> &'static str {
    match op {
        OpKind::Merge => "merge",
        OpKind::Rebase => "rebase",
        OpKind::CherryPick => "cherry-pick",
    }
}

fn print_prompt(w: &mut impl Write, ctx: &ConflictContext, n: usize) -> Result<()> {
    let lines = [
        format!(
            "⚠️  {n} files have conflicts ({}: {} into {})",
            op_name(ctx.op),
            ctx.remote_label,
            ctx.local_label
        ),
        format!("  local  = {} (your work)", ctx.local_label),
        format!("  remote = {} (incoming)", ctx.remote_label),
        "Resolve: [l] local wins everywhere  [r] remote wins everywhere".to_string(),
        "         [f] choose file by file    [m] leave markers and resolve manually".to_string(),
    ];
    for l in lines {
        write!(w, "{l}\r\n")?;
    }
    w.flush()?;
    Ok(())
}

fn state_label(st: FileState) -> (&'static str, Color) {
    match st {
        FileState::Conflict => ("conflict", Color::Yellow),
        FileState::Remote => ("remote  ", Color::Blue),
        FileState::Local => ("local   ", Color::Green),
    }
}

/// Draws the selector and returns how many lines it wrote.
fn draw_selector(w: &mut impl Write, st: &PickerState) -> Result<u16> {
    let mut lines = 0u16;
    write!(w, "\r\n")?;
    lines += 1;
    for (i, (path, fs)) in st.files.iter().enumerate() {
        let (label, color) = state_label(*fs);
        let mark = if i == st.cursor { ">" } else { " " };
        write!(w, "{mark} {} {path}\r\n", label.with(color))?;
        lines += 1;
    }
    write!(
        w,
        "\r\n  {} conflict  {} remote  {} local\r\n",
        "●".with(Color::Yellow),
        "●".with(Color::Blue),
        "●".with(Color::Green)
    )?;
    write!(
        w,
        "  space cycle remote/local  backspace back to conflict  ↑↓/jk move  enter apply  esc/q cancel\r\n"
    )?;
    lines += 3;
    w.flush()?;
    Ok(lines)
}

fn run_selector(w: &mut impl Write, files: &[ConflictFile]) -> Result<Resolution> {
    let mut st = PickerState::new(files);
    queue!(w, cursor::Hide)?;
    let mut drawn = draw_selector(w, &st)?;
    loop {
        let Event::Key(k) = event::read()? else { continue };
        let Some(key) = picker_key(&k) else { continue };
        match st.handle(key) {
            PickerAction::Continue => {
                queue!(w, cursor::MoveUp(drawn), cursor::MoveToColumn(0))?;
                queue!(w, terminal::Clear(ClearType::FromCursorDown))?;
                drawn = draw_selector(w, &st)?;
            }
            PickerAction::Apply(r) => return Ok(r),
            PickerAction::Cancel => return Ok(Resolution::Manual),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cf(p: &str) -> ConflictFile {
        ConflictFile { path: p.into(), hunks: true }
    }
    fn state(names: &[&str]) -> PickerState {
        PickerState::new(&names.iter().map(|n| cf(n)).collect::<Vec<_>>())
    }
    fn ev(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn space_cycles_remote_local_remote() {
        let mut s = state(&["a"]);
        assert_eq!(s.files[0].1, FileState::Conflict);
        assert_eq!(s.handle(PickerKey::Space), PickerAction::Continue);
        assert_eq!(s.files[0].1, FileState::Remote);
        s.handle(PickerKey::Space);
        assert_eq!(s.files[0].1, FileState::Local);
        s.handle(PickerKey::Space);
        assert_eq!(s.files[0].1, FileState::Remote);
    }

    #[test]
    fn backspace_returns_to_conflict() {
        let mut s = state(&["a"]);
        s.handle(PickerKey::Space);
        s.handle(PickerKey::Backspace);
        assert_eq!(s.files[0].1, FileState::Conflict);
    }

    #[test]
    fn enter_applies_only_decided_files_in_order() {
        let mut s = state(&["a", "b", "c"]);
        s.handle(PickerKey::Space); // a: Remote
        s.handle(PickerKey::Down);
        s.handle(PickerKey::Down);
        s.handle(PickerKey::Space);
        s.handle(PickerKey::Space); // c: Local
        assert_eq!(
            s.handle(PickerKey::Enter),
            PickerAction::Apply(Resolution::PerFile(vec![
                ("a".into(), Side::Remote),
                ("c".into(), Side::Local)
            ]))
        );
    }

    #[test]
    fn cursor_stays_in_bounds() {
        let mut s = state(&["a", "b"]);
        s.handle(PickerKey::Up);
        assert_eq!(s.cursor, 0);
        s.handle(PickerKey::Down);
        s.handle(PickerKey::Down);
        s.handle(PickerKey::Down);
        assert_eq!(s.cursor, 1);
    }

    #[test]
    fn cancel_cancels() {
        let mut s = state(&["a"]);
        assert_eq!(s.handle(PickerKey::Cancel), PickerAction::Cancel);
    }

    #[test]
    fn forced_side_skips_prompt() {
        let ctx = ConflictContext {
            op: OpKind::Merge,
            local_label: "main".into(),
            remote_label: "feat".into(),
        };
        let r = TerminalResolver { forced: Some(Side::Local) };
        assert_eq!(r.choose(&ctx, &[cf("a")]).unwrap(), Resolution::All(Side::Local));
    }

    #[test]
    fn selector_keys_map() {
        assert_eq!(picker_key(&ev(KeyCode::Up)), Some(PickerKey::Up));
        assert_eq!(picker_key(&ev(KeyCode::Char('k'))), Some(PickerKey::Up));
        assert_eq!(picker_key(&ev(KeyCode::Down)), Some(PickerKey::Down));
        assert_eq!(picker_key(&ev(KeyCode::Char('j'))), Some(PickerKey::Down));
        assert_eq!(picker_key(&ev(KeyCode::Char(' '))), Some(PickerKey::Space));
        assert_eq!(picker_key(&ev(KeyCode::Backspace)), Some(PickerKey::Backspace));
        assert_eq!(picker_key(&ev(KeyCode::Enter)), Some(PickerKey::Enter));
        assert_eq!(picker_key(&ev(KeyCode::Esc)), Some(PickerKey::Cancel));
        assert_eq!(picker_key(&ev(KeyCode::Char('q'))), Some(PickerKey::Cancel));
        assert_eq!(picker_key(&ev(KeyCode::Char('x'))), None);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(picker_key(&ctrl_c), Some(PickerKey::Cancel));
        let mut rel = ev(KeyCode::Enter);
        rel.kind = KeyEventKind::Release;
        assert_eq!(picker_key(&rel), None);
    }

    #[test]
    fn prompt_keys_map() {
        assert_eq!(prompt_key(&ev(KeyCode::Char('l'))), Some(PromptKey::Local));
        assert_eq!(prompt_key(&ev(KeyCode::Char('r'))), Some(PromptKey::Remote));
        assert_eq!(prompt_key(&ev(KeyCode::Char('m'))), Some(PromptKey::Manual));
        assert_eq!(prompt_key(&ev(KeyCode::Esc)), Some(PromptKey::Manual));
        assert_eq!(prompt_key(&ev(KeyCode::Char('f'))), Some(PromptKey::ByFile));
        assert_eq!(prompt_key(&ev(KeyCode::Enter)), None);
        assert_eq!(prompt_key(&ev(KeyCode::Char('x'))), None);
    }
}
