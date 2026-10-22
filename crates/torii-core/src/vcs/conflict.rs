//! Resolve conflicted index entries in favor of one side (local / remote).
//!
//! "A side wins" means only the conflicting hunks: non-clashing changes of
//! the other side are kept. Conflicts without hunks (binary, delete/modify,
//! a missing side) take the whole chosen side, or delete the file when that
//! side does not have it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::build::CheckoutBuilder;
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

const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;

fn is_regular(mode: u32) -> bool {
    mode & S_IFMT == S_IFREG
}

/// Hunk merge only applies to text files present on both sides. Symlinks and
/// gitlinks (whose ids are commits, often absent from the odb) never qualify.
fn has_hunks(repo: &Repository, c: &git2::IndexConflict) -> Result<bool> {
    let (Some(our), Some(their)) = (&c.our, &c.their) else {
        return Ok(false);
    };
    if !is_regular(our.mode) || !is_regular(their.mode) {
        return Ok(false);
    }
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

/// Tree holding a single blob at the nested path `rel`.
fn tree_with<'r>(
    repo: &'r Repository,
    rel: &Path,
    id: git2::Oid,
    mode: u32,
) -> Result<git2::Tree<'r>> {
    let parts: Vec<&std::ffi::OsStr> = rel.iter().collect();
    let (name, dirs) = parts.split_last().context("empty path")?;
    let mut tb = repo.treebuilder(None)?;
    tb.insert(name, id, mode as i32)?;
    let mut oid = tb.write()?;
    for dir in dirs.iter().rev() {
        let mut tb = repo.treebuilder(None)?;
        tb.insert(dir, oid, 0o040000)?;
        oid = tb.write()?;
    }
    Ok(repo.find_tree(oid)?)
}

/// Three-way merge of one file where conflicting hunks go to the favored side.
/// libgit2 0.19 has no file-level merge API, so merge one-entry trees that use
/// the real path (so `.gitattributes` merge drivers apply). All three trees use
/// the chosen entry's mode so a mode-only difference cannot cause a conflict.
fn merge_hunks(
    repo: &Repository,
    rel: &Path,
    ancestor: Option<&IndexEntry>,
    our: &IndexEntry,
    their: &IndexEntry,
    favor_ours: bool,
) -> Result<Vec<u8>> {
    let mode = if favor_ours { our.mode } else { their.mode };
    let empty =
        || -> Result<git2::Tree<'_>> { Ok(repo.find_tree(repo.treebuilder(None)?.write()?)?) };
    let anc = match ancestor {
        Some(a) => tree_with(repo, rel, a.id, mode)?,
        None => empty()?,
    };
    let mut opts = git2::MergeOptions::new();
    opts.file_favor(if favor_ours {
        git2::FileFavor::Ours
    } else {
        git2::FileFavor::Theirs
    });
    let merged = repo.merge_trees(
        &anc,
        &tree_with(repo, rel, our.id, mode)?,
        &tree_with(repo, rel, their.id, mode)?,
        Some(&opts),
    )?;
    let entry = merged
        .get_path(rel, 0)
        .context("merge of conflicting hunks left the file unresolved")?;
    Ok(repo.find_blob(entry.id)?.content().to_vec())
}

/// What to do for one resolved path. Computing it has no side effects.
enum Plan {
    /// Chosen side has no such file: remove it.
    Remove,
    /// Take this stage-0 entry; `Some(buf)` is merged content to store as its blob.
    Entry(IndexEntry, Option<Vec<u8>>),
}

fn plan_one(
    repo: &Repository,
    c: &git2::IndexConflict,
    rel: &Path,
    favor_ours: bool,
) -> Result<Plan> {
    let chosen = if favor_ours { &c.our } else { &c.their };
    if has_hunks(repo, c)? {
        if let (Some(our), Some(their), Some(chosen)) = (&c.our, &c.their, chosen) {
            let merged = merge_hunks(repo, rel, c.ancestor.as_ref(), our, their, favor_ours)?;
            return Ok(Plan::Entry(stage0(chosen), Some(merged)));
        }
    }
    Ok(match chosen {
        Some(e) => Plan::Entry(stage0(e), None),
        None => Plan::Remove,
    })
}

/// Apply `res`; returns how many files are still conflicted.
///
/// All merges are computed first, then the index is edited and saved, and only
/// then the working tree is updated. A failure before the index is saved
/// leaves both the index on disk and the working tree untouched.
pub fn resolve_files(repo: &Repository, op: OpKind, res: &Resolution) -> Result<usize> {
    let mut index = repo.index()?;
    let mut conflicts = Vec::new();
    for c in index.conflicts()? {
        conflicts.push(c?);
    }

    let mut plans: Vec<(PathBuf, Plan)> = Vec::new();
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
        let Some(side) = side else { continue };
        let rel = PathBuf::from(std::str::from_utf8(&raw).context("non-UTF-8 path in conflict")?);
        let plan = plan_one(repo, c, &rel, op.favors_ours(side))?;
        plans.push((rel, plan));
    }
    if plans.is_empty() {
        return Ok(conflicts.len());
    }

    let edit = |index: &mut git2::Index| -> Result<()> {
        for (rel, plan) in &plans {
            drop_conflict(index, rel);
            match plan {
                Plan::Remove => {}
                Plan::Entry(e, Some(buf)) => index.add_frombuffer(e, buf)?,
                Plan::Entry(e, None) => index.add(e)?,
            }
        }
        index.write()?;
        Ok(())
    };
    if let Err(e) = edit(&mut index) {
        // Drop the half-edited in-memory index.
        let _ = index.read(true);
        return Err(e);
    }

    // Working tree: checkout_index applies filters, modes and symlinks.
    let workdir = repo
        .workdir()
        .context("bare repository has no working tree")?;
    let mut cb = CheckoutBuilder::new();
    cb.force();
    let mut any = false;
    for (rel, plan) in &plans {
        match plan {
            Plan::Remove => match std::fs::remove_file(workdir.join(rel)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            },
            // Gitlinks have no file content to check out.
            Plan::Entry(e, _) if e.mode & S_IFMT == 0o160000 => {}
            Plan::Entry(..) => {
                // checkout would write through an existing symlink; unlink it first.
                let full = workdir.join(rel);
                if std::fs::symlink_metadata(&full).is_ok_and(|m| m.file_type().is_symlink()) {
                    std::fs::remove_file(&full)?;
                }
                cb.path(rel);
                any = true;
            }
        }
    }
    if any {
        repo.checkout_index(Some(&mut index), Some(&mut cb))?;
    }

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
        conflicted_with(|d, i| apply(d, i, cases))
    }

    /// Like `conflicted`, with `setup(dir, 0|1|2)` laying out base/ours/theirs.
    fn conflicted_with(setup: impl Fn(&TempDir, usize)) -> (TempDir, git2::Repository) {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        setup(&dir, 0);
        let base = commit_all(&repo, "base", &[]);
        setup(&dir, 2);
        let theirs = commit_all(&repo, "theirs", &[base]);
        repo.reset(
            repo.find_commit(base).unwrap().as_object(),
            git2::ResetType::Hard,
            None,
        )
        .unwrap();
        setup(&dir, 1);
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
        let idx = repo.index().unwrap();
        assert!(!idx.has_conflicts());
        let e = idx.get_path(std::path::Path::new("f.txt"), 0).unwrap();
        assert_eq!(
            repo.find_blob(e.id).unwrap().content(),
            b"A-ours\nb\nc\nd\nE-theirs\n"
        );
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

    #[cfg(unix)]
    #[test]
    fn exec_bit_is_preserved_from_chosen_side() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, repo) = conflicted_with(|d, i| {
            let body: &[u8] = [&b"1\n2\n"[..], b"ours\n2\n", b"theirs\n2\n"][i];
            write(d, "x.sh", body);
            let mode = if i == 1 { 0o755 } else { 0o644 };
            std::fs::set_permissions(d.path().join("x.sh"), std::fs::Permissions::from_mode(mode))
                .unwrap();
        });
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Local)).unwrap(),
            0
        );
        assert_eq!(read(&dir, "x.sh"), "ours\n2\n");
        let m = std::fs::metadata(dir.path().join("x.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(m & 0o111, 0);
        let idx = repo.index().unwrap();
        assert_eq!(
            idx.get_path(std::path::Path::new("x.sh"), 0).unwrap().mode,
            0o100755
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_conflict_takes_whole_side_as_symlink() {
        let (dir, repo) = conflicted_with(|d, i| {
            let p = d.path().join("l");
            let _ = std::fs::remove_file(&p);
            std::os::unix::fs::symlink(["a", "b", "c"][i], &p).unwrap();
        });
        assert_eq!(
            list_conflicts(&repo).unwrap(),
            vec![ConflictFile {
                path: "l".into(),
                hunks: false
            }]
        );
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Remote)).unwrap(),
            0
        );
        let p = dir.path().join("l");
        assert!(std::fs::symlink_metadata(&p)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_link(&p).unwrap(), std::path::Path::new("c"));
        let idx = repo.index().unwrap();
        assert_eq!(
            idx.get_path(std::path::Path::new("l"), 0).unwrap().mode,
            0o120000
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_in_workdir_is_not_followed() {
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("victim");
        std::fs::write(&target, "keep\n").unwrap();
        let (dir, repo) = conflicted(&[("f.txt", Some(b"base\n"), Some(b"o\n"), Some(b"t\n"))]);
        // Conflicted file replaced by a symlink pointing outside the repo.
        let p = dir.path().join("f.txt");
        std::fs::remove_file(&p).unwrap();
        std::os::unix::fs::symlink(&target, &p).unwrap();
        resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Remote)).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep\n");
        assert_eq!(read(&dir, "f.txt"), "t\n");
    }

    #[test]
    fn gitlink_conflict_does_not_need_commit_in_odb() {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        write(&dir, "keep.txt", b"k\n");
        commit_all(&repo, "base", &[]);
        let mut idx = repo.index().unwrap();
        for (stage, byte) in [(1u16, 1u8), (2, 2), (3, 3)] {
            idx.add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o160000,
                uid: 0,
                gid: 0,
                file_size: 0,
                id: git2::Oid::from_bytes(&[byte; 20]).unwrap(),
                flags: stage << 12,
                flags_extended: 0,
                path: b"sub".to_vec(),
            })
            .unwrap();
        }
        idx.write().unwrap();
        assert_eq!(
            list_conflicts(&repo).unwrap(),
            vec![ConflictFile {
                path: "sub".into(),
                hunks: false
            }]
        );
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Remote)).unwrap(),
            0
        );
        let idx = repo.index().unwrap();
        let e = idx.get_path(std::path::Path::new("sub"), 0).unwrap();
        assert_eq!(e.mode, 0o160000);
        assert_eq!(e.id, git2::Oid::from_bytes(&[3u8; 20]).unwrap());
    }

    #[test]
    fn nested_path_hunk_merge() {
        let (dir, repo) = conflicted_with(|d, i| {
            std::fs::create_dir_all(d.path().join("d/e")).unwrap();
            let body: &[u8] = [BASE, OURS, THEIRS][i];
            write(d, "d/e/f.txt", body);
        });
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Local)).unwrap(),
            0
        );
        assert_eq!(read(&dir, "d/e/f.txt"), "A-ours\nb\nc\nd\nE-theirs\n");
    }

    #[test]
    fn gitattributes_eol_applied_on_checkout() {
        let (dir, repo) = conflicted_with(|d, i| {
            write(d, ".gitattributes", b"*.txt text eol=crlf\n");
            let body: &[u8] = [BASE, OURS, THEIRS][i];
            write(d, "f.txt", body);
        });
        assert_eq!(
            resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Local)).unwrap(),
            0
        );
        let disk = std::fs::read(dir.path().join("f.txt")).unwrap();
        assert_eq!(disk, b"A-ours\r\nb\r\nc\r\nd\r\nE-theirs\r\n");
        let idx = repo.index().unwrap();
        let e = idx.get_path(std::path::Path::new("f.txt"), 0).unwrap();
        assert_eq!(
            repo.find_blob(e.id).unwrap().content(),
            b"A-ours\nb\nc\nd\nE-theirs\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failure_midway_changes_nothing() {
        use std::os::unix::ffi::OsStrExt;
        let (dir, repo) = conflicted_with(|d, i| {
            let body: &[u8] = [&b"x\n"[..], b"o\n", b"t\n"][i];
            write(d, "a.txt", body);
            let bad = std::ffi::OsStr::from_bytes(b"\xff.txt");
            std::fs::write(d.path().join(bad), body).unwrap();
        });
        let before = read(&dir, "a.txt");
        assert!(resolve_files(&repo, OpKind::Merge, &Resolution::All(Side::Remote)).is_err());
        assert_eq!(read(&dir, "a.txt"), before);
        assert!(before.contains("<<<<<<<"));
        assert_eq!(list_conflicts(&repo).unwrap().len(), 2);
        // Saved index is untouched too.
        let fresh = git2::Repository::open(dir.path()).unwrap();
        assert_eq!(list_conflicts(&fresh).unwrap().len(), 2);
    }
}
