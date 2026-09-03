//! `torii restore` — put a tracked file back the way it was.
//!
//! The gap this fills: `remove` deletes a file, `clean` deletes untracked
//! ones, `snapshot` saves the whole work-in-progress. None of them undoes an
//! edit to one tracked file, so the only way out was `git checkout --`.
//!
//! Discarding an edit destroys work that was never committed, so — like
//! `worktree remove` — a snapshot is taken first and its id printed.

use std::process::{Command, Stdio};

fn torii(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_torii"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("spawn torii")
}

/// A repo with one committed file, `note.txt`, holding "committed\n".
fn repo_with_a_commit() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    assert!(torii(tmp.path(), &["init"]).status.success());
    let cfg = tmp.path().join(".git/config");
    let mut s = std::fs::read_to_string(&cfg).unwrap();
    s.push_str("\n[user]\n\tname = Test\n\temail = test@example.com\n");
    std::fs::write(&cfg, s).unwrap();

    std::fs::write(tmp.path().join("note.txt"), "committed\n").unwrap();
    let out = torii(tmp.path(), &["save", "-am", "add note"]);
    assert!(out.status.success(), "save failed: {out:?}");
    tmp
}

fn read(tmp: &tempfile::TempDir, name: &str) -> String {
    std::fs::read_to_string(tmp.path().join(name)).unwrap()
}

#[test]
fn restore_undoes_an_edit_to_a_tracked_file() {
    let tmp = repo_with_a_commit();
    std::fs::write(tmp.path().join("note.txt"), "edited by mistake\n").unwrap();

    let out = torii(tmp.path(), &["restore", "note.txt"]);
    assert!(out.status.success(), "restore failed: {out:?}");
    assert_eq!(read(&tmp, "note.txt"), "committed\n");
}

#[test]
fn restore_takes_a_snapshot_before_it_destroys_anything() {
    let tmp = repo_with_a_commit();
    std::fs::write(tmp.path().join("note.txt"), "work worth keeping\n").unwrap();

    let out = torii(tmp.path(), &["restore", "note.txt"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Snapshot") && stdout.contains("torii snapshot restore"),
        "a destructive restore must say how to get the work back: {stdout}"
    );

    let list = torii(tmp.path(), &["snapshot", "list"]);
    let listed = String::from_utf8_lossy(&list.stdout);
    assert!(
        listed.contains("pre-restore"),
        "the snapshot should be findable by name: {listed}"
    );
}

#[test]
fn no_snapshot_skips_the_safety_net() {
    let tmp = repo_with_a_commit();
    std::fs::write(tmp.path().join("note.txt"), "throwaway\n").unwrap();

    let out = torii(tmp.path(), &["restore", "note.txt", "--no-snapshot"]);
    assert!(out.status.success(), "restore failed: {out:?}");
    assert_eq!(read(&tmp, "note.txt"), "committed\n");

    let list = torii(tmp.path(), &["snapshot", "list"]);
    let listed = String::from_utf8_lossy(&list.stdout);
    assert!(
        !listed.contains("pre-restore"),
        "--no-snapshot must take none: {listed}"
    );
}

#[test]
fn staged_unstages_without_touching_the_file() {
    let tmp = repo_with_a_commit();
    std::fs::write(tmp.path().join("note.txt"), "staged edit\n").unwrap();
    assert!(torii(tmp.path(), &["save", "--stage", "note.txt"])
        .status
        .success());

    let out = torii(tmp.path(), &["restore", "--staged", "note.txt"]);
    assert!(out.status.success(), "unstage failed: {out:?}");

    // The edit survives on disk …
    assert_eq!(read(&tmp, "note.txt"), "staged edit\n");
    // … and is no longer in the index.
    let staged = torii(tmp.path(), &["diff", "--staged"]);
    let staged_out = String::from_utf8_lossy(&staged.stdout);
    assert!(
        !staged_out.contains("staged edit"),
        "the change should be out of the index: {staged_out}"
    );
}

#[test]
fn restoring_a_clean_file_is_a_no_op_without_a_snapshot() {
    let tmp = repo_with_a_commit();

    let out = torii(tmp.path(), &["restore", "note.txt"]);
    assert!(out.status.success(), "restore failed: {out:?}");
    assert_eq!(read(&tmp, "note.txt"), "committed\n");

    let list = torii(tmp.path(), &["snapshot", "list"]);
    let listed = String::from_utf8_lossy(&list.stdout);
    assert!(
        !listed.contains("pre-restore"),
        "nothing was at risk, so nothing should have been snapshotted: {listed}"
    );
}

#[test]
fn a_directory_restores_every_tracked_file_under_it() {
    let tmp = repo_with_a_commit();
    std::fs::create_dir(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/a.txt"), "one\n").unwrap();
    std::fs::write(tmp.path().join("src/b.txt"), "two\n").unwrap();
    assert!(torii(tmp.path(), &["save", "-am", "add src"])
        .status
        .success());

    std::fs::write(tmp.path().join("src/a.txt"), "edited\n").unwrap();
    std::fs::write(tmp.path().join("src/b.txt"), "edited\n").unwrap();

    let out = torii(tmp.path(), &["restore", "src", "--no-snapshot"]);
    assert!(out.status.success(), "restore failed: {out:?}");
    assert_eq!(read(&tmp, "src/a.txt"), "one\n");
    assert_eq!(read(&tmp, "src/b.txt"), "two\n");
}

#[test]
fn an_untracked_path_is_refused_by_name() {
    let tmp = repo_with_a_commit();
    std::fs::write(tmp.path().join("scratch.txt"), "never committed\n").unwrap();

    let out = torii(tmp.path(), &["restore", "scratch.txt"]);
    assert!(!out.status.success(), "should refuse: {out:?}");
    let err = String::from_utf8_lossy(&out.stderr) + String::from_utf8_lossy(&out.stdout);
    assert!(
        err.contains("scratch.txt"),
        "the error must name the path: {err}"
    );
    // And the file it does not track is still there.
    assert_eq!(read(&tmp, "scratch.txt"), "never committed\n");
}
