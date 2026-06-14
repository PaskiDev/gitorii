//! History rewriting that touches **tree content** — the torii-semantic
//! equivalent of `git filter-branch` / `git filter-repo`.
//!
//! A single engine ([`run_filter`]) rev-walks the history reachable from HEAD,
//! applies a per-commit [`CommitFilter`] (tree transform and/or date override),
//! recreates each commit preserving identity/message, re-points every local ref
//! and (because content changed) syncs the working tree. Built on the same
//! pre-flight + snapshot + ref-update machinery as `history_reauthor`.
//!
//! Commands built on it: `replace-text`, `filter-path`, `redate`, `exec-filter`.

use crate::error::{Result, ToriiError};
use git2::{Commit, ObjectType, Oid, Repository, Signature, Time, Tree};
use std::collections::HashMap;
use std::path::Path;

use super::history_reauthor::{collect_commits, pre_flight, update_refs, Stats};

#[derive(Debug, Clone, Default)]
pub struct FilterOptions {
    /// Limit the walk to commits reachable from HEAD but not from this rev.
    pub since: Option<String>,
    /// Report what would change; touch nothing.
    pub dry_run: bool,
    /// Skip the safety snapshot.
    pub no_snapshot: bool,
    /// Allow a dirty working tree.
    pub allow_dirty: bool,
    /// Drop non-merge commits that introduce no change after the transform.
    pub prune_empty: bool,
}

/// A per-commit transform. Implementors rewrite the tree and/or the dates.
pub trait CommitFilter {
    /// Return the new tree OID for `tree` (identity allowed).
    fn filter_tree(&mut self, _repo: &Repository, tree: &Tree) -> Result<Oid> {
        Ok(tree.id())
    }
    /// Optional `(author_time, committer_time)` overrides for this commit.
    fn filter_dates(&mut self, _commit: &Commit) -> Result<(Option<Time>, Option<Time>)> {
        Ok((None, None))
    }
    /// Short label for the snapshot name + summary (e.g. "replace-text").
    fn label(&self) -> &str;
}

/// Clone `sig`'s identity but stamp it with `when` (or keep the original time).
fn sig_retimed(sig: &Signature, when: Option<Time>) -> Result<Signature<'static>> {
    let t = when.unwrap_or_else(|| sig.when());
    Signature::new(sig.name().unwrap_or(""), sig.email().unwrap_or(""), &t).map_err(ToriiError::Git)
}

/// Engine: rewrite history applying `filter` to every reachable commit.
pub fn run_filter(
    repo_path: &Path,
    opts: &FilterOptions,
    filter: &mut dyn CommitFilter,
) -> Result<Stats> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
    pre_flight(&repo, opts.allow_dirty)?;

    let snapshot_id = if !opts.no_snapshot && !opts.dry_run {
        let mgr = crate::snapshot::SnapshotManager::new(repo_path)?;
        let id = mgr.create_snapshot(Some(&format!("pre-{}", filter.label())))?;
        println!(
            "📸 Snapshot: {} (revert with: torii snapshot restore {})",
            id, id
        );
        Some(id)
    } else {
        None
    };

    let oids = collect_commits(&repo, opts.since.as_deref())?;
    let mut stats = Stats {
        scanned: oids.len(),
        snapshot_id,
        ..Default::default()
    };

    // old -> replacement: Some(oid) kept/redirected, None = dropped root.
    let mut remap: HashMap<Oid, Option<Oid>> = HashMap::new();
    let mut content_touched = false;

    for old_oid in &oids {
        let commit = repo.find_commit(*old_oid).map_err(ToriiError::Git)?;

        // Remap parents, dropping pruned ones and de-duplicating.
        let orig_parents: Vec<Oid> = commit.parent_ids().collect();
        let mut new_parent_oids: Vec<Oid> = Vec::new();
        for pid in &orig_parents {
            let mapped = match remap.get(pid) {
                Some(Some(x)) => Some(*x),
                Some(None) => None,
                None => Some(*pid),
            };
            if let Some(m) = mapped {
                if !new_parent_oids.contains(&m) {
                    new_parent_oids.push(m);
                }
            }
        }

        let orig_tree = commit.tree().map_err(ToriiError::Git)?;
        let new_tree_oid = filter.filter_tree(&repo, &orig_tree)?;
        let tree_changed = new_tree_oid != orig_tree.id();
        if tree_changed {
            content_touched = true;
            stats.matched += 1;
        }

        // Prune: a non-merge commit that introduces no change vs its parent,
        // or an empty root.
        if opts.prune_empty && new_parent_oids.len() <= 1 {
            let parent_tree = new_parent_oids
                .first()
                .and_then(|p| repo.find_commit(*p).ok())
                .and_then(|c| c.tree().ok())
                .map(|t| t.id());
            let is_empty_root = new_parent_oids.is_empty()
                && repo
                    .find_tree(new_tree_oid)
                    .map(|t| t.is_empty())
                    .unwrap_or(false);
            if parent_tree == Some(new_tree_oid) || is_empty_root {
                remap.insert(*old_oid, new_parent_oids.first().copied());
                stats.pruned += 1;
                continue;
            }
        }

        let (a_time, c_time) = filter.filter_dates(&commit)?;
        let dates_changed = a_time.is_some() || c_time.is_some();
        let parents_changed = new_parent_oids != orig_parents;

        if !tree_changed && !dates_changed && !parents_changed {
            remap.insert(*old_oid, Some(*old_oid));
            continue;
        }

        if opts.dry_run {
            remap.insert(*old_oid, Some(*old_oid));
            stats.rewritten += 1;
            continue;
        }

        let author = sig_retimed(&commit.author(), a_time)?;
        let committer = sig_retimed(&commit.committer(), c_time)?;
        let new_tree = repo.find_tree(new_tree_oid).map_err(ToriiError::Git)?;
        let parent_commits: Vec<Commit> = new_parent_oids
            .iter()
            .map(|o| repo.find_commit(*o))
            .collect::<std::result::Result<_, _>>()
            .map_err(ToriiError::Git)?;
        let parent_refs: Vec<&Commit> = parent_commits.iter().collect();
        let message = commit.message().unwrap_or("");

        let new_oid = crate::core::commit_inner_split(
            &repo,
            None,
            &author,
            &committer,
            message,
            &new_tree,
            &parent_refs,
        )?;
        remap.insert(*old_oid, Some(new_oid));
        stats.rewritten += 1;
    }

    if opts.dry_run {
        return Ok(stats);
    }

    let ref_map: HashMap<Oid, Oid> = remap
        .iter()
        .filter_map(|(k, v)| v.map(|nv| (*k, nv)))
        .collect();
    update_refs(&repo, &ref_map, &mut stats)?;

    // Tree content changed → bring the working tree + index in line with HEAD,
    // mirroring `history remove-file`. Safe: pre_flight refused a dirty tree
    // unless --allow-dirty was passed.
    if content_touched {
        if let Ok(head) = repo.head() {
            if let Ok(commit) = head.peel_to_commit() {
                let mut co = git2::build::CheckoutBuilder::default();
                co.force();
                let _ = repo.checkout_tree(commit.as_object(), Some(&mut co));
            }
        }
    }

    Ok(stats)
}

pub fn print_summary(stats: &Stats, label: &str, dry_run: bool) {
    if dry_run {
        println!(
            "✏  Dry-run ({label}): would change {} and prune {} of {} commits.",
            stats.matched, stats.pruned, stats.scanned
        );
        println!("   Run again without --dry-run to apply.");
        return;
    }
    println!(
        "✅ {label} complete: {} commits recreated ({} content-changed), {} pruned, {} refs updated.",
        stats.rewritten, stats.matched, stats.pruned, stats.refs_updated
    );
    if let Some(id) = &stats.snapshot_id {
        println!("   Revert: torii snapshot restore {}", id);
    }
    println!("   Push: torii sync --push --force  (history was rewritten)");
}

// ===========================================================================
// replace-text
// ===========================================================================

const DEFAULT_REPLACEMENT: &str = "***REMOVED***";

enum CompiledRule {
    Literal { needle: Vec<u8>, repl: Vec<u8> },
    Regex { re: regex::bytes::Regex, repl: Vec<u8> },
}

impl CompiledRule {
    fn apply(&self, data: Vec<u8>) -> Vec<u8> {
        match self {
            CompiledRule::Literal { needle, repl } => replace_bytes(&data, needle, repl),
            CompiledRule::Regex { re, repl } => {
                re.replace_all(&data, regex::bytes::NoExpand(repl)).into_owned()
            }
        }
    }
}

/// Literal byte-subsequence replacement (works on binary too).
fn replace_bytes(haystack: &[u8], needle: &[u8], repl: &[u8]) -> Vec<u8> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(repl);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// Parse one rule line: `literal:OLD[==>NEW]`, `regex:PAT[==>REPL]`, or a bare
/// string (treated as literal). Missing `==>` ⇒ replacement is `***REMOVED***`.
fn parse_rule_line(line: &str) -> Result<Option<CompiledRule>> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let (is_regex, rest) = if let Some(r) = t.strip_prefix("regex:") {
        (true, r)
    } else if let Some(r) = t.strip_prefix("literal:") {
        (false, r)
    } else {
        (false, t)
    };
    let (pat, repl) = match rest.split_once("==>") {
        Some((p, r)) => (p, r),
        None => (rest, DEFAULT_REPLACEMENT),
    };
    if pat.is_empty() {
        return Err(ToriiError::Usage("empty pattern in replace-text rule".into()));
    }
    let rule = if is_regex {
        CompiledRule::Regex {
            re: regex::bytes::Regex::new(pat)
                .map_err(|e| ToriiError::Usage(format!("bad regex {pat:?}: {e}")))?,
            repl: repl.as_bytes().to_vec(),
        }
    } else {
        CompiledRule::Literal {
            needle: pat.as_bytes().to_vec(),
            repl: repl.as_bytes().to_vec(),
        }
    };
    Ok(Some(rule))
}

pub struct ReplaceText {
    rules: Vec<CompiledRule>,
    redact_secrets: bool,
    paths: Option<Vec<String>>,
    blob_cache: HashMap<Oid, Oid>,
}

impl ReplaceText {
    /// Build from a rules file, inline rule strings, and/or `--redact-secrets`.
    pub fn new(
        rules_file: Option<&Path>,
        inline: &[String],
        redact_secrets: bool,
        paths: Option<Vec<String>>,
    ) -> Result<Self> {
        let mut rules = Vec::new();
        if let Some(path) = rules_file {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| ToriiError::Fs(format!("read {}: {}", path.display(), e)))?;
            for line in raw.lines() {
                if let Some(rule) = parse_rule_line(line)? {
                    rules.push(rule);
                }
            }
        }
        for spec in inline {
            if let Some(rule) = parse_rule_line(spec)? {
                rules.push(rule);
            }
        }
        if rules.is_empty() && !redact_secrets {
            return Err(ToriiError::Usage(
                "no rules: pass --rules, --literal/--regex, or --redact-secrets".into(),
            ));
        }
        Ok(Self {
            rules,
            redact_secrets,
            paths,
            blob_cache: HashMap::new(),
        })
    }

    fn path_included(&self, full: &str) -> bool {
        match &self.paths {
            None => true,
            Some(ps) => ps
                .iter()
                .any(|p| full == p || full.starts_with(&format!("{}/", p.trim_end_matches('/')))),
        }
    }

    fn transform_content(&self, content: &[u8]) -> Option<Vec<u8>> {
        let mut data = content.to_vec();
        for rule in &self.rules {
            data = rule.apply(data);
        }
        if self.redact_secrets {
            data = redact_secret_lines(data);
        }
        if data == content {
            None
        } else {
            Some(data)
        }
    }

    fn transform_blob(&mut self, repo: &Repository, id: Oid) -> Result<Oid> {
        if let Some(c) = self.blob_cache.get(&id) {
            return Ok(*c);
        }
        let blob = repo.find_blob(id).map_err(ToriiError::Git)?;
        let new_id = match self.transform_content(blob.content()) {
            Some(data) => repo.blob(&data).map_err(ToriiError::Git)?,
            None => id,
        };
        self.blob_cache.insert(id, new_id);
        Ok(new_id)
    }

    fn transform_tree(&mut self, repo: &Repository, tree: &Tree, prefix: &str) -> Result<Oid> {
        let mut builder = repo.treebuilder(Some(tree)).map_err(ToriiError::Git)?;
        for entry in tree.iter() {
            let name = match entry.name() {
                Some(n) => n.to_string(),
                None => continue,
            };
            let full = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            match entry.kind() {
                Some(ObjectType::Tree) => {
                    let sub = repo.find_tree(entry.id()).map_err(ToriiError::Git)?;
                    let new_sub = self.transform_tree(repo, &sub, &full)?;
                    if new_sub != entry.id() {
                        builder
                            .insert(&name, new_sub, entry.filemode())
                            .map_err(ToriiError::Git)?;
                    }
                }
                Some(ObjectType::Blob) => {
                    // Only regular files (skip symlinks 0o120000, gitlinks).
                    let mode = entry.filemode();
                    if (mode == 0o100644 || mode == 0o100755) && self.path_included(&full) {
                        let new_blob = self.transform_blob(repo, entry.id())?;
                        if new_blob != entry.id() {
                            builder
                                .insert(&name, new_blob, mode)
                                .map_err(ToriiError::Git)?;
                        }
                    }
                }
                _ => {}
            }
        }
        builder.write().map_err(ToriiError::Git)
    }
}

impl CommitFilter for ReplaceText {
    fn filter_tree(&mut self, repo: &Repository, tree: &Tree) -> Result<Oid> {
        self.transform_tree(repo, tree, "")
    }
    fn label(&self) -> &str {
        "replace-text"
    }
}

/// Replace any line flagged by the built-in secret scanner with `***REMOVED***`.
fn redact_secret_lines(data: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for seg in data.split_inclusive(|&b| b == b'\n') {
        let (line, nl): (&[u8], &[u8]) = match seg.last() {
            Some(b'\n') => (&seg[..seg.len() - 1], b"\n"),
            _ => (seg, b""),
        };
        if let Ok(s) = std::str::from_utf8(line) {
            if crate::scanner::matching_pattern(s).is_some() {
                out.extend_from_slice(DEFAULT_REPLACEMENT.as_bytes());
                out.extend_from_slice(nl);
                continue;
            }
        }
        out.extend_from_slice(line);
        out.extend_from_slice(nl);
    }
    out
}

// ===========================================================================
// shared tree builders
// ===========================================================================

/// Build a tree from a flat `(path, oid, mode)` list via an in-memory index.
fn build_tree_from_entries(repo: &Repository, entries: &[(String, Oid, u32)]) -> Result<Oid> {
    let mut index = git2::Index::new().map_err(ToriiError::Git)?;
    for (path, oid, mode) in entries {
        let entry = git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: *mode,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: *oid,
            flags: 0,
            flags_extended: 0,
            path: path.clone().into_bytes(),
        };
        index.add(&entry).map_err(ToriiError::Git)?;
    }
    index.write_tree_to(repo).map_err(ToriiError::Git)
}

/// Recursively collect every leaf entry (blob or gitlink) as `(path, oid, mode)`.
fn collect_entries(
    repo: &Repository,
    tree: &Tree,
    prefix: &str,
    out: &mut Vec<(String, Oid, u32)>,
) -> Result<()> {
    for entry in tree.iter() {
        let name = match entry.name() {
            Some(n) => n,
            None => continue,
        };
        let full = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        match entry.kind() {
            Some(ObjectType::Tree) => {
                let sub = repo.find_tree(entry.id()).map_err(ToriiError::Git)?;
                collect_entries(repo, &sub, &full, out)?;
            }
            Some(ObjectType::Blob) | Some(ObjectType::Commit) => {
                out.push((full, entry.id(), entry.filemode() as u32));
            }
            _ => {}
        }
    }
    Ok(())
}

// ===========================================================================
// filter-path
// ===========================================================================

/// `path` matches `pat` if it equals it or sits under it (prefix-by-component).
fn matches_path(path: &str, pat: &str) -> bool {
    let pat = pat.trim_end_matches('/');
    path == pat || path.starts_with(&format!("{pat}/"))
}

pub struct FilterPath {
    keep: Vec<String>,
    remove: Vec<String>,
    subdirectory: Option<String>,
    rename: Vec<(String, String)>,
}

impl FilterPath {
    pub fn new(
        keep: Vec<String>,
        remove: Vec<String>,
        subdirectory: Option<String>,
        rename: Vec<(String, String)>,
    ) -> Result<Self> {
        if keep.is_empty() && remove.is_empty() && subdirectory.is_none() && rename.is_empty() {
            return Err(ToriiError::Usage(
                "filter-path needs at least one of --keep / --remove / --subdirectory / --rename"
                    .into(),
            ));
        }
        Ok(Self {
            keep,
            remove,
            subdirectory,
            rename,
        })
    }

    /// Map an original path to its new path, or `None` to drop it.
    fn map_path(&self, path: &str) -> Option<String> {
        let mut p = path.to_string();

        if let Some(sub) = &self.subdirectory {
            let pref = format!("{}/", sub.trim_end_matches('/'));
            p = p.strip_prefix(&pref)?.to_string();
        }
        if self.remove.iter().any(|r| matches_path(&p, r)) {
            return None;
        }
        if !self.keep.is_empty() && !self.keep.iter().any(|k| matches_path(&p, k)) {
            return None;
        }
        for (old, new) in &self.rename {
            let from = old.trim_end_matches('/');
            let to = new.trim_end_matches('/');
            if p == from {
                p = to.to_string();
                break;
            }
            if let Some(rest) = p.strip_prefix(&format!("{from}/")) {
                p = format!("{to}/{rest}");
                break;
            }
        }
        Some(p)
    }
}

impl CommitFilter for FilterPath {
    fn filter_tree(&mut self, repo: &Repository, tree: &Tree) -> Result<Oid> {
        let mut entries = Vec::new();
        collect_entries(repo, tree, "", &mut entries)?;
        let kept: Vec<(String, Oid, u32)> = entries
            .into_iter()
            .filter_map(|(path, oid, mode)| {
                self.map_path(&path)
                    .filter(|np| !np.is_empty())
                    .map(|np| (np, oid, mode))
            })
            .collect();
        build_tree_from_entries(repo, &kept)
    }
    fn label(&self) -> &str {
        "filter-path"
    }
}

// ===========================================================================
// redate
// ===========================================================================

pub struct Redate {
    dates: HashMap<Oid, Time>,
}

impl Redate {
    pub fn new(entries: &[(String, String)], repo_path: &Path) -> Result<Self> {
        let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;
        let mut dates = HashMap::new();
        for (hash, when) in entries {
            let oid = repo
                .revparse_single(hash)
                .map_err(|e| ToriiError::Usage(format!("redate {hash}: {e}")))?
                .peel_to_commit()
                .map_err(|e| ToriiError::Usage(format!("{hash} is not a commit: {e}")))?
                .id();
            let t = crate::core::parse_git_date(when)
                .ok_or_else(|| ToriiError::Usage(format!("unparseable date {when:?}")))?;
            dates.insert(oid, t);
        }
        if dates.is_empty() {
            return Err(ToriiError::Usage("no commits to redate".into()));
        }
        Ok(Self { dates })
    }
}

impl CommitFilter for Redate {
    fn filter_dates(&mut self, commit: &Commit) -> Result<(Option<Time>, Option<Time>)> {
        match self.dates.get(&commit.id()) {
            Some(t) => Ok((Some(*t), Some(*t))),
            None => Ok((None, None)),
        }
    }
    fn label(&self) -> &str {
        "redate"
    }
}

// ===========================================================================
// exec-filter (the arbitrary --tree-filter escape hatch)
// ===========================================================================

pub struct ExecFilter {
    command: String,
    counter: usize,
}

impl ExecFilter {
    pub fn new(command: String) -> Self {
        Self {
            command,
            counter: 0,
        }
    }
}

impl CommitFilter for ExecFilter {
    fn filter_tree(&mut self, repo: &Repository, tree: &Tree) -> Result<Oid> {
        self.counter += 1;
        let dir = std::env::temp_dir().join(format!(
            "torii-exec-{}-{}",
            std::process::id(),
            self.counter
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| ToriiError::Fs(format!("create temp dir: {e}")))?;

        materialize_tree(repo, tree, &dir)?;

        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .current_dir(&dir)
            .status()
            .map_err(|e| ToriiError::RepoState(format!("running tree-filter command: {e}")))?;
        if !status.success() {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(ToriiError::RepoState(format!(
                "tree-filter command failed ({status}): {}",
                self.command
            )));
        }

        let mut entries = Vec::new();
        import_dir(repo, &dir, "", &mut entries)?;
        let _ = std::fs::remove_dir_all(&dir);
        build_tree_from_entries(repo, &entries)
    }
    fn label(&self) -> &str {
        "exec-filter"
    }
}

/// Write a tree's contents onto disk under `dir`.
fn materialize_tree(repo: &Repository, tree: &Tree, dir: &Path) -> Result<()> {
    for entry in tree.iter() {
        let name = match entry.name() {
            Some(n) => n,
            None => continue,
        };
        let path = dir.join(name);
        match entry.kind() {
            Some(ObjectType::Tree) => {
                std::fs::create_dir_all(&path)
                    .map_err(|e| ToriiError::Fs(format!("mkdir {}: {e}", path.display())))?;
                let sub = repo.find_tree(entry.id()).map_err(ToriiError::Git)?;
                materialize_tree(repo, &sub, &path)?;
            }
            Some(ObjectType::Blob) => {
                let blob = repo.find_blob(entry.id()).map_err(ToriiError::Git)?;
                let mode = entry.filemode();
                #[cfg(unix)]
                if mode == 0o120000 {
                    // symlink: content is the target
                    if let Ok(target) = std::str::from_utf8(blob.content()) {
                        let _ = std::os::unix::fs::symlink(target, &path);
                    }
                    continue;
                }
                std::fs::write(&path, blob.content())
                    .map_err(|e| ToriiError::Fs(format!("write {}: {e}", path.display())))?;
                #[cfg(unix)]
                if mode == 0o100755 {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
                }
            }
            _ => {} // gitlinks: nothing materialized
        }
    }
    Ok(())
}

/// Read a directory tree back into git objects as `(path, oid, mode)` entries.
fn import_dir(
    repo: &Repository,
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, Oid, u32)>,
) -> Result<()> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| ToriiError::Fs(format!("read dir {}: {e}", dir.display())))?;
    for ent in rd {
        let ent = ent.map_err(|e| ToriiError::Fs(format!("dir entry: {e}")))?;
        let name = ent.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let ft = ent
            .file_type()
            .map_err(|e| ToriiError::Fs(format!("file type: {e}")))?;

        #[cfg(unix)]
        if ft.is_symlink() {
            if let Ok(target) = std::fs::read_link(ent.path()) {
                let oid = repo
                    .blob(target.to_string_lossy().as_bytes())
                    .map_err(ToriiError::Git)?;
                out.push((rel, oid, 0o120000));
            }
            continue;
        }

        if ft.is_dir() {
            import_dir(repo, &ent.path(), &rel, out)?;
        } else if ft.is_file() {
            let data = std::fs::read(ent.path())
                .map_err(|e| ToriiError::Fs(format!("read {}: {e}", ent.path().display())))?;
            let oid = repo.blob(&data).map_err(ToriiError::Git)?;
            #[allow(unused_mut)]
            let mut mode = 0o100644u32;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if ent.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false) {
                    mode = 0o100755;
                }
            }
            out.push((rel, oid, mode));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_byte_replace() {
        assert_eq!(replace_bytes(b"a SECRET b SECRET", b"SECRET", b"X"), b"a X b X");
        assert_eq!(replace_bytes(b"none here", b"SECRET", b"X"), b"none here");
    }

    #[test]
    fn rule_parsing_forms() {
        // bare → literal, default replacement
        assert!(matches!(
            parse_rule_line("hunter2").unwrap(),
            Some(CompiledRule::Literal { .. })
        ));
        // literal with replacement
        match parse_rule_line("literal:old==>new").unwrap().unwrap() {
            CompiledRule::Literal { needle, repl } => {
                assert_eq!(needle, b"old");
                assert_eq!(repl, b"new");
            }
            _ => panic!("expected literal"),
        }
        // regex
        assert!(matches!(
            parse_rule_line("regex:[0-9]+==>N").unwrap(),
            Some(CompiledRule::Regex { .. })
        ));
        // comments / blanks skipped
        assert!(parse_rule_line("# c").unwrap().is_none());
        assert!(parse_rule_line("   ").unwrap().is_none());
    }

    #[test]
    fn default_replacement_when_no_arrow() {
        match parse_rule_line("literal:token").unwrap().unwrap() {
            CompiledRule::Literal { repl, .. } => assert_eq!(repl, DEFAULT_REPLACEMENT.as_bytes()),
            _ => panic!(),
        }
    }

    #[test]
    fn filter_path_subdirectory_strips_prefix() {
        let f = FilterPath::new(vec![], vec![], Some("crates/lib".into()), vec![]).unwrap();
        assert_eq!(f.map_path("crates/lib/src/a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(f.map_path("README.md"), None); // outside the subdir → dropped
    }

    #[test]
    fn filter_path_remove_keep_rename() {
        let remove = FilterPath::new(vec![], vec!["secrets".into()], None, vec![]).unwrap();
        assert_eq!(remove.map_path("secrets/key.pem"), None);
        assert_eq!(remove.map_path("src/main.rs").as_deref(), Some("src/main.rs"));

        let keep = FilterPath::new(vec!["src".into()], vec![], None, vec![]).unwrap();
        assert_eq!(keep.map_path("src/main.rs").as_deref(), Some("src/main.rs"));
        assert_eq!(keep.map_path("docs/x.md"), None);

        let rename =
            FilterPath::new(vec![], vec![], None, vec![("old".into(), "new".into())]).unwrap();
        assert_eq!(rename.map_path("old/a.txt").as_deref(), Some("new/a.txt"));
    }
}
