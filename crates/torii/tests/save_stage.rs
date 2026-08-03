//! `torii save --stage` — stage paths without committing, so they can be
//! inspected (`torii scan`, `torii diff --staged`) before deciding to
//! commit or back out with the existing `torii save --unstage`.
//!
//! Before this flag existed, the only way to leave something staged for
//! inspection was `git add -N`, which records intent-to-add but not the
//! actual content — so a scanner reading the index sees nothing there.

use std::process::{Command, Stdio};

fn torii(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_torii"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("spawn torii")
}

fn init_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let out = torii(tmp.path(), &["init"]);
    assert!(out.status.success(), "init failed: {:?}", out);
    let cfg = tmp.path().join(".git/config");
    let mut s = std::fs::read_to_string(&cfg).unwrap();
    s.push_str("\n[user]\n\tname = Test\n\temail = test@example.com\n");
    std::fs::write(&cfg, s).unwrap();
    tmp
}

#[test]
fn stage_puts_content_in_the_index_without_committing() {
    let tmp = init_repo();
    std::fs::write(tmp.path().join("new.txt"), "hello\n").unwrap();

    let out = torii(tmp.path(), &["save", "--stage", "new.txt"]);
    assert!(out.status.success(), "stage failed: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Staged"), "unexpected output: {stdout}");

    // Staged, not committed — `torii log` has nothing yet.
    let log = torii(tmp.path(), &["log", "-n", "1"]);
    // On a repo with no commits yet, log should not mention new.txt content
    // as a commit subject.
    let log_out = String::from_utf8_lossy(&log.stdout) + String::from_utf8_lossy(&log.stderr);
    assert!(
        !log_out.contains("new.txt"),
        "nothing should be committed yet: {log_out}"
    );

    // `torii diff --staged` must show the content is actually in the index
    // (this is the exact gap `git add -N` leaves: intent registered, no
    // content, so a diff/scan against the index sees an empty blob).
    let diff = torii(tmp.path(), &["diff", "--staged"]);
    let diff_out = String::from_utf8_lossy(&diff.stdout);
    assert!(
        diff_out.contains("hello"),
        "staged content should be visible in `diff --staged`: {diff_out}"
    );

    // `torii status` reports it under staged changes.
    let status = torii(tmp.path(), &["status"]);
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_out.contains("staged for commit") && status_out.contains("new.txt"),
        "status should list new.txt as staged: {status_out}"
    );
}

#[test]
fn stage_is_reversible_with_existing_unstage() {
    let tmp = init_repo();
    std::fs::write(tmp.path().join("a.txt"), "a\n").unwrap();

    let out = torii(tmp.path(), &["save", "--stage", "a.txt"]);
    assert!(out.status.success(), "stage failed: {:?}", out);

    let out = torii(tmp.path(), &["save", "--unstage", "a.txt"]);
    assert!(out.status.success(), "unstage failed: {:?}", out);

    let status = torii(tmp.path(), &["status"]);
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(
        !status_out.contains("staged for commit"),
        "a.txt should no longer be staged after --unstage: {status_out}"
    );
    // File itself must still be on disk (unstage never deletes).
    assert!(tmp.path().join("a.txt").exists());
}

#[test]
fn stage_all_stages_every_new_file() {
    let tmp = init_repo();
    std::fs::write(tmp.path().join("one.txt"), "1\n").unwrap();
    std::fs::write(tmp.path().join("two.txt"), "2\n").unwrap();

    let out = torii(tmp.path(), &["save", "--stage", "--all"]);
    assert!(out.status.success(), "stage --all failed: {:?}", out);

    let status = torii(tmp.path(), &["status"]);
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(status_out.contains("one.txt"), "{status_out}");
    assert!(status_out.contains("two.txt"), "{status_out}");
}

#[test]
fn stage_without_files_or_all_fails_with_a_clear_error() {
    let tmp = init_repo();
    let out = torii(tmp.path(), &["save", "--stage"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("at least one path") || stderr.contains("--all"),
        "error should explain how to fix it: {stderr}"
    );
}

#[test]
fn staged_content_can_later_be_committed_without_repeating_the_paths() {
    let tmp = init_repo();
    std::fs::write(tmp.path().join("b.txt"), "b\n").unwrap();

    let out = torii(tmp.path(), &["save", "--stage", "b.txt"]);
    assert!(out.status.success(), "stage failed: {:?}", out);

    // No FILES, no --all — commits exactly what was staged earlier.
    let out = torii(tmp.path(), &["save", "-m", "feat: add b"]);
    assert!(
        out.status.success(),
        "commit of staged content failed: {:?}",
        out
    );

    let log = torii(tmp.path(), &["log", "-n", "1"]);
    let log_out = String::from_utf8_lossy(&log.stdout);
    assert!(log_out.contains("feat: add b"), "{log_out}");
}
