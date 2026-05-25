//! Wrapper around the `rad` CLI for the Radicle peer-to-peer host.
//!
//! Radicle is fundamentally different from the other platforms we
//! support:
//!
//! - **No central server.** Every operation is local; `rad` syncs
//!   issues, patches, and refs over the Radicle gossip protocol.
//! - **No HTTP REST.** All interactions go through the `rad` binary on
//!   the user's PATH. We shell out the same way we do for GPG.
//! - **Projects are identified by RIDs** (z-base32 hashes), not
//!   owner/repo paths. The URL parser puts the RID into `owner` and
//!   leaves `repo` empty so the rest of the surface keeps working
//!   without a special case at every call site.
//! - **No CI native.** Patches and issues live in special refs inside
//!   the repo; pipelines / releases / packages have no concept on
//!   Radicle and return clear errors.
//!
//! The `rad` binary lives on the user's PATH. If missing, every
//! Radicle op surfaces the absence of the binary with an install hint.

use std::process::{Command, Stdio};

use serde_json::Value;

use crate::error::{Result, ToriiError};

/// Default `rad` binary name. Overridable in a future release via a
/// `radicle.program` config key (TODO when 0.8.0 lands platforms.toml).
const RAD_BIN: &str = "rad";

/// Run `rad <args>` and return its stdout as UTF-8.
pub fn run_rad(args: &[&str]) -> Result<String> {
    let output = Command::new(RAD_BIN)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToriiError::InvalidConfig(format!(
                    "rad binary not found on PATH. Install radicle from \
                     https://radicle.xyz and re-run."
                ))
            } else {
                ToriiError::InvalidConfig(format!("failed to spawn rad: {}", e))
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToriiError::InvalidConfig(format!(
            "rad {} failed (exit {}):\n{}",
            args.join(" "),
            output.status.code().unwrap_or(-1),
            stderr.trim()
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| ToriiError::InvalidConfig(format!("rad output not UTF-8: {}", e)))
}

/// Run `rad <args>` with `--json` appended and parse the response.
/// Useful for the new (1.0+) JSON-formatted output. For older `rad`
/// versions that don't support `--json`, callers fall back to
/// parsing the text output of `run_rad`.
pub fn run_rad_json(args: &[&str]) -> Result<Value> {
    let mut argv: Vec<&str> = args.to_vec();
    argv.push("--format");
    argv.push("json");
    let stdout = run_rad(&argv)?;
    // `rad` emits one JSON value per line for list endpoints; collect
    // them into an array. Single-value commands emit a single object.
    let trimmed = stdout.trim();
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        serde_json::from_str(trimmed)
            .map_err(|e| ToriiError::InvalidConfig(format!("rad JSON parse: {}", e)))
    } else {
        // NDJSON: one object per line.
        let mut items = Vec::new();
        for line in trimmed.lines() {
            if line.trim().is_empty() { continue; }
            let v: Value = serde_json::from_str(line)
                .map_err(|e| ToriiError::InvalidConfig(format!("rad NDJSON parse: {}", e)))?;
            items.push(v);
        }
        Ok(Value::Array(items))
    }
}
