//! History rewrite: rewrite commit **messages** in bulk.
//!
//! The message-equivalent of [`crate::history_reauthor`] (identity) and
//! `torii history rewrite` (dates). Given a set of `(commit → new message)`
//! pairs it rev-walks the history reachable from HEAD, recreates each targeted
//! commit with the new message, and re-points every local ref — **preserving
//! author, committer, all timestamps and tree content exactly**.
//!
//! Why this exists: there was previously no non-interactive way to reword
//! commit messages in torii. `rebase -i` needs an editor, and `save --amend`
//! only reaches the tip. This closes that gap (the `git filter-repo
//! --message-callback` use case) with the same safety rails as `reauthor`
//! (pre-flight checks + automatic snapshot).

use crate::error::{Result, ToriiError};
use git2::{Commit, Oid, Repository, Signature};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use super::history_reauthor::{collect_commits, pre_flight, update_refs, Stats};

#[derive(Debug, Clone, Default)]
pub struct RewordOptions {
    /// Limit the walk to commits reachable from HEAD but not from this rev.
    pub since: Option<String>,
    /// Report what would change; touch nothing.
    pub dry_run: bool,
    /// Skip the safety snapshot (off by default — opt-in skip).
    pub no_snapshot: bool,
    /// Allow a dirty working tree (off by default).
    pub allow_dirty: bool,
}

/// Parse a batch map file: one `<hash> <new single-line message>` per line.
/// Blank lines and `#` comments are skipped. Multi-line bodies aren't
/// expressible here — use `-F <file>` on a single commit for those.
pub fn load_reword_map(path: &Path) -> Result<Vec<(String, String)>> {
    let raw = fs::read_to_string(path)
        .map_err(|e| ToriiError::Fs(format!("read {}: {}", path.display(), e)))?;

    let mut out = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (hash, msg) = trimmed.split_once(char::is_whitespace).ok_or_else(|| {
            ToriiError::InvalidConfig(format!(
                "{}:{}: expected '<hash> <message>'",
                path.display(),
                line_no
            ))
        })?;
        let msg = msg.trim();
        if msg.is_empty() {
            return Err(ToriiError::InvalidConfig(format!(
                "{}:{}: empty message for {}",
                path.display(),
                line_no,
                hash
            )));
        }
        out.push((hash.to_string(), msg.to_string()));
    }

    if out.is_empty() {
        return Err(ToriiError::InvalidConfig(format!(
            "no reword entries in {}",
            path.display()
        )));
    }
    Ok(out)
}

/// Strip trailing whitespace and ensure exactly one trailing newline, matching
/// git's default message cleanup. Empty after cleanup is rejected by the caller.
fn normalize_message(msg: &str) -> String {
    let trimmed = msg.trim_end();
    format!("{trimmed}\n")
}

/// Rewrite commit messages for the given `(hash, message)` entries.
pub fn reword(repo_path: &Path, entries: &[(String, String)], opts: &RewordOptions) -> Result<Stats> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;

    pre_flight(&repo, opts.allow_dirty)?;

    // Resolve each user-supplied revision to a concrete commit OID.
    let mut targets: HashMap<Oid, String> = HashMap::new();
    for (hash, msg) in entries {
        let oid = repo
            .revparse_single(hash)
            .map_err(|e| ToriiError::Usage(format!("reword {hash}: {e}")))?
            .peel_to_commit()
            .map_err(|e| ToriiError::Usage(format!("{hash} is not a commit: {e}")))?
            .id();
        let normalized = normalize_message(msg);
        if normalized.trim().is_empty() {
            return Err(ToriiError::Usage(format!("empty message for {hash}")));
        }
        if targets.insert(oid, normalized).is_some() {
            return Err(ToriiError::Usage(format!(
                "commit {hash} listed more than once"
            )));
        }
    }

    // Oldest-first so parents are recreated before their children.
    let oids = collect_commits(&repo, opts.since.as_deref())?;
    let walk_set: HashSet<Oid> = oids.iter().copied().collect();

    // Reachability check up front (before any snapshot/mutation).
    let unreachable: Vec<Oid> = targets
        .keys()
        .filter(|oid| !walk_set.contains(oid))
        .copied()
        .collect();
    for oid in &unreachable {
        println!(
            "⚠ commit {} is not reachable from HEAD{} — skipped",
            short(oid),
            opts.since
                .as_deref()
                .map(|s| format!(" (with --since {s})"))
                .unwrap_or_default()
        );
    }
    if unreachable.len() == targets.len() {
        return Err(ToriiError::Usage(
            "none of the given commits are reachable from HEAD".into(),
        ));
    }

    let snapshot_id = if !opts.no_snapshot && !opts.dry_run {
        let mgr = crate::snapshot::SnapshotManager::new(repo_path)?;
        let id = mgr.create_snapshot(Some("pre-reword"))?;
        println!(
            "📸 Snapshot: {} (revert with: torii snapshot restore {})",
            id, id
        );
        Some(id)
    } else {
        None
    };

    let mut stats = Stats {
        scanned: oids.len(),
        snapshot_id,
        ..Default::default()
    };

    // old_oid -> new_oid (identity when the commit didn't need recreating).
    let mut remap: HashMap<Oid, Oid> = HashMap::new();

    for old_oid in &oids {
        let commit = repo.find_commit(*old_oid).map_err(ToriiError::Git)?;
        let new_msg = targets.get(old_oid);

        // Re-parent against any rewritten ancestors.
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

        let message_changed = new_msg.is_some();
        if message_changed {
            stats.matched += 1;
        }

        // A commit that neither changes message nor sits above a rewritten
        // ancestor keeps its OID untouched.
        if !message_changed && !parents_changed {
            remap.insert(*old_oid, *old_oid);
            continue;
        }

        if opts.dry_run {
            remap.insert(*old_oid, *old_oid); // pretend identity for descendants
            stats.rewritten += 1;
            continue;
        }

        // Preserve author and committer exactly — name, email AND timestamps.
        let orig_author = commit.author();
        let orig_committer = commit.committer();
        let author = Signature::new(
            orig_author.name().unwrap_or(""),
            orig_author.email().unwrap_or(""),
            &orig_author.when(),
        )
        .map_err(ToriiError::Git)?;
        let committer = Signature::new(
            orig_committer.name().unwrap_or(""),
            orig_committer.email().unwrap_or(""),
            &orig_committer.when(),
        )
        .map_err(ToriiError::Git)?;

        let message: &str = match new_msg {
            Some(m) => m.as_str(),
            None => commit.message().unwrap_or(""),
        };
        let tree = commit.tree().map_err(ToriiError::Git)?;
        let parent_refs: Vec<&Commit> = new_parents.iter().collect();

        let new_oid = crate::core::commit_inner_split(
            &repo,
            None,
            &author,
            &committer,
            message,
            &tree,
            &parent_refs,
        )?;

        remap.insert(*old_oid, new_oid);
        stats.rewritten += 1;
    }

    if opts.dry_run {
        return Ok(stats);
    }

    update_refs(&repo, &remap, &mut stats)?;

    Ok(stats)
}

fn short(oid: &Oid) -> String {
    let s = oid.to_string();
    s.chars().take(8).collect()
}

pub fn print_summary(stats: &Stats, dry_run: bool) {
    if dry_run {
        println!(
            "✏  Dry-run: would reword {} commit(s) and recreate {} of {} commits, touching {} refs.",
            stats.matched, stats.rewritten, stats.scanned, stats.refs_updated
        );
        println!("   Run again without --dry-run to apply.");
        return;
    }
    println!(
        "✅ Reword complete: {} message(s) rewritten, {} commits recreated, {} refs updated.",
        stats.matched, stats.rewritten, stats.refs_updated
    );
    if let Some(id) = &stats.snapshot_id {
        println!("   Revert: torii snapshot restore {}", id);
    }
    println!("   Push: torii sync --push --force  (history was rewritten)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ensures_single_trailing_newline() {
        assert_eq!(normalize_message("hello"), "hello\n");
        assert_eq!(normalize_message("hello\n\n\n"), "hello\n");
        assert_eq!(normalize_message("a\nb  "), "a\nb\n");
    }

    #[test]
    fn map_parses_hash_and_message() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "# comment\n\n3024979 feat: add login\n6a58608  fix: null check  \n",
        )
        .unwrap();
        let entries = load_reword_map(tmp.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], ("3024979".into(), "feat: add login".into()));
        assert_eq!(entries[1], ("6a58608".into(), "fix: null check".into()));
    }

    #[test]
    fn map_rejects_message_only_lines() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "justahashnomessage\n").unwrap();
        assert!(load_reword_map(tmp.path()).is_err());
    }
}
