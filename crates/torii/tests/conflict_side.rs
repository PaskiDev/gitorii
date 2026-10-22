//! End-to-end: `--local` / `--remote` and the no-tty fallback on sync and cherry-pick.

use std::path::Path;
use std::process::{Command, Output, Stdio};

struct Fixture {
    dir: tempfile::TempDir,
    home: tempfile::TempDir,
    theirs: git2::Oid,
}

/// Repo where HEAD ("local") and `origin/main` ("theirs") both change f.txt.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();

    let write_commit = |content: &str, msg: &str, parent: Option<&git2::Commit>, update: Option<&str>| {
        std::fs::write(dir.path().join("f.txt"), content).unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(Path::new("f.txt")).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let parents: Vec<&git2::Commit> = parent.into_iter().collect();
        repo.commit(update, &sig, &sig, msg, &tree, &parents).unwrap()
    };

    let base = write_commit("base\n", "base", None, Some("HEAD"));
    let base = repo.find_commit(base).unwrap();
    let theirs = write_commit("theirs\n", "theirs", Some(&base), None);
    repo.reference("refs/remotes/origin/main", theirs, true, "test").unwrap();
    write_commit("local\n", "local", Some(&base), Some("HEAD"));
    drop(base);
    Fixture { dir, home, theirs }
}

fn torii(fx: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_torii"))
        .args(args)
        .current_dir(fx.dir.path())
        .env("HOME", fx.home.path())
        .env("XDG_CONFIG_HOME", fx.home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run torii")
}

fn f_txt(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.dir.path().join("f.txt")).unwrap()
}

#[test]
fn merge_without_tty_or_flag_leaves_markers() {
    let fx = fixture();
    let out = torii(&fx, &["sync", "origin/main", "--merge"]);
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("✅ Merged"), "{stdout}");
    assert!(f_txt(&fx).contains("<<<<<<<"), "{}", f_txt(&fx));
}

#[test]
fn merge_with_remote_flag_resolves_and_commits() {
    let fx = fixture();
    let out = torii(&fx, &["sync", "origin/main", "--merge", "--remote"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(f_txt(&fx), "theirs\n");
    let repo = git2::Repository::open(fx.dir.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2);
}

#[test]
fn cherry_pick_with_local_flag_keeps_head_content() {
    let fx = fixture();
    let hash = fx.theirs.to_string();
    let out = torii(&fx, &["cherry-pick", &hash, "--local"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(f_txt(&fx), "local\n");
    assert!(!f_txt(&fx).contains("<<<<<<<"));
}
