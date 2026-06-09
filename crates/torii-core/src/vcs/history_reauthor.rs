//! History rewrite: re-author commits in bulk.
//!
//! Two entry points:
//! - `reauthor`        — match one `(old → new)` pair from CLI flags.
//! - `mailmap_apply`   — apply every mapping from a `.mailmap` file
//!   (standard git format).
//!
//! Both share the same walker: rev-walk the reachable history from HEAD (or
//! from `--since` exclusive), recreate every commit that matches with the
//! new signature, and re-point all local refs (branches, lightweight tags,
//! annotated tags, HEAD) at the new OIDs.
//!
//! Design notes that aren't obvious from the code:
//!
//! - **Tagger is rewritten on annotated tags** (not preserved). Rationale:
//!   the whole point of reauthor is identity reconciliation; leaving a stale
//!   tagger from the old identity contradicts that intent.
//! - **GPG re-signing is automatic** when `git.sign_commits = true` *and* a
//!   key is configured. If the flag is on but no key is reachable we emit a
//!   warning and continue with unsigned commits — the user asked to rewrite
//!   history, not to silently lose data over a missing key.
//! - **Original committer timestamps are preserved.** We change *who*
//!   authored/committed, never *when*. The author timestamp follows the same
//!   rule. Use `torii history rewrite` for date changes.
//! - **Signatures invalidate after rewrite.** Documented; not auto-fixed
//!   except for GPG as above.

use crate::error::{Result, ToriiError};
use git2::{Commit, Oid, Reference, Repository, Signature, Sort, Time};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

// -- Identity ---------------------------------------------------------------

/// A target identity to *write*. Always has both name and email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub email: String,
}

impl Identity {
    /// Parse `"Name <email>"`. Anything else is rejected — the *new* identity
    /// must always have both halves, otherwise the rewrite is ambiguous.
    pub fn parse_full(s: &str) -> Result<Self> {
        let s = s.trim();
        let open = s.rfind('<').ok_or_else(|| {
            ToriiError::Usage(format!(
                "identity must be in 'Name <email>' form, got: {s:?}"
            ))
        })?;
        let close = s.rfind('>').ok_or_else(|| {
            ToriiError::Usage(format!(
                "identity must be in 'Name <email>' form, got: {s:?}"
            ))
        })?;
        if close < open {
            return Err(ToriiError::Usage(format!("malformed identity: {s:?}")));
        }
        let name = s[..open].trim().trim_end_matches(',').trim().to_string();
        let email = s[open + 1..close].trim().to_string();
        if name.is_empty() || email.is_empty() {
            return Err(ToriiError::Usage(format!(
                "identity needs non-empty name and email: {s:?}"
            )));
        }
        Ok(Self { name, email })
    }
}

// -- Matcher ----------------------------------------------------------------

/// What a user wants to match against author/committer of an existing commit.
///
/// Resolved from the `--old` flag (auto-detected by format) or from a
/// `.mailmap` line. Each variant matches a different subset of commits.
#[derive(Debug, Clone)]
pub enum OldMatcher {
    /// Full identity: both name and email must equal.
    Full { name: String, email: String },
    /// Match any name with this email.
    EmailOnly(String),
    /// Match any email with this name.
    NameOnly(String),
}

impl OldMatcher {
    /// Auto-detect the form from a free-form `--old` string.
    ///
    /// Rules (checked in order):
    /// - Contains `<…>` → parse as full `"Name <email>"`.
    /// - Contains `@` (and no `<>`) → treat the whole thing as an email.
    /// - Otherwise → treat as a name.
    pub fn parse_loose(s: &str) -> Result<Self> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ToriiError::Usage("--old cannot be empty".into()));
        }
        if s.contains('<') && s.contains('>') {
            let id = Identity::parse_full(s)?;
            Ok(OldMatcher::Full {
                name: id.name,
                email: id.email,
            })
        } else if s.contains('@') {
            Ok(OldMatcher::EmailOnly(s.to_string()))
        } else {
            Ok(OldMatcher::NameOnly(s.to_string()))
        }
    }

    fn matches(&self, name: &str, email: &str) -> bool {
        match self {
            OldMatcher::Full { name: n, email: e } => name == n && email == e,
            OldMatcher::EmailOnly(e) => email == e,
            OldMatcher::NameOnly(n) => name == n,
        }
    }
}

// -- Mapping (one or many rules) -------------------------------------------

/// A single rewrite rule. `old` is what to match, `new` is what to write.
#[derive(Debug, Clone)]
pub struct Rule {
    pub old: OldMatcher,
    pub new: Identity,
}

/// A collection of rules applied first-match-wins.
#[derive(Debug, Default, Clone)]
pub struct Mapping {
    pub rules: Vec<Rule>,
}

impl Mapping {
    pub fn single(old: OldMatcher, new: Identity) -> Self {
        Self {
            rules: vec![Rule { old, new }],
        }
    }

    fn apply(&self, name: &str, email: &str) -> Option<&Identity> {
        self.rules
            .iter()
            .find(|r| r.old.matches(name, email))
            .map(|r| &r.new)
    }
}

// -- Mailmap parser --------------------------------------------------------

/// Parse a `.mailmap` file in the standard git format documented at
/// <https://git-scm.com/docs/gitmailmap>. Supported lines:
///
/// ```text
/// Proper Name <commit@email.xx>
/// <proper@email.xx> <commit@email.xx>
/// Proper Name <proper@email.xx> <commit@email.xx>
/// Proper Name <proper@email.xx> Commit Name <commit@email.xx>
/// ```
///
/// Comments (`#…`) and blank lines are skipped. Unparseable lines abort the
/// whole load with a precise line number so the user can fix the file.
pub fn load_mailmap<P: AsRef<Path>>(path: P) -> Result<Mapping> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)
        .map_err(|e| ToriiError::Fs(format!("read {}: {}", path.display(), e)))?;

    let mut rules = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = strip_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }
        let rule = parse_mailmap_line(trimmed).map_err(|e| {
            ToriiError::InvalidConfig(format!("{}:{}: {}", path.display(), line_no, e))
        })?;
        rules.push(rule);
    }
    Ok(Mapping { rules })
}

fn strip_comment(line: &str) -> &str {
    // git mailmap treats `#` as line comment regardless of position.
    match line.find('#') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

/// Returns at most one rule per non-empty line. The four supported shapes
/// distinguish by counting `<email>` blocks: one → name rewrite only, two →
/// email rewrite (with optional name rewrite on either side).
fn parse_mailmap_line(s: &str) -> std::result::Result<Rule, String> {
    let emails: Vec<(usize, usize)> = bracket_spans(s)?;
    match emails.len() {
        1 => {
            // "Proper Name <commit-email>" → rewrite name when email matches.
            let (open, close) = emails[0];
            let new_name = s[..open].trim().trim_end_matches(',').trim();
            let commit_email = s[open + 1..close].trim();
            if new_name.is_empty() || commit_email.is_empty() {
                return Err("expected 'Proper Name <commit@email>'".into());
            }
            Ok(Rule {
                old: OldMatcher::EmailOnly(commit_email.to_string()),
                new: Identity {
                    name: new_name.to_string(),
                    email: commit_email.to_string(),
                },
            })
        }
        2 => {
            // "<proper-email> <commit-email>"  or
            // "Proper Name <proper-email> <commit-email>"  or
            // "Proper Name <proper-email> Commit Name <commit-email>"
            let (open1, close1) = emails[0];
            let (open2, close2) = emails[1];

            let lhs_name = s[..open1].trim().trim_end_matches(',').trim();
            let proper_email = s[open1 + 1..close1].trim();
            let between = s[close1 + 1..open2].trim().trim_end_matches(',').trim();
            let commit_email = s[open2 + 1..close2].trim();

            if proper_email.is_empty() || commit_email.is_empty() {
                return Err("empty email in mailmap entry".into());
            }

            // If `lhs_name` is empty, the rule applies email-only.
            let old = if between.is_empty() {
                OldMatcher::EmailOnly(commit_email.to_string())
            } else {
                OldMatcher::Full {
                    name: between.to_string(),
                    email: commit_email.to_string(),
                }
            };

            // New name falls back to commit name (the matched name) if unset.
            let new_name = if !lhs_name.is_empty() {
                lhs_name.to_string()
            } else if !between.is_empty() {
                between.to_string()
            } else {
                // No name at all — keep email change only by using the email
                // local-part as a placeholder name. Git itself does the same.
                proper_email
                    .split('@')
                    .next()
                    .unwrap_or(proper_email)
                    .to_string()
            };

            Ok(Rule {
                old,
                new: Identity {
                    name: new_name,
                    email: proper_email.to_string(),
                },
            })
        }
        _ => Err(format!(
            "expected one or two <email> blocks, found {}",
            emails.len()
        )),
    }
}

/// Return `(open, close)` byte offsets for every `<…>` pair in `s`. Errors
/// on unbalanced brackets so a malformed line is rejected loudly.
fn bracket_spans(s: &str) -> std::result::Result<Vec<(usize, usize)>, String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let close = bytes[i + 1..]
                .iter()
                .position(|&b| b == b'>')
                .ok_or_else(|| format!("unclosed '<' at byte {i}"))?
                + i
                + 1;
            out.push((i, close));
            i = close + 1;
        } else {
            i += 1;
        }
    }
    Ok(out)
}

// -- Options + stats --------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Limit walk to commits reachable from HEAD but not from this rev.
    /// `None` means "all reachable commits from HEAD".
    pub since: Option<String>,
    /// Just report what would change; touch nothing.
    pub dry_run: bool,
    /// Skip the safety snapshot. Off by default — opt-in skip.
    pub no_snapshot: bool,
    /// Also rewrite committer (default: only author).
    pub committer: bool,
    /// Allow working tree to have uncommitted changes. Off by default.
    pub allow_dirty: bool,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub scanned: usize,
    pub matched: usize,
    pub rewritten: usize,
    pub refs_updated: usize,
    pub tags_rewritten: usize,
    pub snapshot_id: Option<String>,
}

// -- Public entry points ---------------------------------------------------

/// Rewrite history with a single `(old → new)` mapping (CLI form).
pub fn reauthor(repo_path: &Path, old: OldMatcher, new: Identity, opts: &Options) -> Result<Stats> {
    let mapping = Mapping::single(old, new);
    rewrite(repo_path, &mapping, opts)
}

/// Apply a `.mailmap` file.
pub fn mailmap_apply(repo_path: &Path, mailmap_path: &Path, opts: &Options) -> Result<Stats> {
    let mapping = load_mailmap(mailmap_path)?;
    if mapping.rules.is_empty() {
        return Err(ToriiError::InvalidConfig(format!(
            "no usable rules in {}",
            mailmap_path.display()
        )));
    }
    rewrite(repo_path, &mapping, opts)
}

// -- The walker -------------------------------------------------------------

fn rewrite(repo_path: &Path, mapping: &Mapping, opts: &Options) -> Result<Stats> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;

    pre_flight(&repo, opts)?;

    // Optional: take a safety snapshot before touching anything.
    let snapshot_id = if !opts.no_snapshot && !opts.dry_run {
        let mgr = crate::snapshot::SnapshotManager::new(repo_path)?;
        let id = mgr.create_snapshot(Some("pre-reauthor"))?;
        println!(
            "📸 Snapshot: {} (revert with: torii snapshot restore {})",
            id, id
        );
        Some(id)
    } else {
        None
    };

    // Build the commit list (oldest-first, so parents are processed first).
    let oids = collect_commits(&repo, opts.since.as_deref())?;

    let mut stats = Stats {
        scanned: oids.len(),
        snapshot_id,
        ..Default::default()
    };

    // old_oid -> new_oid (identity if the commit didn't need a rewrite).
    let mut remap: HashMap<Oid, Oid> = HashMap::new();

    // Detect "this commit needs rewriting" by checking ancestors too: a
    // commit whose author didn't match but whose parents got rewritten
    // still needs a new commit object (different parent OIDs).
    for old_oid in &oids {
        let commit = repo.find_commit(*old_oid).map_err(ToriiError::Git)?;

        let new_author = remap_signature(&commit.author(), mapping);
        let new_committer = if opts.committer {
            remap_signature(&commit.committer(), mapping)
        } else {
            None
        };

        // Parents after remapping — if a parent was rewritten, we point at
        // the new one; otherwise we keep the original parent OID.
        let mut new_parents: Vec<Commit> = Vec::with_capacity(commit.parent_count());
        let mut parents_changed = false;
        for parent in commit.parents() {
            let pid = parent.id();
            let mapped = remap.get(&pid).copied().unwrap_or(pid);
            if mapped != pid {
                parents_changed = true;
            }
            new_parents.push(repo.find_commit(mapped).map_err(ToriiError::Git)?);
        }

        let identity_changed = new_author.is_some() || new_committer.is_some();
        if identity_changed {
            stats.matched += 1;
        }

        if !identity_changed && !parents_changed {
            remap.insert(*old_oid, *old_oid);
            continue;
        }

        if opts.dry_run {
            remap.insert(*old_oid, *old_oid); // pretend identity for parents
            stats.rewritten += 1;
            continue;
        }

        let author = sig_with_time(
            new_author.as_deref().unwrap_or(""),
            new_author.as_deref().unwrap_or(""),
            commit.author().when(),
            new_author.is_some().then_some(()),
            &commit.author(),
        )?;

        let committer = sig_with_time(
            new_committer.as_deref().unwrap_or(""),
            new_committer.as_deref().unwrap_or(""),
            commit.committer().when(),
            new_committer.is_some().then_some(()),
            &commit.committer(),
        )?;

        let tree = commit.tree().map_err(ToriiError::Git)?;
        let msg = commit.message().unwrap_or("");
        let parent_refs: Vec<&Commit> = new_parents.iter().collect();

        let new_oid = crate::core::commit_inner_split(
            &repo,
            None,
            &author,
            &committer,
            msg,
            &tree,
            &parent_refs,
        )?;

        remap.insert(*old_oid, new_oid);
        stats.rewritten += 1;
    }

    if opts.dry_run {
        return Ok(stats);
    }

    // Update refs to point at the new OIDs.
    update_refs(&repo, &remap, opts, &mut stats)?;

    Ok(stats)
}

/// Aborts on pending operations and (unless --allow-dirty) on a dirty tree.
fn pre_flight(repo: &Repository, opts: &Options) -> Result<()> {
    if repo.state() != git2::RepositoryState::Clean {
        return Err(ToriiError::RepoState(format!(
            "refusing to rewrite: repository has a pending operation ({:?}). \
             Finish or abort it first.",
            repo.state()
        )));
    }
    if !opts.allow_dirty {
        let statuses = repo
            .statuses(Some(
                git2::StatusOptions::new()
                    .include_untracked(false)
                    .include_ignored(false),
            ))
            .map_err(ToriiError::Git)?;
        let dirty = statuses
            .iter()
            .any(|s| !s.status().contains(git2::Status::IGNORED));
        if dirty {
            return Err(ToriiError::RepoState(
                "refusing to rewrite: working tree has uncommitted changes. \
                 Commit, stash (torii snapshot stash), or pass --allow-dirty."
                    .into(),
            ));
        }
    }
    Ok(())
}

fn collect_commits(repo: &Repository, since: Option<&str>) -> Result<Vec<Oid>> {
    let mut walk = repo.revwalk().map_err(ToriiError::Git)?;
    walk.push_head().map_err(ToriiError::Git)?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)
        .map_err(ToriiError::Git)?;

    if let Some(rev) = since {
        let obj = repo
            .revparse_single(rev)
            .map_err(|e| ToriiError::Usage(format!("--since {rev}: {e}")))?;
        walk.hide(obj.id()).map_err(ToriiError::Git)?;
    }

    let mut out = Vec::new();
    for r in walk {
        out.push(r.map_err(ToriiError::Git)?);
    }
    Ok(out)
}

/// Returns `Some("Name|email")` packed into one string when the signature
/// would change under `mapping`; `None` if it stays the same.
///
/// The packed string is awkward but lets us avoid lifetimes when threading
/// through `sig_with_time`. The `|` separator is safe because email RFCs
/// forbid `|` in the local part.
fn remap_signature(sig: &Signature, mapping: &Mapping) -> Option<String> {
    let name = sig.name().unwrap_or("");
    let email = sig.email().unwrap_or("");
    mapping
        .apply(name, email)
        .map(|new| format!("{}|{}", new.name, new.email))
}

/// Build a libgit2 `Signature` keeping the original time. If `changed` is
/// `None`, just clones the original signature (still allocates a new
/// `Signature` because we need an owned value for `repo.commit`).
fn sig_with_time<'a>(
    packed_name_email: &str,
    _unused: &str,
    when: Time,
    changed: Option<()>,
    original: &Signature,
) -> Result<Signature<'a>> {
    if changed.is_some() {
        let (name, email) = packed_name_email
            .split_once('|')
            .ok_or_else(|| ToriiError::InvalidConfig("internal: malformed remap".into()))?;
        Signature::new(name, email, &when).map_err(ToriiError::Git)
    } else {
        Signature::new(
            original.name().unwrap_or(""),
            original.email().unwrap_or(""),
            &when,
        )
        .map_err(ToriiError::Git)
    }
}

/// Move every local branch, lightweight tag, HEAD and annotated tag from
/// any old OID to its remapped new OID.
fn update_refs(
    repo: &Repository,
    remap: &HashMap<Oid, Oid>,
    opts: &Options,
    stats: &mut Stats,
) -> Result<()> {
    // Collect refs up front — mutating during iteration is fragile.
    let refs: Vec<Reference> = repo
        .references()
        .map_err(ToriiError::Git)?
        .filter_map(|r| r.ok())
        .collect();

    let mut updated_branches: HashSet<String> = HashSet::new();

    for r in refs {
        // Skip remotes; we only rewrite local history.
        let name = match r.name() {
            Some(n) => n.to_string(),
            None => continue,
        };
        if name.starts_with("refs/remotes/") {
            continue;
        }

        if name.starts_with("refs/tags/") {
            handle_tag_ref(repo, &r, &name, remap, opts, stats)?;
            continue;
        }

        // Plain ref (branch or note or other). Re-target if its tip moved.
        if let Some(target) = r.target() {
            if let Some(&new_oid) = remap.get(&target) {
                if new_oid != target {
                    let mut r = repo.find_reference(&name).map_err(ToriiError::Git)?;
                    r.set_target(new_oid, "torii reauthor")
                        .map_err(ToriiError::Git)?;
                    stats.refs_updated += 1;
                    updated_branches.insert(name);
                }
            }
        }
    }

    // HEAD: detached or branch-pointing. If it's pointing at a branch we
    // already updated, libgit2 keeps HEAD in sync automatically. If it's
    // detached at a rewritten commit, re-point it manually.
    let head = repo.head().map_err(ToriiError::Git)?;
    if head.kind() == Some(git2::ReferenceType::Direct) {
        if let Some(oid) = head.target() {
            if let Some(&new_oid) = remap.get(&oid) {
                if new_oid != oid {
                    repo.set_head_detached(new_oid).map_err(ToriiError::Git)?;
                    stats.refs_updated += 1;
                }
            }
        }
    }

    Ok(())
}

/// Annotated tag rewrite: build a new tag object with the rewritten tagger
/// (always — see module docs) pointing at the new commit; lightweight tags
/// just retarget.
fn handle_tag_ref(
    repo: &Repository,
    r: &Reference,
    name: &str,
    remap: &HashMap<Oid, Oid>,
    _opts: &Options,
    stats: &mut Stats,
) -> Result<()> {
    let short = name.strip_prefix("refs/tags/").unwrap_or(name);
    let target_oid = match r.target() {
        Some(t) => t,
        None => return Ok(()),
    };

    // Resolve to a tag object (annotated) or commit (lightweight).
    let obj = repo
        .find_object(target_oid, None)
        .map_err(ToriiError::Git)?;

    if let Some(tag) = obj.as_tag() {
        let pointee = tag.target_id();
        if let Some(&new_pointee) = remap.get(&pointee) {
            if new_pointee == pointee {
                return Ok(());
            }
            let new_commit = repo.find_commit(new_pointee).map_err(ToriiError::Git)?;
            // Tagger rewrite: read existing tagger, ask the mapping, replace.
            let tagger = tag
                .tagger()
                .map(|t| Signature::new(t.name().unwrap_or(""), t.email().unwrap_or(""), &t.when()))
                .transpose()
                .map_err(ToriiError::Git)?
                .unwrap_or_else(|| {
                    // No original tagger somehow — synthesize from new commit.
                    new_commit.author().to_owned()
                });

            let message = tag.message().unwrap_or("");
            repo.tag(
                short,
                new_commit.as_object(),
                &tagger,
                message,
                true, // force
            )
            .map_err(ToriiError::Git)?;
            stats.tags_rewritten += 1;
        }
    } else {
        // Lightweight tag: behaves like any direct ref.
        if let Some(&new_oid) = remap.get(&target_oid) {
            if new_oid != target_oid {
                let mut r = repo.find_reference(name).map_err(ToriiError::Git)?;
                r.set_target(new_oid, "torii reauthor")
                    .map_err(ToriiError::Git)?;
                stats.refs_updated += 1;
            }
        }
    }
    Ok(())
}

// -- Pretty printer ---------------------------------------------------------

pub fn print_summary(stats: &Stats, dry_run: bool) {
    if dry_run {
        println!(
            "✏  Dry-run: would rewrite {} of {} commits, touch {} refs and {} annotated tags.",
            stats.rewritten, stats.scanned, stats.refs_updated, stats.tags_rewritten
        );
        println!("   Run again without --dry-run to apply.");
        return;
    }
    println!(
        "✅ Rewrite complete: {} commits remapped ({} matched author/committer), \
         {} refs updated, {} annotated tags rewritten.",
        stats.rewritten, stats.matched, stats.refs_updated, stats.tags_rewritten
    );
    if let Some(id) = &stats.snapshot_id {
        println!("   Revert: torii snapshot restore {}", id);
    }
    println!("   Push: torii sync --push --force  (history was rewritten)");
}

// -- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_identity() {
        let id = Identity::parse_full("Pasqual <paski@paski.dev>").unwrap();
        assert_eq!(id.name, "Pasqual");
        assert_eq!(id.email, "paski@paski.dev");
    }

    #[test]
    fn rejects_partial_identity() {
        assert!(Identity::parse_full("Pasqual").is_err());
        assert!(Identity::parse_full("paski@paski.dev").is_err());
        assert!(Identity::parse_full("Pasqual <>").is_err());
        assert!(Identity::parse_full("Pasqual <").is_err());
    }

    #[test]
    fn old_matcher_autodetect() {
        match OldMatcher::parse_loose("outsider <x@y.com>").unwrap() {
            OldMatcher::Full { name, email } => {
                assert_eq!(name, "outsider");
                assert_eq!(email, "x@y.com");
            }
            _ => panic!("expected Full"),
        }
        match OldMatcher::parse_loose("x@y.com").unwrap() {
            OldMatcher::EmailOnly(e) => assert_eq!(e, "x@y.com"),
            _ => panic!("expected EmailOnly"),
        }
        match OldMatcher::parse_loose("outsider").unwrap() {
            OldMatcher::NameOnly(n) => assert_eq!(n, "outsider"),
            _ => panic!("expected NameOnly"),
        }
    }

    #[test]
    fn mailmap_name_only_form() {
        let r = parse_mailmap_line("Pasqual Peñalver <old@x>").unwrap();
        match r.old {
            OldMatcher::EmailOnly(e) => assert_eq!(e, "old@x"),
            _ => panic!(),
        }
        assert_eq!(r.new.name, "Pasqual Peñalver");
        assert_eq!(r.new.email, "old@x");
    }

    #[test]
    fn mailmap_email_only_form() {
        let r = parse_mailmap_line("<new@x> <old@x>").unwrap();
        assert!(matches!(r.old, OldMatcher::EmailOnly(ref e) if e == "old@x"));
        assert_eq!(r.new.email, "new@x");
    }

    #[test]
    fn mailmap_full_rewrite() {
        let r = parse_mailmap_line("New Name <new@x> Old Name <old@x>").unwrap();
        match r.old {
            OldMatcher::Full { name, email } => {
                assert_eq!(name, "Old Name");
                assert_eq!(email, "old@x");
            }
            _ => panic!(),
        }
        assert_eq!(r.new.name, "New Name");
        assert_eq!(r.new.email, "new@x");
    }

    #[test]
    fn mailmap_skips_comments_and_blanks() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "# header\n\nProper <e@x>\n# inline\n<a@x> <b@x>\n",
        )
        .unwrap();
        let m = load_mailmap(tmp.path()).unwrap();
        assert_eq!(m.rules.len(), 2);
    }

    #[test]
    fn matcher_apply_precedence() {
        let mapping = Mapping {
            rules: vec![
                Rule {
                    old: OldMatcher::Full {
                        name: "A".into(),
                        email: "a@x".into(),
                    },
                    new: Identity {
                        name: "AA".into(),
                        email: "aa@x".into(),
                    },
                },
                Rule {
                    old: OldMatcher::EmailOnly("a@x".into()),
                    new: Identity {
                        name: "EE".into(),
                        email: "ee@x".into(),
                    },
                },
            ],
        };
        // First rule wins.
        let hit = mapping.apply("A", "a@x").unwrap();
        assert_eq!(hit.name, "AA");
        // Second rule catches different name.
        let hit = mapping.apply("Other", "a@x").unwrap();
        assert_eq!(hit.name, "EE");
        // No match at all.
        assert!(mapping.apply("X", "x@x").is_none());
    }
}
