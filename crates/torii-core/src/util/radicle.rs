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
                ToriiError::Subprocess {
                    tool: "rad".into(),
                    message: "rad binary not found on PATH. Install radicle from \
                     https://radicle.xyz and re-run."
                        .to_string(),
                }
            } else {
                ToriiError::Subprocess {
                    tool: "rad".into(),
                    message: format!("failed to spawn rad: {}", e),
                }
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToriiError::Subprocess {
            tool: "rad".into(),
            message: format!(
                "rad {} failed (exit {}):\n{}",
                args.join(" "),
                output.status.code().unwrap_or(-1),
                stderr.trim()
            ),
        });
    }

    String::from_utf8(output.stdout).map_err(|e| ToriiError::Subprocess {
        tool: "rad".into(),
        message: format!("rad output not UTF-8: {}", e),
    })
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
        serde_json::from_str(trimmed).map_err(|e| ToriiError::MalformedResponse {
            provider: "radicle".into(),
            message: format!("rad JSON parse: {}", e),
        })
    } else {
        // NDJSON: one object per line.
        let mut items = Vec::new();
        for line in trimmed.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value =
                serde_json::from_str(line).map_err(|e| ToriiError::MalformedResponse {
                    provider: "radicle".into(),
                    message: format!("rad NDJSON parse: {}", e),
                })?;
            items.push(v);
        }
        Ok(Value::Array(items))
    }
}

/// A Radicle URN (`rad:z…`) or the `rad://` URL form, as opposed to a
/// git URL libgit2 can clone.
pub fn is_radicle_url(s: &str) -> bool {
    let s = s.trim();
    s.starts_with("rad:") || s.starts_with("rad://")
}

/// Canonical `rad:<rid>` from `rad:z…`, `rad://z…[/nid]`, or a bare RID.
pub fn normalize_rid(s: &str) -> Result<String> {
    let s = s.trim();
    if s.is_empty() {
        return Err(ToriiError::InvalidConfig(
            "Radicle RID is empty. Expected `rad:z…`.".into(),
        ));
    }
    let rest = s
        .strip_prefix("rad://")
        .or_else(|| s.strip_prefix("rad:"))
        .unwrap_or(s);
    let rid = rest.split('/').next().unwrap_or(rest).trim();
    if rid.is_empty() {
        return Err(ToriiError::InvalidConfig(format!(
            "Radicle RID is empty in `{s}`. Expected `rad:z…`."
        )));
    }
    Ok(format!("rad:{rid}"))
}

/// `rad clone <rid> [<dest>]`. Blocks until the node has fetched and
/// checked out. The node itself is the auth; there is no token.
pub fn rad_clone(rid: &str, dest: Option<&str>) -> Result<()> {
    rad_clone_cancelable(rid, dest, None)
}

/// Same as [`rad_clone`], but the caller can raise `cancel` to kill the
/// `rad` process. A stop during fetch leaves whatever `rad` had already
/// written in storage; the working copy is not guaranteed to exist.
pub fn rad_clone_cancelable(
    rid: &str,
    dest: Option<&str>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<()> {
    let rid = normalize_rid(rid)?;
    let dest_owned = dest.map(str::to_string);
    let mut args: Vec<&str> = vec!["clone", rid.as_str()];
    if let Some(d) = dest_owned.as_deref() {
        args.push(d);
    }

    let Some(flag) = cancel else {
        let _ = run_rad(&args)?;
        return Ok(());
    };

    // Stdio is discarded: piping it without a reader deadlocks when
    // `rad` fills the buffer, and inheriting it would write over the TUI.
    let mut child = Command::new(RAD_BIN)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToriiError::Subprocess {
                    tool: "rad".into(),
                    message: "rad binary not found on PATH. Install radicle from \
                     https://radicle.xyz and re-run."
                        .to_string(),
                }
            } else {
                ToriiError::Subprocess {
                    tool: "rad".into(),
                    message: format!("failed to spawn rad: {}", e),
                }
            }
        })?;

    loop {
        if flag.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToriiError::Subprocess {
                tool: "rad".into(),
                message: "rad clone stopped".into(),
            });
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(ToriiError::Subprocess {
                    tool: "rad".into(),
                    message: format!(
                        "rad {} failed (exit {})",
                        args.join(" "),
                        status.code().unwrap_or(-1)
                    ),
                });
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => {
                return Err(ToriiError::Subprocess {
                    tool: "rad".into(),
                    message: format!("rad clone wait: {e}"),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radicle_urls_are_the_urn_and_the_url_form() {
        assert!(is_radicle_url("rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5"));
        assert!(is_radicle_url(" rad://z3gqcJUoA1n9HaHKufZs5FCSGazv5/nid "));
        assert!(!is_radicle_url("https://github.com/u/r"));
        assert!(!is_radicle_url("radicle"));
    }

    #[test]
    fn normalize_rid_accepts_bare_and_prefixed() {
        assert_eq!(
            normalize_rid("z3gqcJUoA1n9HaHKufZs5FCSGazv5").unwrap(),
            "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5"
        );
        assert_eq!(
            normalize_rid("rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5").unwrap(),
            "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5"
        );
        assert_eq!(
            normalize_rid("rad://z3gqcJUoA1n9HaHKufZs5FCSGazv5/z6Mkabc").unwrap(),
            "rad:z3gqcJUoA1n9HaHKufZs5FCSGazv5"
        );
        assert!(normalize_rid("").is_err());
        assert!(normalize_rid("rad:").is_err());
    }
}
