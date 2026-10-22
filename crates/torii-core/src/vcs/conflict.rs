//! Resolve conflicted index entries in favor of one side (local / remote).
//!
//! "A side wins" means only the conflicting hunks: non-clashing changes of
//! the other side are kept. Conflicts without hunks (binary, delete/modify,
//! a missing side) take the whole chosen side, or delete the file when that
//! side does not have it.

use std::path::Path;

use anyhow::{Context, Result};
use git2::{IndexEntry, Repository};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Local,
    Remote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Merge,
    Rebase,
    CherryPick,
}

impl OpKind {
    /// Whether choosing `side` means taking the index "ours" side.
    /// Merge/cherry-pick: local = ours. Rebase: local = theirs.
    pub fn favors_ours(self, side: Side) -> bool {
        match self {
            OpKind::Merge | OpKind::CherryPick => side == Side::Local,
            OpKind::Rebase => side == Side::Remote,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: String,
    /// false: resolved by taking the whole file from one side.
    pub hunks: bool,
}

pub struct ConflictContext {
    pub op: OpKind,
    pub local_label: String,
    pub remote_label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    All(Side),
    PerFile(Vec<(String, Side)>),
    Manual,
}

pub trait ConflictResolver {
    fn choose(&self, ctx: &ConflictContext, files: &[ConflictFile]) -> Result<Resolution>;
}

/// Always leaves the conflicts for manual resolution.
pub struct ManualResolver;

impl ConflictResolver for ManualResolver {
    fn choose(&self, _: &ConflictContext, _: &[ConflictFile]) -> Result<Resolution> {
        Ok(Resolution::Manual)
    }
}

/// Always picks the same side for every file.
pub struct FixedResolver(pub Side);

impl ConflictResolver for FixedResolver {
    fn choose(&self, _: &ConflictContext, _: &[ConflictFile]) -> Result<Resolution> {
        Ok(Resolution::All(self.0))
    }
}

fn entry_path(c: &git2::IndexConflict) -> Option<Vec<u8>> {
    c.our
        .as_ref()
        .or(c.their.as_ref())
        .or(c.ancestor.as_ref())
        .map(|e| e.path.clone())
}

fn has_hunks(repo: &Repository, c: &git2::IndexConflict) -> Result<bool> {
    let (Some(our), Some(their)) = (&c.our, &c.their) else {
        return Ok(false);
    };
    let o = repo.find_blob(our.id)?;
    let t = repo.find_blob(their.id)?;
    Ok(!o.is_binary() && !t.is_binary())
}

/// Conflicted files of the index, sorted by path.
pub fn list_conflicts(repo: &Repository) -> Result<Vec<ConflictFile>> {
    let index = repo.index()?;
    let mut out = Vec::new();
    for c in index.conflicts()? {
        let c = c?;
        let Some(raw) = entry_path(&c) else { continue };
        out.push(ConflictFile {
            path: String::from_utf8_lossy(&raw).into_owned(),
            hunks: has_hunks(repo, &c)?,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn write_workdir(repo: &Repository, rel: &Path, content: &[u8], mode: u32) -> Result<()> {
    let workdir = repo
        .workdir()
        .context("bare repository has no working tree")?;
    let full = workdir.join(rel);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&full, content).with_context(|| format!("writing {}", full.display()))?;
    #[cfg(unix)]
    if mode & 0o170000 == 0o100000 {
        use std::os::unix::fs::PermissionsExt;
        let perm = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
        std::fs::set_permissions(&full, std::fs::Permissions::from_mode(perm))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

/// Stage-0 copy of a conflict entry (stage bits cleared).
fn stage0(e: &IndexEntry) -> IndexEntry {
    IndexEntry {
        ctime: e.ctime,
        mtime: e.mtime,
        dev: e.dev,
        ino: e.ino,
        mode: e.mode,
        uid: e.uid,
        gid: e.gid,
        file_size: e.file_size,
        id: e.id,
        flags: e.flags & !0x3000,
        flags_extended: e.flags_extended,
        path: e.path.clone(),
    }
}

/// Drop the conflict stages (1..=3) and any stage-0 entry of `rel`.
/// Missing stages are expected (delete/modify), so errors are ignored.
fn drop_conflict(index: &mut git2::Index, rel: &Path) {
    for stage in 1..=3 {
        let _ = index.remove(rel, stage);
    }
    let _ = index.remove(rel, 0);
}

/// Three-way merge of one file where conflicting hunks go to the favored side.
/// libgit2 0.19 has no file-level merge API, so merge one-entry trees.
fn merge_hunks(
    repo: &Repository,
    ancestor: Option<&IndexEntry>,
    our: &IndexEntry,
    their: &IndexEntry,
    favor_ours: bool,
) -> Result<Vec<u8>> {
    const NAME: &str = "f";
    let tree = |e: Option<&IndexEntry>| -> Result<git2::Tree<'_>> {
        let mut tb = repo.treebuilder(None)?;
        if let Some(e) = e {
            tb.insert(NAME, e.id, 0o100644)?;
        }
        Ok(repo.find_tree(tb.write()?)?)
    };
    let mut opts = git2::MergeOptions::new();
    opts.file_favor(if favor_ours {
        git2::FileFavor::Ours
    } else {
        git2::FileFavor::Theirs
    });
    let merged = repo.merge_trees(
        &tree(ancestor)?,
        &tree(Some(our))?,
        &tree(Some(their))?,
        Some(&opts),
    )?;
    let entry = merged
        .get_path(Path::new(NAME), 0)
        .context("merge of conflicting hunks left the file unresolved")?;
    Ok(repo.find_blob(entry.id)?.content().to_vec())
}

fn resolve_one(
    repo: &Repository,
    index: &mut git2::Index,
    c: &git2::IndexConflict,
    favor_ours: bool,
) -> Result<()> {
    let Some(raw) = entry_path(c) else {
        return Ok(());
    };
    let rel =
        Path::new(std::str::from_utf8(&raw).context("non-UTF-8 path in conflict")?).to_owned();
    let chosen = if favor_ours { &c.our } else { &c.their };

    let (content, entry) = if has_hunks(repo, c)? {
        let (our, their) = (c.our.as_ref().unwrap(), c.their.as_ref().unwrap());
        let merged = merge_hunks(repo, c.ancestor.as_ref(), our, their, favor_ours)?;
        (merged, chosen.as_ref().unwrap())
    } else {
        match chosen {
            Some(e) => (repo.find_blob(e.id)?.content().to_vec(), e),
            None => {
                let workdir = repo
                    .workdir()
                    .context("bare repository has no working tree")?;
                match std::fs::remove_file(workdir.join(&rel)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                drop_conflict(index, &rel);
                return Ok(());
            }
        }
    };

    write_workdir(repo, &rel, &content, entry.mode)?;
    drop_conflict(index, &rel);
    index.add_frombuffer(&stage0(entry), &content)?;
    Ok(())
}

/// Apply `res`; returns how many files are still conflicted.
pub fn resolve_files(repo: &Repository, op: OpKind, res: &Resolution) -> Result<usize> {
    let mut index = repo.index()?;
    let mut conflicts = Vec::new();
    for c in index.conflicts()? {
        conflicts.push(c?);
    }
    for c in &conflicts {
        let Some(raw) = entry_path(c) else { continue };
        let side = match res {
            Resolution::Manual => None,
            Resolution::All(s) => Some(*s),
            Resolution::PerFile(list) => {
                let p = String::from_utf8_lossy(&raw);
                list.iter().find(|(f, _)| *f == p).map(|(_, s)| *s)
            }
        };
        if let Some(side) = side {
            resolve_one(repo, &mut index, c, op.favors_ours(side))?;
        }
    }
    index.write()?;
    let mut left = 0;
    for c in index.conflicts()? {
        c?;
        left += 1;
    }
    Ok(left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    type Case<'a> = (
        &'a str,
        Option<&'a [u8]>,
        Option<&'a [u8]>,
        Option<&'a [u8]>,
    );

    fn sig() -> git2::Signature<'static> {
        git2::Signature::now("t", "t@example.com").unwrap()
    }

    fn write(dir: &TempDir, name: &str, content: &[u8]) {
        std::fs::write(dir.path().join(name), content).unwrap();
    }

    fn commit_all(repo: &git2::Repository, msg: &str, parents: &[git2::Oid]) -> git2::Oid {
        let mut idx = repo.index().unwrap();
        idx.add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        idx.update_all(["*"], None).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let pcs: Vec<git2::Commit> = parents
            .iter()
            .map(|&o| repo.find_commit(o).unwrap())
            .collect();
        let prefs: Vec<&git2::Commit> = pcs.iter().collect();
        let s = sig();
        repo.commit(Some("HEAD"), &s, &s, msg, &tree, &prefs)
            .unwrap()
    }

    fn apply(dir: &TempDir, which: usize, cases: &[Case]) {
        for (name, base, ours, theirs) in cases {
            let v = [base, ours, theirs][which];
            match v {
                Some(c) => write(dir, name, c),
                None => {
                    let _ = std::fs::remove_file(dir.path().join(name));
                }
            }
        }
    }

    /// Repo left mid-merge: HEAD has `ours`, merged branch has `theirs`.
    fn conflicted(cases: &[Case]) -> (TempDir, git2::Repository) {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        apply(&dir, 0, cases);
        let base = commit_all(&repo, "base", &[]);
        apply(&dir, 2, cases);
        let theirs = commit_all(&repo, "theirs", &[base]);
        repo.reset(
            repo.find_commit(base).unwrap().as_object(),
            git2::ResetType::Hard,
            None,
        )
        .unwrap();
        apply(&dir, 1, cases);
        commit_all(&repo, "ours", &[base]);
        {
            let ann = repo.find_annotated_commit(theirs).unwrap();
            repo.merge(&[&ann], None, None).unwrap();
        }
        (dir, repo)
    }

    fn read(dir: &TempDir, name: &str) -> String {
        std::fs::read_to_string(dir.path().join(name)).unwrap()
    }

    const BASE: &[u8] = b"a\nb\nc\nd\ne\n";
    const OURS: &[u8] = b"A-ours\nb\nc\nd\ne\n";
    const THEIRS: &[u8] = b"A-theirs\nb\nc\nd\nE-theirs\n";

    fn hunk_repo() -> (TempDir, git2::Repository) {
        conflicted(&[("f.txt", Some(BASE), Some(OURS), Some(THEIRS))])
    }

    #[test]
    fn favors_ours_mapping() {
        assert!(OpKind::Merge.favors_ours(Side::Local));
        assert!(!OpKind::Merge.favors_ours(Side::Remote));
        assert!(!OpKind::Rebase.favors_ours(Side::Local));
        assert!(OpKind::Rebase.favors_ours(Side::Remote));
        assert!(OpKind::CherryPick.favors_ours(Side::Local));
    }

    #[test]
    fn local_wins_only_conflicting_hunk() {
        let (dir, repo) = hunk_repo();
        assert_eq!(
            list_conflicts(&repo).unwrap(),
            vec![ConflictFile {
                path: "f.txt".into(),
                hunks: true
            }]
        );
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Local)).unwrap(),
            0
        );
        assert_eq!(read(&dir, "f.txt"), "A-ours\nb\nc\nd\nE-theirs\n");
        assert!(!repo.index().unwrap().has_conflicts());
    }

    #[test]
    fn remote_wins_only_conflicting_hunk() {
        let (dir, repo) = hunk_repo();
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Remote)).unwrap(),
            0
        );
        assert_eq!(read(&dir, "f.txt"), "A-theirs\nb\nc\nd\nE-theirs\n");
        assert!(!repo.index().unwrap().has_conflicts());
    }

    #[test]
    fn per_file_leaves_unchosen_in_conflict() {
        let (dir, repo) = conflicted(&[
            ("a.txt", Some(b"x\n"), Some(b"o\n"), Some(b"t\n")),
            ("b.txt", Some(b"x\n"), Some(b"o\n"), Some(b"t\n")),
        ]);
        let res = Resolution::PerFile(vec![
            ("a.txt".into(), Side::Remote),
            ("nope.txt".into(), Side::Local),
        ]);
        assert_eq!(resolve_files(&repo, OpKind::Merge, &res).unwrap(), 1);
        assert_eq!(read(&dir, "a.txt"), "t\n");
        assert!(read(&dir, "b.txt").contains("<<<<<<<"));
        let left = list_conflicts(&repo).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].path, "b.txt");
    }

    #[test]
    fn manual_touches_nothing() {
        let (dir, repo) = conflicted(&[
            ("a.txt", Some(b"x\n"), Some(b"o\n"), Some(b"t\n")),
            ("b.txt", Some(b"x\n"), Some(b"o\n"), Some(b"t\n")),
        ]);
        let a = read(&dir, "a.txt");
        let b = read(&dir, "b.txt");
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::Manual).unwrap(),
            2
        );
        assert_eq!(read(&dir, "a.txt"), a);
        assert_eq!(read(&dir, "b.txt"), b);
        assert_eq!(list_conflicts(&repo).unwrap().len(), 2);
    }

    #[test]
    fn delete_vs_modify_takes_whole_side() {
        let cases: [Case; 1] = [("g.txt", Some(b"base\n"), None, Some(b"theirs\n"))];
        let (dir, repo) = conflicted(&cases);
        assert_eq!(
            list_conflicts(&repo).unwrap(),
            vec![ConflictFile {
                path: "g.txt".into(),
                hunks: false
            }]
        );
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Local)).unwrap(),
            0
        );
        assert!(!dir.path().join("g.txt").exists());
        assert!(repo
            .index()
            .unwrap()
            .get_path(std::path::Path::new("g.txt"), 0)
            .is_none());
        assert!(!repo.index().unwrap().has_conflicts());

        let (dir, repo) = conflicted(&cases);
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Remote)).unwrap(),
            0
        );
        assert_eq!(read(&dir, "g.txt"), "theirs\n");
        assert!(repo
            .index()
            .unwrap()
            .get_path(std::path::Path::new("g.txt"), 0)
            .is_some());
        assert!(!repo.index().unwrap().has_conflicts());
    }

    #[test]
    fn binary_conflict_takes_whole_side() {
        let (dir, repo) = conflicted(&[(
            "bin.dat",
            Some(&[0u8, 1, 2]),
            Some(&[0u8, 1, 3]),
            Some(&[0u8, 9, 9, 9]),
        )]);
        assert_eq!(
            list_conflicts(&repo).unwrap(),
            vec![ConflictFile {
                path: "bin.dat".into(),
                hunks: false
            }]
        );
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Remote)).unwrap(),
            0
        );
        assert_eq!(
            std::fs::read(dir.path().join("bin.dat")).unwrap(),
            vec![0u8, 9, 9, 9]
        );
    }

    #[test]
    fn rebase_local_means_own_commit() {
        // ours = upstream, theirs = mine, as a rebase leaves the index.
        let (dir, repo) = conflicted(&[(
            "f.txt",
            Some(b"base\n"),
            Some(b"upstream\n"),
            Some(b"mine\n"),
        )]);
        assert_eq!(
            resolve_files(&repo, OpKind::Rebase, &Resolution::All(Side::Local)).unwrap(),
            0
        );
        assert_eq!(read(&dir, "f.txt"), "mine\n");
    }

    #[test]
    fn add_add_conflict_resolves_without_ancestor() {
        let (dir, repo) = conflicted(&[("n.txt", None, Some(b"ours\n"), Some(b"theirs\n"))]);
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Remote)).unwrap(),
            0
        );
        assert_eq!(read(&dir, "n.txt"), "theirs\n");
    }

    #[test]
    fn resolvers_return_fixed_choices() {
        let ctx = ConflictContext {
            op: OpKind::Merge,
            local_label: "l".into(),
            remote_label: "r".into(),
        };
        assert_eq!(
            ManualResolver.choose(&ctx, &[]).unwrap(),
            Resolution::Manual
        );
        assert_eq!(
            FixedResolver(Side::Remote).choose(&ctx, &[]).unwrap(),
            Resolution::All(Side::Remote)
        );
    }
}
