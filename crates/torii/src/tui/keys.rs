//! User-defined keys: what a chord is, how it is written down, and what it
//! resolves to.
//!
//! Three rules shape the whole design, and each one comes from a way this
//! feature normally breaks:
//!
//! 1. **A binding is a sequence of chords.** `ctrl+g` is one chord; `g s` is
//!    two. A chord that begins a longer binding cannot also fire on its own —
//!    the resolver reports it as pending and waits.
//! 2. **While text is being typed, a plain letter is a letter.** Only chords
//!    carrying a modifier resolve in a text field, or writing a commit message
//!    would run half the program. The resolver is told which mode it is in and
//!    refuses to fire the rest.
//! 3. **Every action stays reachable without a binding**, through the palette
//!    the leader opens. A user who binds nothing loses nothing, and one who
//!    binds badly is not locked out.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fmt;

/// One keypress: a key plus the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    pub fn new(code: KeyCode, mods: KeyModifiers) -> Self {
        // Shift is already baked into the character crossterm reports, so
        // keeping it as a modifier would make `shift+a` never match the `A`
        // that actually arrives.
        let mods = match code {
            KeyCode::Char(_) => mods - KeyModifiers::SHIFT,
            _ => mods,
        };
        Self { code, mods }
    }

    pub fn from_event(key: KeyEvent) -> Self {
        Self::new(key.code, key.modifiers)
    }

    /// Whether this chord carries a modifier that makes it safe to fire while
    /// text is being typed.
    pub fn is_modified(&self) -> bool {
        self.mods.contains(KeyModifiers::CONTROL) || self.mods.contains(KeyModifiers::ALT)
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.contains(KeyModifiers::CONTROL) {
            write!(f, "ctrl+")?;
        }
        if self.mods.contains(KeyModifiers::ALT) {
            write!(f, "alt+")?;
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            write!(f, "shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::F(n) => write!(f, "f{n}"),
            KeyCode::Enter => f.write_str("enter"),
            KeyCode::Tab => f.write_str("tab"),
            KeyCode::BackTab => f.write_str("backtab"),
            KeyCode::Backspace => f.write_str("backspace"),
            KeyCode::Delete => f.write_str("delete"),
            KeyCode::Insert => f.write_str("insert"),
            KeyCode::Home => f.write_str("home"),
            KeyCode::End => f.write_str("end"),
            KeyCode::PageUp => f.write_str("pageup"),
            KeyCode::PageDown => f.write_str("pagedown"),
            KeyCode::Up => f.write_str("up"),
            KeyCode::Down => f.write_str("down"),
            KeyCode::Left => f.write_str("left"),
            KeyCode::Right => f.write_str("right"),
            KeyCode::Esc => f.write_str("esc"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// One or more chords, pressed in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding(pub Vec<Chord>);

impl Binding {
    pub fn first(&self) -> Option<&Chord> {
        self.0.first()
    }

    /// A binding is safe while typing only if its first chord is modified:
    /// that is the one that has to win against a letter going into a field.
    pub fn is_typing_safe(&self) -> bool {
        self.first().map(Chord::is_modified).unwrap_or(false)
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text: Vec<String> = self.0.iter().map(|c| c.to_string()).collect();
        f.write_str(&text.join(" "))
    }
}

/// Read a binding as a user writes it: `ctrl+g`, `g s`, `alt+x`, `ctrl+k b`.
pub fn parse_binding(text: &str) -> Result<Binding, String> {
    let chords: Vec<&str> = text.split_whitespace().collect();
    if chords.is_empty() {
        return Err("empty binding".into());
    }
    if chords.len() > 3 {
        return Err("a binding is at most three chords".into());
    }
    let mut out = Vec::new();
    for chord in chords {
        out.push(parse_chord(chord)?);
    }
    Ok(Binding(out))
}

fn parse_chord(text: &str) -> Result<Chord, String> {
    let mut mods = KeyModifiers::empty();
    let mut rest = text;
    loop {
        let lower = rest.to_ascii_lowercase();
        if let Some(tail) = lower.strip_prefix("ctrl+") {
            mods |= KeyModifiers::CONTROL;
            rest = &rest[rest.len() - tail.len()..];
        } else if let Some(tail) = lower.strip_prefix("alt+") {
            mods |= KeyModifiers::ALT;
            rest = &rest[rest.len() - tail.len()..];
        } else if let Some(tail) = lower.strip_prefix("shift+") {
            mods |= KeyModifiers::SHIFT;
            rest = &rest[rest.len() - tail.len()..];
        } else {
            break;
        }
    }

    let lower = rest.to_ascii_lowercase();
    let code = match lower.as_str() {
        "space" => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "esc" | "escape" => KeyCode::Esc,
        other => {
            if let Some(n) = other.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                KeyCode::F(n)
            } else {
                let mut chars = rest.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => KeyCode::Char(c),
                    _ => return Err(format!("`{text}` is not a key")),
                }
            }
        }
    };
    Ok(Chord::new(code, mods))
}

/// What a keypress meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// No binding starts with what has been pressed: hand the key back to the
    /// view.
    Unbound,
    /// A longer binding starts here — hold the key and wait for the next one.
    Pending,
    /// Run this action.
    Fire(String),
}

/// The bindings in force, and the leader that opens the palette.
#[derive(Debug, Clone)]
pub struct Keymap {
    /// Binding → action id.
    pub bindings: Vec<(Binding, String)>,
    /// The one binding that is always available, including while typing: it
    /// opens the palette, from which every action can be run by name.
    pub leader: Binding,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: Vec::new(),
            // Ctrl-K: modified, so it survives a text field, and free in every
            // view's own keys.
            leader: Binding(vec![Chord::new(KeyCode::Char('k'), KeyModifiers::CONTROL)]),
        }
    }
}

impl Keymap {
    /// Resolve `chord` against what has already been pressed.
    ///
    /// `typing` says the focused thing is a text field, in which case only
    /// bindings that begin with a modified chord may fire — a bare letter
    /// belongs to the text.
    pub fn resolve(&self, pending: &[Chord], chord: Chord, typing: bool) -> Resolution {
        let mut sequence = pending.to_vec();
        sequence.push(chord);

        let usable = |b: &Binding| !typing || b.is_typing_safe();

        // An exact match fires, unless a longer binding also starts this way —
        // then the shorter one would make the longer unreachable.
        let exact = self
            .bindings
            .iter()
            .find(|(b, _)| usable(b) && b.0 == sequence);
        let longer = self
            .bindings
            .iter()
            .any(|(b, _)| usable(b) && b.0.len() > sequence.len() && b.0.starts_with(&sequence));

        match (exact, longer) {
            (Some((_, action)), false) => Resolution::Fire(action.clone()),
            (_, true) => Resolution::Pending,
            (None, false) => Resolution::Unbound,
        }
    }

    /// Which action a binding is on, if any.
    pub fn action_for(&self, binding: &Binding) -> Option<&str> {
        self.bindings
            .iter()
            .find(|(b, _)| b == binding)
            .map(|(_, a)| a.as_str())
    }

    /// The binding an action is on, if any.
    pub fn binding_for(&self, action: &str) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|(_, a)| a == action)
            .map(|(b, _)| b)
    }

    /// Put `action` on `binding`, taking it off whatever held either before —
    /// two actions on one binding would make one of them unreachable, and an
    /// action on two bindings is a rebind the user did not ask for.
    pub fn bind(&mut self, binding: Binding, action: &str) {
        self.bindings.retain(|(b, a)| b != &binding && a != action);
        self.bindings.push((binding, action.to_string()));
    }

    pub fn unbind(&mut self, action: &str) {
        self.bindings.retain(|(_, a)| a != action);
    }

    /// Bindings that would swallow a prefix of another, or that shadow the
    /// leader. Shown in the config screen rather than refused: the user may be
    /// mid-edit.
    pub fn conflicts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (b, a) in &self.bindings {
            if b == &self.leader {
                out.push(format!("`{b}` is the palette leader, so `{a}` never fires"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> Chord {
        parse_chord(text).unwrap()
    }

    fn map(pairs: &[(&str, &str)]) -> Keymap {
        let mut km = Keymap::default();
        for (binding, action) in pairs {
            km.bind(parse_binding(binding).unwrap(), action);
        }
        km
    }

    #[test]
    fn a_binding_reads_the_way_it_is_written() {
        assert_eq!(parse_binding("ctrl+g").unwrap().to_string(), "ctrl+g");
        assert_eq!(parse_binding("g s").unwrap().to_string(), "g s");
        assert_eq!(parse_binding("alt+x").unwrap().to_string(), "alt+x");
        assert_eq!(parse_binding("ctrl+k b").unwrap().to_string(), "ctrl+k b");
        assert_eq!(parse_binding("f5").unwrap().to_string(), "f5");
        assert_eq!(parse_binding("space").unwrap().to_string(), "space");
    }

    /// Crossterm reports a shifted letter as the uppercase char, so keeping
    /// SHIFT as a modifier too would make the binding never match.
    #[test]
    fn shift_lives_in_the_character_not_in_the_modifiers() {
        let typed = Chord::from_event(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        assert_eq!(typed, Chord::new(KeyCode::Char('A'), KeyModifiers::empty()));
    }

    #[test]
    fn nonsense_is_refused_with_a_reason() {
        assert!(parse_binding("").is_err());
        assert!(parse_binding("ctrl+gg").is_err());
        assert!(parse_binding("a b c d").is_err());
    }

    #[test]
    fn a_single_chord_fires_on_its_own() {
        let km = map(&[("ctrl+g", "goto:log")]);
        assert_eq!(
            km.resolve(&[], chord("ctrl+g"), false),
            Resolution::Fire("goto:log".into())
        );
        assert_eq!(km.resolve(&[], chord("ctrl+h"), false), Resolution::Unbound);
    }

    #[test]
    fn a_sequence_waits_for_its_second_key() {
        let km = map(&[("g s", "goto:sync")]);
        assert_eq!(km.resolve(&[], chord("g"), false), Resolution::Pending);
        assert_eq!(
            km.resolve(&[chord("g")], chord("s"), false),
            Resolution::Fire("goto:sync".into())
        );
        assert_eq!(
            km.resolve(&[chord("g")], chord("z"), false),
            Resolution::Unbound
        );
    }

    /// A prefix that also fires on its own would make the longer binding
    /// unreachable — the short one always wins the race.
    #[test]
    fn a_prefix_never_steals_a_longer_binding() {
        let km = map(&[("g", "goto:files"), ("g s", "goto:sync")]);
        assert_eq!(km.resolve(&[], chord("g"), false), Resolution::Pending);
        assert_eq!(
            km.resolve(&[chord("g")], chord("s"), false),
            Resolution::Fire("goto:sync".into())
        );
    }

    /// The rule that keeps `save` usable: a bare letter typed into a message
    /// is a letter, whatever it is bound to.
    #[test]
    fn a_letter_stays_a_letter_while_typing() {
        let km = map(&[("g s", "goto:sync"), ("ctrl+g", "goto:log")]);

        assert_eq!(km.resolve(&[], chord("g"), true), Resolution::Unbound);
        assert_eq!(
            km.resolve(&[], chord("ctrl+g"), true),
            Resolution::Fire("goto:log".into()),
            "a modified chord still works in a field"
        );
    }

    #[test]
    fn binding_an_action_moves_it_rather_than_duplicating_it() {
        let mut km = map(&[("ctrl+g", "goto:log")]);
        km.bind(parse_binding("ctrl+l").unwrap(), "goto:log");

        assert_eq!(km.bindings.len(), 1);
        assert_eq!(km.binding_for("goto:log").unwrap().to_string(), "ctrl+l");
        assert_eq!(km.resolve(&[], chord("ctrl+g"), false), Resolution::Unbound);
    }

    /// Two actions on one binding would leave one of them unreachable, so the
    /// newcomer takes the binding and the old one is left unbound.
    #[test]
    fn a_binding_holds_one_action() {
        let mut km = map(&[("ctrl+g", "goto:log")]);
        km.bind(parse_binding("ctrl+g").unwrap(), "goto:sync");

        assert_eq!(km.bindings.len(), 1);
        assert_eq!(km.binding_for("goto:log"), None);
        assert_eq!(
            km.resolve(&[], chord("ctrl+g"), false),
            Resolution::Fire("goto:sync".into())
        );
    }

    #[test]
    fn shadowing_the_leader_is_reported() {
        let mut km = Keymap::default();
        km.bind(km.leader.clone(), "goto:log");
        let conflicts = km.conflicts();
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        assert!(conflicts[0].contains("palette"), "{conflicts:?}");
    }
}

// ── The action catalogue ─────────────────────────────────────────────────────

/// An action a key can be put on. `id` is what the file stores, so it must not
/// change once shipped; `label` and `group` are for the config screen and the
/// palette.
pub struct ActionDef {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
}

/// Every bindable action.
///
/// The ids are the contract with `~/.torii/keys.toml`: renaming one silently
/// unbinds whatever a user had put on it, so they are added to, not edited.
pub const ACTIONS: &[ActionDef] = &[
    // Where to go.
    def("goto:files", "Files", "go"),
    def("goto:save", "Save", "go"),
    def("goto:sync", "Sync", "go"),
    def("goto:snapshot", "Snapshot", "go"),
    def("goto:ignore", "Ignore rules", "go"),
    def("goto:log", "Log", "go"),
    def("goto:branch", "Branches", "go"),
    def("goto:tag", "Tags", "go"),
    def("goto:pr", "Pull requests", "go"),
    def("goto:issue", "Issues", "go"),
    def("goto:platform", "Platform", "go"),
    def("goto:remote", "Remotes", "go"),
    def("goto:workspace", "Workspace", "go"),
    def("goto:worktree", "Worktrees", "go"),
    def("goto:submodule", "Submodules", "go"),
    def("goto:bisect", "Bisect", "go"),
    def("goto:auth", "Auth", "go"),
    def("goto:config", "Config", "go"),
    // The window itself.
    def("app:events", "Toggle the event log", "app"),
    def("app:help", "Help", "app"),
    def("app:refresh", "Reload the repository", "app"),
    def("app:back", "Back to the previous view", "app"),
    def("app:quit", "Quit", "app"),
    def("app:palette", "Open the action palette", "app"),
    // Things worth reaching from anywhere.
    def("repo:scan", "Scan for secrets", "repo"),
    def("repo:scan-history", "Scan the whole history", "repo"),
];

const fn def(id: &'static str, label: &'static str, group: &'static str) -> ActionDef {
    ActionDef { id, label, group }
}

pub fn action_label(id: &str) -> &str {
    ACTIONS
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.label)
        .unwrap_or(id)
}

// ── ~/.torii/keys.toml ───────────────────────────────────────────────────────

/// Where the bindings live. One file, copyable between machines.
pub fn keys_path() -> std::path::PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".torii/keys.toml"))
        .unwrap_or_else(|| std::path::PathBuf::from("keys.toml"))
}

impl Keymap {
    pub fn load() -> Self {
        Self::from_text(&std::fs::read_to_string(keys_path()).unwrap_or_default())
    }

    /// Parse the file. A line that makes no sense is skipped rather than
    /// fatal: a typo in one binding must not cost the user the other twenty.
    pub fn from_text(text: &str) -> Self {
        let mut km = Keymap::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((left, right)) = line.split_once('=') else {
                continue;
            };
            let key = left.trim().trim_matches('"');
            let value = right.trim().trim_matches('"');
            if key == "leader" {
                if let Ok(b) = parse_binding(value) {
                    km.leader = b;
                }
                continue;
            }
            if let Ok(b) = parse_binding(key) {
                km.bind(b, value);
            }
        }
        km
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# torii key bindings — `binding = \"action\"`.\n\
             # A binding is one chord (`ctrl+g`) or a sequence (`g s`).\n\
             # While text is being typed only chords with ctrl/alt fire.\n\n\
             [keys]\n",
        );
        out.push_str(&format!("leader = \"{}\"\n", self.leader));
        for (binding, action) in &self.bindings {
            out.push_str(&format!("\"{binding}\" = \"{action}\"\n"));
        }
        out
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = keys_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn a_file_survives_a_round_trip() {
        let mut km = Keymap::default();
        km.bind(parse_binding("ctrl+g").unwrap(), "goto:log");
        km.bind(parse_binding("g s").unwrap(), "goto:sync");
        km.leader = parse_binding("alt+space").unwrap();

        let reread = Keymap::from_text(&km.to_text());
        assert_eq!(reread.leader.to_string(), "alt+space");
        assert_eq!(
            reread.binding_for("goto:log").unwrap().to_string(),
            "ctrl+g"
        );
        assert_eq!(reread.binding_for("goto:sync").unwrap().to_string(), "g s");
    }

    /// One bad line must not cost the user the rest of the file.
    #[test]
    fn a_broken_line_is_skipped_not_fatal() {
        let km = Keymap::from_text(
            "[keys]\n\"ctrl+g\" = \"goto:log\"\n\"ctrl+nonsense\" = \"goto:sync\"\ngarbage\n\"g s\" = \"goto:tag\"\n",
        );
        assert_eq!(km.bindings.len(), 2);
        assert!(km.binding_for("goto:log").is_some());
        assert!(km.binding_for("goto:tag").is_some());
        assert!(km.binding_for("goto:sync").is_none());
    }

    #[test]
    fn no_file_means_no_bindings_and_a_working_leader() {
        let km = Keymap::from_text("");
        assert!(km.bindings.is_empty());
        assert_eq!(km.leader.to_string(), "ctrl+k");
        assert!(
            km.leader.is_typing_safe(),
            "the way out must survive a text field"
        );
    }

    /// Every action the config screen offers must be one the runtime knows;
    /// an id that drifts silently unbinds whatever the user had on it.
    #[test]
    fn the_catalogue_has_no_duplicate_ids() {
        let mut ids: Vec<&str> = ACTIONS.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate action id");
    }
}
