//! Regression test: `torii log | head` must die quietly via SIGPIPE
//! (exactly like git) instead of panicking with "failed printing to
//! stdout: Broken pipe". See main.rs::reset_sigpipe().
#![cfg(unix)]

use std::process::{Command, Stdio};

#[test]
fn log_into_closed_pipe_dies_by_sigpipe_not_panic() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_torii"))
        .args(["log", "-n", "300"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn torii");

    // Close the read end immediately — the child's next write gets EPIPE,
    // which must terminate it via SIGPIPE (not a Rust panic).
    drop(child.stdout.take());

    let status = child.wait().expect("wait on torii");

    use std::os::unix::process::ExitStatusExt;
    match status.signal() {
        // Killed by a signal: must be SIGPIPE (13).
        Some(sig) => assert_eq!(sig, libc::SIGPIPE, "died by unexpected signal"),
        // Finished before noticing the closed pipe (tiny output): fine,
        // as long as it did NOT panic (panics exit with code 101).
        None => assert_ne!(
            status.code(),
            Some(101),
            "torii panicked on broken pipe — SIGPIPE regression"
        ),
    }

    let mut stderr = String::new();
    use std::io::Read;
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .ok();
    assert!(
        !stderr.contains("panicked"),
        "panic message on stderr: {stderr}"
    );
}
