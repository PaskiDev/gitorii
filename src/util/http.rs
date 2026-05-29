//! Shared HTTP helpers for the platform clients (`pr`, `issue`,
//! `release`, `pipeline`, `package`).
//!
//! Before 0.7.14, each surface had its own boilerplate around `reqwest`:
//! build a client, set user-agent, send, parse status, extract a JSON
//! message field for errors. That repeated across ~15 client structs
//! (GitHub × GitLab × Gitea × 5 surfaces).
//!
//! This module consolidates that into three primitives:
//!
//! - [`make_client`] — builds the `reqwest::blocking::Client` with the
//!   gitorii user-agent. Use everywhere instead of inline `Client::builder()`.
//! - [`send_json`] — runs a `RequestBuilder`, checks status, returns
//!   the parsed JSON body. Folds the three-line send/status/parse
//!   dance into one call.
//! - [`send_empty`] — same, but for operations that don't return a body
//!   we care about (cancel, retry, delete).
//! - [`extract_array`] — turns a JSON value into a `&Vec<Value>` with
//!   a consistent error message when the platform returns a non-array.
//!
//! The `ctx` parameter on `send_json` / `send_empty` is a free-form
//! label that goes into the error message (e.g. `"GitHub"`,
//! `"Gitea (cancel pipeline)"`). Callers include the URL there when it
//! helps debugging.

use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder};
use serde_json::Value;

use crate::error::{Result, ToriiError};

/// User-agent string sent on every platform API call.
pub const USER_AGENT: &str = "gitorii-cli";

/// Per-request hard cap. A platform API that hangs longer than this
/// should fail and surface a clear error instead of freezing torii.
/// 60 s is generous — most API endpoints respond in <2 s; the outlier
/// is GitLab Pipeline list on huge projects which can take 10-15 s.
const REQUEST_TIMEOUT_SECS: u64 = 60;

/// Hard cap on the *connect* phase. If we can't reach the host at all
/// in 10 s, no API call is going to succeed either.
const CONNECT_TIMEOUT_SECS: u64 = 10;

/// Construct the standard blocking HTTP client used by every platform
/// client. Sets a global request timeout so a hung API can't freeze
/// torii forever. Panics only on a build failure we don't expect at
/// runtime (would mean `reqwest` is fundamentally broken).
pub fn make_client() -> Client {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .build()
        .expect("reqwest client build failed")
}

/// Send a request, check status, parse JSON. Returns the parsed value
/// on 2xx; on any other status, returns a formatted `InvalidConfig`
/// error including the platform's own message field when present.
///
/// `ctx` is a short label for the error message ("GitHub", "Gitea
/// retry", etc.) — the caller picks something that disambiguates.
pub fn send_json(req: RequestBuilder, ctx: &str) -> Result<Value> {
    let resp = req.send()
        .map_err(|e| ToriiError::InvalidConfig(format!("{} API error: {}", ctx, e)))?;
    let status = resp.status();
    let body = resp.text()
        .map_err(|e| ToriiError::InvalidConfig(format!("{} API read error: {}", ctx, e)))?;
    let json: Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| serde_json::json!({ "raw_body": body }));
    if !status.is_success() {
        let msg = json.get("message").and_then(|v| v.as_str())
            .or_else(|| json.get("error").and_then(|v| v.as_str()))
            .unwrap_or(if body.is_empty() { "(no message)" } else { &body });
        return Err(ToriiError::InvalidConfig(format!(
            "{} API {}: {}", ctx, status, msg
        )));
    }
    Ok(json)
}

/// Send a request and ignore the response body, only checking status.
/// Used for cancel / retry / delete style operations.
pub fn send_empty(req: RequestBuilder, ctx: &str) -> Result<()> {
    let resp = req.send()
        .map_err(|e| ToriiError::InvalidConfig(format!("{} API error: {}", ctx, e)))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let txt = resp.text().unwrap_or_default();
        return Err(ToriiError::InvalidConfig(format!(
            "{} API {} failed: {}", ctx, s, txt
        )));
    }
    Ok(())
}

/// Extract the top-level array from a JSON value, or fail with a
/// consistent diagnostic that includes the URL/context.
pub fn extract_array<'a>(json: &'a Value, ctx: &str) -> Result<&'a Vec<Value>> {
    json.as_array().ok_or_else(|| ToriiError::InvalidConfig(format!(
        "expected array body for {}, got: {}", ctx, json
    )))
}

/// Send a request and return its body as text. For endpoints like
/// `/jobs/{id}/trace` or `/builds/{id}/log` that return plain text
/// instead of JSON — bypasses [`send_json`]'s `serde_json` parse step.
///
/// Same error shape as [`send_json`]: status check, contextual error
/// message, single point of timeout enforcement.
pub fn send_text(req: RequestBuilder, ctx: &str) -> Result<String> {
    let resp = req.send()
        .map_err(|e| ToriiError::InvalidConfig(format!("{} API error: {}", ctx, e)))?;
    let status = resp.status();
    let body = resp.text()
        .map_err(|e| ToriiError::InvalidConfig(format!("{} API read error: {}", ctx, e)))?;
    if !status.is_success() {
        return Err(ToriiError::InvalidConfig(format!(
            "{} API {}: {}",
            ctx, status,
            if body.is_empty() { "(empty body)" } else { body.lines().next().unwrap_or(&body) }
        )));
    }
    Ok(body)
}

/// Send a request and return its body as raw bytes. For artifact
/// downloads (zip / tarball) — the bytes go straight to disk on the
/// caller's side.
pub fn send_bytes(req: RequestBuilder, ctx: &str) -> Result<Vec<u8>> {
    let resp = req.send()
        .map_err(|e| ToriiError::InvalidConfig(format!("{} API error: {}", ctx, e)))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(ToriiError::InvalidConfig(format!(
            "{} API {}: {}",
            ctx, status,
            if body.is_empty() { "(binary response, empty)" } else { &body }
        )));
    }
    let bytes = resp.bytes()
        .map_err(|e| ToriiError::InvalidConfig(format!("{} API read error: {}", ctx, e)))?;
    Ok(bytes.to_vec())
}
