//! Regression tests: `torii save` with secret-scanner findings must not
//! hang waiting for the [y/N] prompt when stdin is not a TTY — it fails
//! fast pointing at `--yes`, and `--yes` commits past the findings.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

// Split so torii's own secret scanner doesn't flag this source file —
// the child process under test still sees the assembled key.
const FAKE_AWS_KEY: &str = concat!("aws_access_key_id = ", "AKIA", "IOSFODNN7EXAMPLE");

fn torii(dir: &std::path::Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_torii"));
    c.args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

/// Temp repo with identity configured and a staged-to-be fake secret.
fn repo_with_secret() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let out = torii(tmp.path(), &["init"])
        .stdin(Stdio::null())
        .output()
        .expect("torii init");
    assert!(out.status.success(), "init failed: {:?}", out);
    // Identity via plain .git/config — keeps the test free of git2.
    let cfg = tmp.path().join(".git/config");
    let mut s = std::fs::read_to_string(&cfg).unwrap();
    s.push_str("\n[user]\n\tname = Test\n\temail = test@example.com\n");
    std::fs::write(&cfg, s).unwrap();
    std::fs::write(tmp.path().join("creds.txt"), FAKE_AWS_KEY).unwrap();
    tmp
}

/// Wait up to `secs` for the child; kill + panic if it's still running
/// (that's the old hang reproduced).
fn wait_or_kill(mut child: Child, secs: u64) -> (std::process::ExitStatus, String) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            let mut err = String::new();
            child.stderr.take().unwrap().read_to_string(&mut err).ok();
            return (status, err);
        }
        if Instant::now() > deadline {
            child.kill().ok();
            panic!("torii save hung for {secs}s waiting on a prompt with no TTY");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn save_with_findings_and_piped_stdin_fails_fast_instead_of_hanging() {
    let tmp = repo_with_secret();
    // Stdio::piped() + keeping the handle open = the exact CI/pipe shape
    // that used to block read_line() forever.
    let child = torii(tmp.path(), &["save", "-am", "feat: x"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn torii save");

    let (status, stderr) = wait_or_kill(child, 10);
    assert!(!status.success(), "must refuse to commit, got: {status:?}");
    assert!(
        stderr.contains("--yes"),
        "error should point at --yes, got: {stderr}"
    );
}

#[test]
fn save_with_findings_and_yes_flag_commits() {
    let tmp = repo_with_secret();
    let child = torii(tmp.path(), &["save", "-am", "feat: x", "--yes"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn torii save --yes");

    let (status, stderr) = wait_or_kill(child, 10);
    assert!(
        status.success(),
        "--yes must commit past findings: {stderr}"
    );

    let log = torii(tmp.path(), &["log", "-n", "1"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&log.stdout);
    assert!(
        stdout.contains("feat: x"),
        "commit missing from log: {stdout}"
    );
}
