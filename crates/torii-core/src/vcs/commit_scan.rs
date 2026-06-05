//! Commit policy enforcement.
//!
//! Loads a `policies/commits.toml` file and evaluates each commit in a range
//! against the rules. Used by:
//!
//!   - `torii scan commits [--all] [--since N]` (CLI)
//!   - pre-save hook (planned)
//!   - CI gates (future, server-side)
//!
//! Schema (all keys optional):
//!
//!   forbid_trailers     = ["regex", …]
//!   require_trailers    = ["regex", …]
//!   forbid_subjects     = ["regex", …]
//!   author_email_matches = "regex"
//!   subject_max_length  = 72
//!   subject_min_length  = 8
//!   require_conventional = true
//!
//! All regexes are case-insensitive by default.

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Deserialize;

use crate::error::{Result, ToriiError};

/// Raw TOML shape — directly deserialised from `policies/commits.toml`.
#[derive(Debug, Default, Deserialize)]
struct RawPolicy {
    #[serde(default)]
    forbid_trailers: Vec<String>,
    #[serde(default)]
    require_trailers: Vec<String>,
    #[serde(default)]
    forbid_subjects: Vec<String>,
    #[serde(default)]
    author_email_matches: Option<String>,
    #[serde(default)]
    subject_max_length: Option<usize>,
    #[serde(default)]
    subject_min_length: Option<usize>,
    #[serde(default)]
    require_conventional: bool,
}

/// Compiled, ready-to-run policy.
pub struct CompiledCommitPolicy {
    forbid_trailers: Vec<Regex>,
    require_trailers: Vec<Regex>,
    forbid_subjects: Vec<Regex>,
    author_email_matches: Option<Regex>,
    subject_max_length: Option<usize>,
    subject_min_length: Option<usize>,
    require_conventional: bool,
}

/// One rule failure for one commit.
#[derive(Debug, Clone)]
pub struct Violation {
    /// Full commit OID — kept for future `--fix` mode that needs to
    /// rewrite the exact commit.
    #[allow(dead_code)]
    pub commit_id: String,
    pub commit_short: String,
    pub subject: String,
    pub rule: String,
    pub detail: String,
}

impl CompiledCommitPolicy {
    pub fn from_toml(src: &str) -> Result<Self> {
        let raw: RawPolicy = toml::from_str(src)
            .map_err(|e| ToriiError::InvalidConfig(format!("parse policy TOML: {}", e)))?;
        let mut p = CompiledCommitPolicy {
            forbid_trailers: Vec::new(),
            require_trailers: Vec::new(),
            forbid_subjects: Vec::new(),
            author_email_matches: None,
            subject_max_length: raw.subject_max_length,
            subject_min_length: raw.subject_min_length,
            require_conventional: raw.require_conventional,
        };
        for pat in &raw.forbid_trailers {
            p.forbid_trailers.push(compile(pat)?);
        }
        for pat in &raw.require_trailers {
            p.require_trailers.push(compile(pat)?);
        }
        for pat in &raw.forbid_subjects {
            p.forbid_subjects.push(compile(pat)?);
        }
        if let Some(pat) = &raw.author_email_matches {
            p.author_email_matches = Some(compile(pat)?);
        }
        Ok(p)
    }

    /// Load + compile a policy file. Returns `Ok(None)` if file does not
    /// exist (callers can decide whether absent policy is silent or an error).
    pub fn load(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let src = std::fs::read_to_string(path)
            .map_err(|e| ToriiError::Fs(format!("read {}: {}", path.display(), e)))?;
        Ok(Some(Self::from_toml(&src)?))
    }

    /// Evaluate a commit. Returns 0+ violations.
    pub fn check(
        &self,
        commit_id: &str,
        author_email: &str,
        message: &str,
    ) -> Vec<Violation> {
        let short: String = commit_id.chars().take(7).collect();
        let subject = message.lines().next().unwrap_or("").trim().to_string();
        let mut out = Vec::new();

        let push = |out: &mut Vec<Violation>, rule: &str, detail: String| {
            out.push(Violation {
                commit_id: commit_id.to_string(),
                commit_short: short.clone(),
                subject: subject.clone(),
                rule: rule.to_string(),
                detail,
            });
        };

        for re in &self.forbid_trailers {
            for line in message.lines() {
                if re.is_match(line) {
                    push(
                        &mut out,
                        "forbid_trailers",
                        format!("matches /{}/: `{}`", re.as_str(), line.trim()),
                    );
                    break; // one match per rule per commit is enough
                }
            }
        }

        for re in &self.require_trailers {
            let found = message.lines().any(|l| re.is_match(l));
            if !found {
                push(
                    &mut out,
                    "require_trailers",
                    format!("no line matches /{}/", re.as_str()),
                );
            }
        }

        for re in &self.forbid_subjects {
            if re.is_match(&subject) {
                push(
                    &mut out,
                    "forbid_subjects",
                    format!("subject matches /{}/", re.as_str()),
                );
            }
        }

        if let Some(re) = &self.author_email_matches {
            if !re.is_match(author_email) {
                push(
                    &mut out,
                    "author_email_matches",
                    format!("`{}` doesn't match /{}/", author_email, re.as_str()),
                );
            }
        }

        if let Some(max) = self.subject_max_length {
            let len = subject.chars().count();
            if len > max {
                push(
                    &mut out,
                    "subject_max_length",
                    format!("subject is {} chars (max {})", len, max),
                );
            }
        }
        if let Some(min) = self.subject_min_length {
            let len = subject.chars().count();
            if len < min {
                push(
                    &mut out,
                    "subject_min_length",
                    format!("subject is {} chars (min {})", len, min),
                );
            }
        }

        if self.require_conventional && !is_conventional(&subject) {
            push(
                &mut out,
                "require_conventional",
                "subject doesn't match `<type>(scope?): description`".to_string(),
            );
        }

        out
    }
}

fn compile(pat: &str) -> Result<Regex> {
    // Default to case-insensitive — trailers / authors are usually compared
    // without caring about case.
    let with_flag = format!("(?i){}", pat);
    Regex::new(&with_flag)
        .map_err(|e| ToriiError::InvalidConfig(format!("bad regex /{}/: {}", pat, e)))
}

/// Conventional Commits subject:
///   feat: ...
///   feat(scope): ...
///   fix!: ...   (breaking)
///   chore(release)!: ...
fn is_conventional(subject: &str) -> bool {
    static TYPES: &[&str] = &[
        "feat", "fix", "docs", "style", "refactor", "perf", "test",
        "build", "ci", "chore", "revert",
    ];
    let Some(colon) = subject.find(':') else { return false };
    let head = &subject[..colon];
    let head = head.strip_suffix('!').unwrap_or(head);
    let (ty, _scope) = match head.find('(') {
        Some(open) => {
            let close = head.rfind(')').unwrap_or(open);
            (&head[..open], Some(&head[open + 1..close]))
        }
        None => (head, None),
    };
    TYPES.contains(&ty)
}

/// Default location of the commit policy file inside a repo.
pub fn default_policy_path(repo_root: &Path) -> PathBuf {
    repo_root.join("policies").join("commits.toml")
}

/// Convenience: scan a range of commits with a loaded policy.
/// `since_oid` = inclusive end of range (older). If None, walks all of HEAD.
pub fn scan_repo(
    repo: &git2::Repository,
    policy: &CompiledCommitPolicy,
    limit: usize,
) -> Result<Vec<Violation>> {
    let mut walk = repo.revwalk().map_err(ToriiError::Git)?;
    walk.push_head().map_err(ToriiError::Git)?;
    let mut all = Vec::new();
    for oid in walk.take(limit) {
        let oid = oid.map_err(ToriiError::Git)?;
        let commit = repo.find_commit(oid).map_err(ToriiError::Git)?;
        let id = oid.to_string();
        let email = commit.author().email().unwrap_or("").to_string();
        let msg = commit.message().unwrap_or("").to_string();
        all.extend(policy.check(&id, &email, &msg));
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pol(src: &str) -> CompiledCommitPolicy {
        CompiledCommitPolicy::from_toml(src).unwrap()
    }

    #[test]
    fn forbid_trailer_catches_claude() {
        let p = pol(r#"forbid_trailers = ["Co-Authored-By:.*Claude"]"#);
        let v = p.check(
            "abc123",
            "x@y",
            "feat: stuff\n\nCo-Authored-By: Claude Sonnet <noreply@anthropic.com>",
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "forbid_trailers");
    }

    #[test]
    fn require_trailer_missing() {
        let p = pol(r#"require_trailers = ["Signed-off-by:"]"#);
        let v = p.check("abc", "x@y", "feat: stuff");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "require_trailers");
    }

    #[test]
    fn require_trailer_present_no_violation() {
        let p = pol(r#"require_trailers = ["Signed-off-by:"]"#);
        let v = p.check("abc", "x@y", "feat: stuff\n\nSigned-off-by: A B <a@b>");
        assert!(v.is_empty());
    }

    #[test]
    fn subject_length_limits() {
        let p = pol("subject_max_length = 10\nsubject_min_length = 5");
        assert_eq!(p.check("a", "x@y", "ok done").len(), 0);
        assert_eq!(p.check("a", "x@y", "x").len(), 1); // too short
        assert_eq!(p.check("a", "x@y", "way too long subject here").len(), 1); // too long
    }

    #[test]
    fn forbid_subject() {
        let p = pol(r#"forbid_subjects = ["^(wip|tmp)$"]"#);
        assert_eq!(p.check("a", "x@y", "wip").len(), 1);
        assert_eq!(p.check("a", "x@y", "feat: real").len(), 0);
    }

    #[test]
    fn author_email_mismatch() {
        let p = pol(r#"author_email_matches = ".*@paski\\.dev$""#);
        assert_eq!(p.check("a", "x@y.com", "feat: x").len(), 1);
        assert_eq!(p.check("a", "me@paski.dev", "feat: x").len(), 0);
    }

    #[test]
    fn conventional_commits() {
        let p = pol("require_conventional = true");
        assert_eq!(p.check("a", "x@y", "feat: ok").len(), 0);
        assert_eq!(p.check("a", "x@y", "feat(scope): ok").len(), 0);
        assert_eq!(p.check("a", "x@y", "fix!: breaking").len(), 0);
        assert_eq!(p.check("a", "x@y", "random message").len(), 1);
        assert_eq!(p.check("a", "x@y", "wibble: unknown type").len(), 1);
    }

    #[test]
    fn is_conventional_helper() {
        assert!(is_conventional("feat: x"));
        assert!(is_conventional("feat(scope): x"));
        assert!(is_conventional("fix!: x"));
        assert!(is_conventional("chore(release)!: x"));
        assert!(!is_conventional("random"));
        assert!(!is_conventional("frob: x"));
    }

    #[test]
    fn empty_policy_is_valid() {
        let p = pol("");
        assert!(p.check("a", "x@y", "anything").is_empty());
    }

    #[test]
    fn comments_and_unknown_keys_ok() {
        let p = pol("# comment\nrequire_conventional = true");
        assert_eq!(p.check("a", "x@y", "wibble").len(), 1);
    }
}
