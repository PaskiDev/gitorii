use std::path::Path;
use std::fs;
use std::io::{self, BufRead};
use anyhow::{Result, anyhow};

/// Parsed `.toriignore` — paths + extension sections (secrets/size/hooks).
#[derive(Default)]
#[allow(dead_code)]
pub struct ToriIgnore {
    patterns: Vec<String>,
    pub secrets: Vec<SecretRule>,
    pub size: SizeRules,
    pub hooks: HookRules,
}

#[derive(Clone)]
pub struct SecretRule {
    pub name: String,
    pub regex: regex::Regex,
}

#[derive(Default, Clone)]
pub struct SizeRules {
    /// Hard limit — block save above this (bytes)
    pub max_bytes: Option<u64>,
    /// Soft limit — warn above this (bytes)
    pub warn_bytes: Option<u64>,
    /// Glob patterns excluded from size enforcement
    pub exclude: Vec<String>,
}

#[derive(Default, Clone)]
pub struct HookRules {
    pub pre_save: Vec<String>,
    pub pre_sync: Vec<String>,
    pub post_save: Vec<String>,
    pub post_sync: Vec<String>,
}

#[derive(PartialEq, Eq)]
enum Section {
    Paths,
    Secrets,
    Size,
    Hooks(HookKind),
}

#[derive(PartialEq, Eq, Clone, Copy)]
#[allow(dead_code)]
enum HookKind { PreSave, PreSync, PostSave, PostSync }

impl ToriIgnore {
    /// Load from repository root (returns default if file absent).
    ///
    /// Merges `.toriignore` (committed, public) with `.toriignore.local`
    /// (gitignored, machine-local). `.local` rules take precedence — they
    /// extend `[secrets]`/`[size]`/`[hooks]` and append path patterns.
    /// Use `.local` for sensitive rules whose mere existence would aid
    /// recon if the public repo leaks (paths like `/internal/billing/`,
    /// custom regex for proprietary secret formats, etc).
    pub fn load<P: AsRef<Path>>(repo_path: P) -> Result<Self> {
        let root = repo_path.as_ref();
        let public = root.join(".toriignore");
        let local = root.join(".toriignore.local");

        let mut out = if public.exists() {
            Self::from_file(&public)?
        } else {
            Self::default()
        };

        if local.exists() {
            let local_rules = Self::from_file(&local)?;
            out.merge(local_rules);
        }

        Ok(out)
    }

    /// Append rules from `other` into self. Path patterns concat;
    /// secrets/hooks concat; size limits in `other` override (tighter
    /// local policy wins).
    fn merge(&mut self, other: Self) {
        self.patterns.extend(other.patterns);
        self.secrets.extend(other.secrets);
        self.hooks.pre_save.extend(other.hooks.pre_save);
        self.hooks.pre_sync.extend(other.hooks.pre_sync);
        self.hooks.post_save.extend(other.hooks.post_save);
        self.hooks.post_sync.extend(other.hooks.post_sync);
        self.size.exclude.extend(other.size.exclude);
        if other.size.max_bytes.is_some() { self.size.max_bytes = other.size.max_bytes; }
        if other.size.warn_bytes.is_some() { self.size.warn_bytes = other.size.warn_bytes; }
    }

    /// Default content seeded by `torii init`
    pub fn default_content() -> &'static str {
        "# Torii ignore file — controls what torii tracks and snapshots\n\
         # Syntax extends .gitignore with optional [sections]\n\
         \n\
         # Build output\n\
         /target\n\
         /build\n\
         /dist\n\
         \n\
         # Dependencies\n\
         node_modules/\n\
         .bun/\n\
         \n\
         # Environment & secrets\n\
         .env\n\
         .env.*\n\
         !.env.example\n\
         \n\
         # Torii local config\n\
         .torii/\n\
         \n\
         # OS & editor\n\
         .DS_Store\n\
         Thumbs.db\n\
         *.swp\n\
         *.swo\n\
         *~\n\
         .idea/\n\
         .vscode/\n\
         \n\
         # ─── Custom secret patterns (uncomment to enable) ─────────────────\n\
         # [secrets]\n\
         # deny: AKIA[0-9A-Z]{16}              # AWS access keys\n\
         # deny: ghp_[A-Za-z0-9]{36}           # GitHub PAT\n\
         # deny: xkeysib-[a-z0-9]{64,}         # Brevo API\n\
         \n\
         # ─── File size limits (uncomment to enable) ───────────────────────\n\
         # [size]\n\
         # max: 10MB\n\
         # warn: 1MB\n\
         # exclude: *.psd, *.zip, *.bin\n\
         \n\
         # ─── Pre-save / pre-sync hooks (uncomment to enable) ──────────────\n\
         # [hooks]\n\
         # pre-save: cargo fmt --check\n\
         # pre-save: cargo clippy -- -D warnings\n\
         # pre-sync: cargo test --no-run\n\
         \n\
         # ─── Sensitive rules → use .toriignore.local ──────────────────────\n\
         # Rules whose existence would help an attacker recon (internal\n\
         # paths, proprietary secret formats) belong in .toriignore.local.\n\
         # That file is gitignored automatically — never committed.\n\
         # Same syntax as this file. Local rules merge on top of public ones.\n"
    }

    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = fs::File::open(path)?;
        let reader = io::BufReader::new(file);
        let mut out = Self::default();
        let mut section = Section::Paths;

        for raw in reader.lines() {
            let raw = raw?;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') { continue; }

            // Section header: [name] or [name:variant]
            if line.starts_with('[') && line.ends_with(']') {
                let inner = &line[1..line.len() - 1];
                section = match inner {
                    "secrets" => Section::Secrets,
                    "size" => Section::Size,
                    "hooks" => Section::Hooks(HookKind::PreSave), // sentinel; key picks variant
                    _ => Section::Paths, // unknown section → ignore as path noise
                };
                continue;
            }

            match section {
                Section::Paths => out.patterns.push(line.to_string()),
                Section::Secrets => out.parse_secret(line)?,
                Section::Size => out.parse_size(line)?,
                Section::Hooks(_) => out.parse_hook(line)?,
            }
        }

        Ok(out)
    }

    fn parse_secret(&mut self, line: &str) -> Result<()> {
        // Format: deny: <regex>            (optional `# name` after)
        let body = line.strip_prefix("deny:").map(str::trim);
        let Some(body) = body else { return Ok(()); };
        let (pattern, name) = match body.find('#') {
            Some(i) => (body[..i].trim(), body[i + 1..].trim().to_string()),
            None => (body, format!("Custom rule {}", self.secrets.len() + 1)),
        };
        if pattern.is_empty() { return Ok(()); }
        let regex = regex::Regex::new(pattern)
            .map_err(|e| anyhow!("invalid regex `{}` in [secrets]: {}", pattern, e))?;
        self.secrets.push(SecretRule { name, regex });
        Ok(())
    }

    fn parse_size(&mut self, line: &str) -> Result<()> {
        let (key, val) = match line.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => return Ok(()),
        };
        match key {
            "max" => self.size.max_bytes = Some(parse_size_value(val)?),
            "warn" => self.size.warn_bytes = Some(parse_size_value(val)?),
            "exclude" => self.size.exclude.extend(
                val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
            ),
            _ => {}
        }
        Ok(())
    }

    fn parse_hook(&mut self, line: &str) -> Result<()> {
        let (key, val) = match line.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => return Ok(()),
        };
        if val.is_empty() { return Ok(()); }
        match key {
            "pre-save" => self.hooks.pre_save.push(val.to_string()),
            "pre-sync" => self.hooks.pre_sync.push(val.to_string()),
            "post-save" => self.hooks.post_save.push(val.to_string()),
            "post-sync" => self.hooks.post_sync.push(val.to_string()),
            _ => {}
        }
        Ok(())
    }

    /// Returns true if any path pattern matches (legacy gitignore-style)
    #[allow(dead_code)]
    pub fn is_ignored<P: AsRef<Path>>(&self, path: P) -> bool {
        let s = path.as_ref().to_string_lossy();
        let s = s.trim_start_matches('/');
        for p in &self.patterns {
            let p = p.trim_start_matches('/');
            if matches_pattern(s, p) { return true; }
        }
        false
    }

    #[allow(dead_code)]
    pub fn patterns(&self) -> &[String] { &self.patterns }
}

/// Parse "10MB", "500KB", "1024", "2GB" → bytes
fn parse_size_value(s: &str) -> Result<u64> {
    let s = s.trim();
    let upper = s.to_uppercase();
    let (num_str, mul): (&str, u64) =
        if let Some(rest) = upper.strip_suffix("GB") { (rest, 1024 * 1024 * 1024) }
        else if let Some(rest) = upper.strip_suffix("MB") { (rest, 1024 * 1024) }
        else if let Some(rest) = upper.strip_suffix("KB") { (rest, 1024) }
        else if let Some(rest) = upper.strip_suffix("B") { (rest, 1) }
        else { (upper.as_str(), 1) };
    let num: u64 = num_str.trim().parse()
        .map_err(|_| anyhow!("invalid size value: `{}`", s))?;
    Ok(num.checked_mul(mul).ok_or_else(|| anyhow!("size overflow: {}", s))?)
}

fn matches_pattern(path: &str, pattern: &str) -> bool {
    if path == pattern { return true; }
    if pattern.ends_with('/') {
        let dir = pattern.trim_end_matches('/');
        if path.starts_with(dir) { return true; }
    }
    if pattern.contains('*') { return wildcard_match(path, pattern); }
    if pattern.starts_with("*.") {
        let ext = pattern.trim_start_matches("*.");
        if path.ends_with(&format!(".{}", ext)) { return true; }
    }
    path.contains(pattern)
}

fn wildcard_match(path: &str, pattern: &str) -> bool {
    if pattern.contains("**/") {
        let parts: Vec<&str> = pattern.split("**/").collect();
        if parts.len() == 2 {
            let suffix = parts[1];
            for (i, _) in path.match_indices('/') {
                if simple_glob(&path[i + 1..], suffix) { return true; }
            }
            if simple_glob(path, suffix) { return true; }
        }
    }
    if pattern.starts_with('*') && pattern.ends_with('*') {
        let middle = pattern.trim_matches('*');
        return path.contains(middle);
    }
    if pattern.starts_with('*') {
        return path.ends_with(pattern.trim_start_matches('*'));
    }
    if pattern.ends_with('*') {
        return path.starts_with(pattern.trim_end_matches('*'));
    }
    false
}

fn simple_glob(text: &str, pattern: &str) -> bool {
    if !pattern.contains('*') { return text == pattern; }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        match text[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 { return false; }
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    if let Some(last) = parts.last() {
        if !last.is_empty() { return text.ends_with(last); }
    }
    true
}

/// Match a path against an exclude glob like `*.psd` or `vendor/*`
pub fn glob_match(path: &str, pattern: &str) -> bool {
    matches_pattern(path, pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_str(s: &str) -> ToriIgnore {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".toriignore");
        std::fs::write(&p, s).unwrap();
        ToriIgnore::from_file(&p).unwrap()
    }

    #[test]
    fn parses_paths_only() {
        let t = from_str("target\nnode_modules/\n");
        assert_eq!(t.patterns.len(), 2);
        assert!(t.is_ignored("target"));
        assert!(t.is_ignored("node_modules/x"));
    }

    #[test]
    fn parses_secrets_section() {
        let t = from_str("[secrets]\ndeny: AKIA[0-9A-Z]{16}\ndeny: ghp_[A-Za-z0-9]{36}\n");
        assert_eq!(t.secrets.len(), 2);
        assert!(t.secrets[0].regex.is_match("AKIAIOSFODNN7EXAMPLE"));
        assert!(!t.secrets[0].regex.is_match("not a key"));
    }

    #[test]
    fn parses_secrets_with_name() {
        let t = from_str("[secrets]\ndeny: xkeysib-[a-z0-9]{20,}  # Brevo\n");
        assert_eq!(t.secrets[0].name, "Brevo");
    }

    #[test]
    fn parses_size_section() {
        let t = from_str("[size]\nmax: 10MB\nwarn: 500KB\nexclude: *.psd, *.zip\n");
        assert_eq!(t.size.max_bytes, Some(10 * 1024 * 1024));
        assert_eq!(t.size.warn_bytes, Some(500 * 1024));
        assert_eq!(t.size.exclude.len(), 2);
    }

    #[test]
    fn parses_hooks_section() {
        let t = from_str("[hooks]\npre-save: cargo fmt --check\npre-save: cargo clippy\npre-sync: cargo test\n");
        assert_eq!(t.hooks.pre_save.len(), 2);
        assert_eq!(t.hooks.pre_sync.len(), 1);
        assert_eq!(t.hooks.pre_save[0], "cargo fmt --check");
    }

    #[test]
    fn parse_size_value_units() {
        assert_eq!(parse_size_value("10MB").unwrap(), 10 * 1024 * 1024);
        assert_eq!(parse_size_value("500KB").unwrap(), 500 * 1024);
        assert_eq!(parse_size_value("2GB").unwrap(), 2u64 * 1024 * 1024 * 1024);
        assert_eq!(parse_size_value("1024").unwrap(), 1024);
    }

    #[test]
    fn local_overlay_merges_with_public() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".toriignore"),
            "node_modules/\n[secrets]\ndeny: AKIA[0-9A-Z]{16}\n[size]\nmax: 10MB\n",
        ).unwrap();
        std::fs::write(
            dir.path().join(".toriignore.local"),
            "/internal/billing/\n[secrets]\ndeny: PROP_[a-z]{20}  # Proprietary\n[size]\nmax: 5MB\n",
        ).unwrap();

        let t = ToriIgnore::load(dir.path()).unwrap();
        assert_eq!(t.patterns.len(), 2);
        assert_eq!(t.secrets.len(), 2);
        // Local overrides — tighter limit wins
        assert_eq!(t.size.max_bytes, Some(5 * 1024 * 1024));
    }

    #[test]
    fn local_only_works_without_public() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".toriignore.local"),
            "secret.txt\n[hooks]\npre-save: ./check.sh\n",
        ).unwrap();
        let t = ToriIgnore::load(dir.path()).unwrap();
        assert_eq!(t.patterns.len(), 1);
        assert_eq!(t.hooks.pre_save.len(), 1);
    }

    #[test]
    fn no_files_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let t = ToriIgnore::load(dir.path()).unwrap();
        assert!(t.patterns.is_empty());
        assert!(t.secrets.is_empty());
    }

    #[test]
    fn invalid_secret_regex_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".toriignore");
        std::fs::write(&p, "[secrets]\ndeny: [unclosed\n").unwrap();
        assert!(ToriIgnore::from_file(&p).is_err());
    }
}
