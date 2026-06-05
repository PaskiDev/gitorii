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

/// What `gpg --verify` reported for a signature. Returned by
/// [`verify`] so callers (CLI, TUI, log-column renderer) can show a
/// status-coloured indicator without each one re-parsing `gpg`
/// stderr.
#[derive(Debug, Clone, PartialEq)]
pub enum VerifyStatus {
    /// `gpg: Good signature` — the key is trusted and the data is
    /// intact.
    Good { signer: String },
    /// Signature is valid but the signing key isn't in the local
    /// keyring (`NO_PUBKEY` / `Can't check signature`). The data
    /// could still be authentic; the user just can't prove it
    /// locally.
    UnknownKey { key_id: Option<String> },
    /// `gpg: BAD signature` — payload was tampered with, or the
    /// signature was made by a different key than what's attached.
    Bad,
    /// Anything else gpg might say: expired key, revoked, agent
    /// errors, …
    Other(String),
}

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
    let bin = resolve_program(program);

    let mut child = Command::new(&bin)
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
                ToriiError::Subprocess { tool: "gpg".into(), message: format!(
                    "gpg binary not found (tried `{}`). Install gpg or \
                     `torii config set git.gpg_program /path/to/gpg2`.",
                    bin
                ) }
            } else {
                ToriiError::Subprocess { tool: "gpg".into(), message: format!("failed to spawn gpg: {}", e) }
            }
        })?;

    {
        let stdin = child.stdin.as_mut()
            .ok_or_else(|| ToriiError::Subprocess { tool: "gpg".into(), message: "gpg stdin unavailable".into() })?;
        stdin.write_all(content)
            .map_err(|e| ToriiError::Subprocess { tool: "gpg".into(), message: format!("writing to gpg stdin: {}", e) })?;
    }

    let output = child.wait_with_output()
        .map_err(|e| ToriiError::Subprocess { tool: "gpg".into(), message: format!("waiting for gpg: {}", e) })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToriiError::Subprocess { tool: "gpg".into(), message: format!(
            "gpg signing failed (exit {}). gpg stderr:\n{}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ) });
    }

    String::from_utf8(output.stdout)
        .map_err(|e| ToriiError::Subprocess { tool: "gpg".into(), message: format!(
            "gpg output was not valid UTF-8: {}", e
        ) })
}

/// Resolve which gpg binary to invoke. Argument (config-supplied)
/// wins; absent that, fall back to `gpg`.
pub fn resolve_program(program: Option<&str>) -> String {
    program
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("gpg")
        .to_string()
}

/// Verify a detached GPG signature against the original signed
/// payload. Mirrors what hosts (GitHub / GitLab) do server-side and
/// what `git verify-commit` does locally.
///
/// Implementation: dump the armor to a tempfile, pipe the payload on
/// stdin, and parse gpg's status output for the verdict. Exit codes
/// alone don't distinguish "bad signature" from "unknown signer" so
/// we scan the status lines too.
pub fn verify(armor: &str, payload: &[u8], program: Option<&str>) -> Result<VerifyStatus> {
    use std::fs::write;
    let bin = resolve_program(program);

    // gpg's "verify a detached sig" form is `gpg --verify <sig> <data>`.
    // We get the data via stdin to avoid juggling two tempfiles.
    let sig_path = std::env::temp_dir().join(format!(
        "torii-verify-{}.asc",
        // Cheap unique tag based on the armor itself; no time/random
        // needed because each verify is short-lived.
        armor.len()
    ));
    write(&sig_path, armor)
        .map_err(|e| ToriiError::Fs(format!("write sig tempfile: {}", e)))?;

    let mut child = Command::new(&bin)
        .args([
            "--status-fd", "1",
            "--no-tty", "--batch",
            "--verify", sig_path.to_str().unwrap_or(""),
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToriiError::Subprocess { tool: "gpg".into(), message: format!(
                    "gpg binary not found (tried `{}`). Set git.gpg_program in config.",
                    bin
                ) }
            } else {
                ToriiError::Subprocess { tool: "gpg".into(), message: format!("failed to spawn gpg: {}", e) }
            }
        })?;
    {
        let stdin = child.stdin.as_mut()
            .ok_or_else(|| ToriiError::Subprocess { tool: "gpg".into(), message: "gpg stdin unavailable".into() })?;
        stdin.write_all(payload)
            .map_err(|e| ToriiError::Fs(format!("writing payload: {}", e)))?;
    }
    let out = child.wait_with_output()
        .map_err(|e| ToriiError::Subprocess { tool: "gpg".into(), message: format!("waiting for gpg: {}", e) })?;

    let _ = std::fs::remove_file(&sig_path);

    let status_lines = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr_lines = String::from_utf8_lossy(&out.stderr).to_string();

    // GPG status-fd lines start with `[GNUPG:] <TAG> …`. We look for
    // the canonical tags first; if none match we fall back to the
    // human stderr which is what `git verify-commit` shows anyway.
    let mut signer: Option<String> = None;
    let mut bad = false;
    let mut no_key: Option<String> = None;
    for line in status_lines.lines() {
        let l = line.trim_start_matches("[GNUPG:] ").trim();
        if let Some(rest) = l.strip_prefix("GOODSIG ") {
            let mut parts = rest.splitn(2, ' ');
            let _keyid = parts.next();
            signer = parts.next().map(|s| s.to_string());
        } else if l.starts_with("BADSIG ") {
            bad = true;
        } else if let Some(rest) = l.strip_prefix("NO_PUBKEY ") {
            no_key = Some(rest.trim().to_string());
        } else if l.starts_with("ERRSIG ") && no_key.is_none() {
            // ERRSIG includes the missing-key case too; extract the
            // long key id from field index 1.
            let parts: Vec<&str> = l.split_whitespace().collect();
            if let Some(k) = parts.get(1) {
                no_key = Some(k.to_string());
            }
        }
    }
    if bad {
        return Ok(VerifyStatus::Bad);
    }
    if let Some(s) = signer {
        return Ok(VerifyStatus::Good { signer: s });
    }
    if let Some(k) = no_key {
        return Ok(VerifyStatus::UnknownKey { key_id: Some(k) });
    }
    // Fall back to the stderr summary so the user still sees
    // something useful (e.g. expired key, revoked, broken keyring).
    Ok(VerifyStatus::Other(stderr_lines.lines().last().unwrap_or("unknown").to_string()))
}
