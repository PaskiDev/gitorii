//! The `.toriignore` pair, read as editable rules rather than as effect.
//!
//! `ToriIgnore::load` merges `.toriignore` with `.toriignore.local` and hands
//! back what the rules *do* — which is what the scanner needs and exactly what
//! an editor cannot use: once merged, a rule no longer knows which file it
//! came from, so it cannot be shown as public or private, and it cannot be
//! removed.
//!
//! This reads the two files line by line and keeps the provenance, so a rule
//! carries its file and its line number. Adding and removing then means
//! writing one line, in one file, that the user can see.
//!
//! The two files are not interchangeable: `.toriignore` is committed and
//! public, `.toriignore.local` is gitignored and private. A secret *pattern*
//! is a description of what your secrets look like, so it belongs in the
//! private file by default — the same call `torii ignore secret` makes.

use crate::error::{Result, ToriiError};
use std::fmt;
use std::path::{Path, PathBuf};

/// Which of the two files a rule lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `.toriignore` — committed, visible to anyone with the repo.
    Public,
    /// `.toriignore.local` — gitignored, never leaves this machine.
    Local,
}

impl Origin {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Public => ".toriignore",
            Self::Local => ".toriignore.local",
        }
    }

    pub fn path(self, root: &Path) -> PathBuf {
        root.join(self.file_name())
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Local => "local",
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which section of the file a rule sits under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Path,
    Secret,
    Size,
    Hook,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Secret => "secret",
            Self::Size => "size",
            Self::Hook => "hook",
        }
    }
}

/// One line of one file, with everything needed to show it and to remove it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub origin: Origin,
    pub kind: Kind,
    /// 1-based, so it matches what an editor would say.
    pub line_no: usize,
    /// The line as written, comment and all.
    pub raw: String,
    /// The pattern itself: the path glob, or the regex behind `deny:`.
    pub pattern: String,
    /// The trailing `# name` of a secret rule, when it has one.
    pub name: Option<String>,
}

/// Every rule in both files, public first, in file order.
pub fn load(root: &Path) -> Result<Vec<Rule>> {
    let mut rules = Vec::new();
    for origin in [Origin::Public, Origin::Local] {
        let path = origin.path(root);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // a missing file is simply no rules
        };
        rules.extend(parse(&text, origin));
    }
    Ok(rules)
}

/// Split one file into rules. Comments and blank lines carry no rule, and a
/// section header changes what the lines under it mean.
fn parse(text: &str, origin: Origin) -> Vec<Rule> {
    let mut out = Vec::new();
    let mut kind = Kind::Path;

    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            kind = match &line[1..line.len() - 1] {
                "secrets" => Kind::Secret,
                "size" => Kind::Size,
                "hooks" => Kind::Hook,
                _ => Kind::Path,
            };
            continue;
        }

        let (pattern, name) = match kind {
            Kind::Secret => {
                let body = line.strip_prefix("deny:").unwrap_or(line).trim();
                match body.split_once('#') {
                    Some((p, n)) => (p.trim().to_string(), Some(n.trim().to_string())),
                    None => (body.to_string(), None),
                }
            }
            _ => (line.to_string(), None),
        };

        out.push(Rule {
            origin,
            kind,
            line_no: i + 1,
            raw: raw.to_string(),
            pattern,
            name,
        });
    }
    out
}

/// Append a path pattern to one of the two files.
pub fn add_path(root: &Path, pattern: &str, origin: Origin) -> Result<()> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err(ToriiError::Usage("a path rule needs a pattern".into()));
    }
    append_line(&origin.path(root), None, pattern)
}

/// Append a secret rule. The regex is compiled first: an invalid one written
/// to the file would make every later scan fail to load its own rules.
pub fn add_secret(root: &Path, pattern: &str, name: Option<&str>, origin: Origin) -> Result<()> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err(ToriiError::Usage("a secret rule needs a pattern".into()));
    }
    regex::Regex::new(pattern).map_err(|e| ToriiError::Usage(format!("invalid regex: {e}")))?;
    let line = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!("deny: {pattern}  # {n}"),
        None => format!("deny: {pattern}"),
    };
    append_line(&origin.path(root), Some("secrets"), &line)
}

/// Write `key: value` under `[section]`, replacing the line that key already
/// has rather than adding a second one.
///
/// A file with two `max:` lines is not an error the parser reports — it takes
/// the last and the user is left wondering which one is in force. Keys that
/// hold a list (`pre-save`, `exclude`) are appended to instead, since more
/// than one of those is the point.
pub fn set_setting(
    root: &Path,
    section: &str,
    key: &str,
    value: &str,
    origin: Origin,
    multi: bool,
) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ToriiError::Usage(format!("`{key}` needs a value")));
    }
    let line = format!("{key}: {value}");
    let path = origin.path(root);

    if !multi {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(idx) = setting_line(&text, section, key) {
                let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
                lines[idx] = line;
                let mut out = lines.join("\n");
                out.push('\n');
                return std::fs::write(&path, out)
                    .map_err(|e| ToriiError::Fs(format!("{}: {e}", path.display())));
            }
        }
    }
    append_line(&path, Some(section), &line)
}

/// Drop the line a key holds under `[section]`, if it has one.
pub fn unset_setting(root: &Path, section: &str, key: &str, origin: Origin) -> Result<()> {
    let path = origin.path(root);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(()); // nothing written means nothing to unset
    };
    let Some(idx) = setting_line(&text, section, key) else {
        return Ok(());
    };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.remove(idx);
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| ToriiError::Fs(format!("{}: {e}", path.display())))
}

/// The index of the line where `key` is set under `[section]`, if any.
fn setting_line(text: &str, section: &str, key: &str) -> Option<usize> {
    let mut current = "paths";
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = &line[1..line.len() - 1];
            continue;
        }
        if current == section {
            if let Some((k, _)) = line.split_once(':') {
                if k.trim() == key {
                    return Some(i);
                }
            }
        }
    }
    None
}

/// Remove exactly the line a rule came from.
///
/// The line is matched by content as well as by number: the file may have been
/// edited since it was read, and dropping the wrong line of a file that
/// governs a secret scanner is not a mistake worth risking.
pub fn remove(root: &Path, rule: &Rule) -> Result<()> {
    let path = rule.origin.path(root);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ToriiError::Fs(format!("{}: {e}", path.display())))?;

    let mut lines: Vec<&str> = text.lines().collect();
    let idx = rule.line_no.saturating_sub(1);
    if lines.get(idx).map(|l| *l != rule.raw).unwrap_or(true) {
        return Err(ToriiError::RepoState(format!(
            "{} changed since it was read — reopen the view and try again",
            rule.origin.file_name()
        )));
    }
    lines.remove(idx);

    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| ToriiError::Fs(format!("{}: {e}", path.display())))
}

/// Append `line`, under `section` when one is named, creating the header the
/// first time. Mirrors what `torii ignore` writes, so both produce the same
/// file.
fn append_line(path: &Path, section: Option<&str>, line: &str) -> Result<()> {
    use std::io::Write;

    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| ToriiError::Fs(format!("{}: {e}", path.display())))?;

    let write = |out: &mut std::fs::File| -> std::io::Result<()> {
        if let Some(section) = section {
            let header = format!("[{section}]");
            let has_header = existing.lines().any(|l| l.trim() == header);
            if !has_header {
                if !existing.is_empty() && !existing.ends_with('\n') {
                    writeln!(out)?;
                }
                writeln!(out)?;
                writeln!(out, "{header}")?;
            }
        } else if !existing.is_empty() && !existing.ends_with('\n') {
            writeln!(out)?;
        }
        writeln!(out, "{line}")
    };
    write(&mut out).map_err(|e| ToriiError::Fs(format!("{}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn a_rule_knows_which_file_it_came_from() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            ".toriignore",
            "build/\n\n[secrets]\ndeny: AAAA  # Fake\n",
        );
        write(tmp.path(), ".toriignore.local", "internal/billing/\n");

        let rules = load(tmp.path()).unwrap();
        assert_eq!(rules.len(), 3);

        assert_eq!(rules[0].origin, Origin::Public);
        assert_eq!(rules[0].kind, Kind::Path);
        assert_eq!(rules[0].pattern, "build/");
        assert_eq!(rules[0].line_no, 1);

        assert_eq!(rules[1].kind, Kind::Secret);
        assert_eq!(rules[1].pattern, "AAAA");
        assert_eq!(rules[1].name.as_deref(), Some("Fake"));

        assert_eq!(rules[2].origin, Origin::Local);
        assert_eq!(rules[2].pattern, "internal/billing/");
    }

    #[test]
    fn comments_and_blank_lines_are_not_rules() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            ".toriignore",
            "# a comment\n\n   \nbuild/\n# [secrets] commented out\n",
        );
        let rules = load(tmp.path()).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "build/");
        assert_eq!(rules[0].line_no, 4, "the line number is the file's own");
    }

    #[test]
    fn a_secret_rule_defaults_to_the_private_file() {
        let tmp = tempfile::tempdir().unwrap();
        add_secret(tmp.path(), "AKIA[0-9A-Z]{16}", Some("AWS"), Origin::Local).unwrap();

        let local = std::fs::read_to_string(tmp.path().join(".toriignore.local")).unwrap();
        assert!(local.contains("[secrets]"), "{local}");
        assert!(local.contains("deny: AKIA[0-9A-Z]{16}  # AWS"), "{local}");
        assert!(
            !tmp.path().join(".toriignore").exists(),
            "the public file must not be touched"
        );
    }

    #[test]
    fn an_invalid_regex_never_reaches_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let err = add_secret(tmp.path(), "AKIA[0-9", None, Origin::Local).unwrap_err();
        assert!(format!("{err}").contains("invalid regex"), "{err}");
        assert!(!tmp.path().join(".toriignore.local").exists());
    }

    #[test]
    fn the_section_header_is_written_once() {
        let tmp = tempfile::tempdir().unwrap();
        add_secret(tmp.path(), "AAAA", None, Origin::Public).unwrap();
        add_secret(tmp.path(), "BBBB", None, Origin::Public).unwrap();

        let text = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert_eq!(text.matches("[secrets]").count(), 1, "{text}");
        assert!(
            text.contains("deny: AAAA") && text.contains("deny: BBBB"),
            "{text}"
        );
    }

    #[test]
    fn removing_a_rule_takes_its_line_and_leaves_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ".toriignore", "build/\ntarget/\n*.log\n");

        let rules = load(tmp.path()).unwrap();
        let target = rules.iter().find(|r| r.pattern == "target/").unwrap();
        remove(tmp.path(), target).unwrap();

        let text = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert_eq!(text, "build/\n*.log\n");
    }

    #[test]
    fn a_setting_replaces_its_own_line_instead_of_piling_up() {
        let tmp = tempfile::tempdir().unwrap();
        set_setting(tmp.path(), "size", "max", "10MB", Origin::Public, false).unwrap();
        set_setting(tmp.path(), "size", "max", "20MB", Origin::Public, false).unwrap();

        let text = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert_eq!(text.matches("max:").count(), 1, "{text}");
        assert!(text.contains("max: 20MB"), "{text}");
        assert_eq!(text.matches("[size]").count(), 1, "{text}");
    }

    /// A hook is a list: a second one is another line, not a replacement.
    #[test]
    fn a_list_setting_takes_more_than_one_line() {
        let tmp = tempfile::tempdir().unwrap();
        set_setting(
            tmp.path(),
            "hooks",
            "pre-save",
            "cargo fmt --check",
            Origin::Public,
            true,
        )
        .unwrap();
        set_setting(
            tmp.path(),
            "hooks",
            "pre-save",
            "cargo test",
            Origin::Public,
            true,
        )
        .unwrap();

        let text = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert_eq!(text.matches("pre-save:").count(), 2, "{text}");
    }

    #[test]
    fn unsetting_takes_the_line_out_and_leaves_the_section() {
        let tmp = tempfile::tempdir().unwrap();
        set_setting(tmp.path(), "size", "max", "10MB", Origin::Public, false).unwrap();
        set_setting(tmp.path(), "size", "warn", "1MB", Origin::Public, false).unwrap();

        unset_setting(tmp.path(), "size", "max", Origin::Public).unwrap();

        let text = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert!(!text.contains("max:"), "{text}");
        assert!(text.contains("warn: 1MB"), "{text}");
    }

    /// A key of the same name in another section is a different setting.
    #[test]
    fn a_key_is_only_matched_inside_its_own_section() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".toriignore"),
            "[hooks]\nmax: not a size\n\n[size]\nmax: 10MB\n",
        )
        .unwrap();

        set_setting(tmp.path(), "size", "max", "20MB", Origin::Public, false).unwrap();

        let text = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert!(
            text.contains("max: not a size"),
            "the hook line is untouched: {text}"
        );
        assert!(text.contains("max: 20MB"), "{text}");
    }

    #[test]
    fn unsetting_something_never_written_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        unset_setting(tmp.path(), "size", "max", Origin::Public).unwrap();
    }

    /// The file governs a secret scanner. If it moved under us, refuse rather
    /// than delete whatever now sits on that line.
    #[test]
    fn a_stale_rule_refuses_to_delete_the_wrong_line() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ".toriignore", "build/\ntarget/\n");
        let rules = load(tmp.path()).unwrap();
        let stale = rules
            .iter()
            .find(|r| r.pattern == "target/")
            .unwrap()
            .clone();

        // Someone edits the file in the meantime.
        write(tmp.path(), ".toriignore", "build/\nsomething-else/\n");

        let err = remove(tmp.path(), &stale).unwrap_err();
        assert!(
            format!("{err}").contains("changed since it was read"),
            "{err}"
        );
        let text = std::fs::read_to_string(tmp.path().join(".toriignore")).unwrap();
        assert_eq!(text, "build/\nsomething-else/\n", "nothing was removed");
    }
}
