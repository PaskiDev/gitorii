//! GPG signing helper for `torii save` and `torii history reauthor`.
//!
//! Until 0.7.14, `git.sign_commits = true` was silently accepted by the
//! config layer but never honoured at commit time — the bug reported by
//! the user (`commit created without gpgsig header even though config
//! says gpgsign=true`). This module fixes that by shelling out to the
//! local `gpg` binary the same way upstream git does.
//!
//! We deliberately avoid pulling in an in-process OpenPGP library
//! (sequoia / rpgp) for now:
//!
//! - Keeps gitorii's footprint small (no large crypto dep tree).
//! - Reuses the user's existing keyring + agent + pinentry flow. If
//!   they sign with `git commit -S` today, `torii save` works
//!   identically without configuring anything new.
//! - Matches the spec used by hosts (GitHub / GitLab / Codeberg)
//!   exactly — they just verify the ASCII-armored signature attached
//!   as `gpgsig`.
//!
//! Tradeoff: requires `gpg` (or `gpg2`) on PATH. Documented in the
//! error message when missing.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{Result, ToriiError};

/// Sign the given commit content with GPG, returning the ASCII-armored
/// detached signature ready to attach as the `gpgsig` header.
///
/// - `content`: the raw commit object bytes produced by
///   `Repository::commit_create_buffer`.
/// - `key`: the key identifier (long fingerprint or short id) to sign
///   with. Passed to gpg via `-u`.
/// - `program`: which gpg binary to invoke (defaults to `gpg`). Useful
///   on hosts where gpg2 is installed under a different name.
///
/// Errors:
/// - The gpg binary is missing → returns a hint to install it.
/// - The key is unknown to the local keyring → propagates gpg's stderr
///   so the user sees the real reason.
/// - Pinentry failure (locked agent, wrong passphrase) → same.
pub fn sign_blob(content: &[u8], key: &str, program: Option<&str>) -> Result<String> {
    let bin = program.unwrap_or("gpg");

    let mut child = Command::new(bin)
        .args([
            "--detach-sign",
            "--armor",
            "--local-user", key,
            // No status output on stdout — keep the armored signature
            // alone there so we can read it cleanly.
            "--no-tty",
            "--batch",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToriiError::InvalidConfig(format!(
                    "gpg binary not found (tried `{}`). Install gpg or \
                     set `git.gpg_program` in your torii config.",
                    bin
                ))
            } else {
                ToriiError::InvalidConfig(format!("failed to spawn gpg: {}", e))
            }
        })?;

    {
        let stdin = child.stdin.as_mut()
            .ok_or_else(|| ToriiError::InvalidConfig("gpg stdin unavailable".into()))?;
        stdin.write_all(content)
            .map_err(|e| ToriiError::InvalidConfig(format!("writing to gpg stdin: {}", e)))?;
    }

    let output = child.wait_with_output()
        .map_err(|e| ToriiError::InvalidConfig(format!("waiting for gpg: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToriiError::InvalidConfig(format!(
            "gpg signing failed (exit {}). gpg stderr:\n{}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| ToriiError::InvalidConfig(format!(
            "gpg output was not valid UTF-8: {}", e
        )))
}
