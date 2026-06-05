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
/// on 2xx; on any other status, returns a `PlatformApi`
/// error including the platform's own message field when present.
///
/// `ctx` is a short label for the error message ("GitHub", "Gitea
/// retry", etc.) — the caller picks something that disambiguates.
pub fn send_json(req: RequestBuilder, ctx: &str) -> Result<Value> {
    let resp = req.send().map_err(|e| ToriiError::Network {
        provider: ctx.to_string(),
        message: e.to_string(),
    })?;
    let status = resp.status();
    let body = resp.text().map_err(|e| ToriiError::Network {
        provider: ctx.to_string(),
        message: format!("read error: {}", e),
    })?;
    let json: Value =
        serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({ "raw_body": body }));
    if !status.is_success() {
        let msg = json
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| json.get("error").and_then(|v| v.as_str()))
            .unwrap_or(if body.is_empty() {
                "(no message)"
            } else {
                &body
            });
        return Err(ToriiError::PlatformApi {
            provider: ctx.to_string(),
            status: status.as_u16(),
            message: msg.to_string(),
        });
    }
    Ok(json)
}

/// Send a request and ignore the response body, only checking status.
/// Used for cancel / retry / delete style operations.
pub fn send_empty(req: RequestBuilder, ctx: &str) -> Result<()> {
    let resp = req.send().map_err(|e| ToriiError::Network {
        provider: ctx.to_string(),
        message: e.to_string(),
    })?;
    if !resp.status().is_success() {
        let s = resp.status();
        let txt = resp.text().unwrap_or_default();
        return Err(ToriiError::PlatformApi {
            provider: ctx.to_string(),
            status: s.as_u16(),
            message: txt,
        });
    }
    Ok(())
}

/// Extract the top-level array from a JSON value, or fail with a
/// consistent diagnostic that includes the URL/context.
pub fn extract_array<'a>(json: &'a Value, ctx: &str) -> Result<&'a Vec<Value>> {
    json.as_array()
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: ctx.to_string(),
            message: format!("expected array body, got: {}", json),
        })
}

/// Send a request and return its body as text. For endpoints like
/// `/jobs/{id}/trace` or `/builds/{id}/log` that return plain text
/// instead of JSON — bypasses [`send_json`]'s `serde_json` parse step.
///
/// Same error shape as [`send_json`]: status check, contextual error
/// message, single point of timeout enforcement.
pub fn send_text(req: RequestBuilder, ctx: &str) -> Result<String> {
    let resp = req.send().map_err(|e| ToriiError::Network {
        provider: ctx.to_string(),
        message: e.to_string(),
    })?;
    let status = resp.status();
    let body = resp.text().map_err(|e| ToriiError::Network {
        provider: ctx.to_string(),
        message: format!("read error: {}", e),
    })?;
    if !status.is_success() {
        return Err(ToriiError::PlatformApi {
            provider: ctx.to_string(),
            status: status.as_u16(),
            message: if body.is_empty() {
                "(empty body)".to_string()
            } else {
                body.lines().next().unwrap_or(&body).to_string()
            },
        });
    }
    Ok(body)
}

/// Send a request and return its body as raw bytes. For artifact
/// downloads (zip / tarball) — the bytes go straight to disk on the
/// caller's side.
pub fn send_bytes(req: RequestBuilder, ctx: &str) -> Result<Vec<u8>> {
    let resp = req.send().map_err(|e| ToriiError::Network {
        provider: ctx.to_string(),
        message: e.to_string(),
    })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(ToriiError::PlatformApi {
            provider: ctx.to_string(),
            status: status.as_u16(),
            message: if body.is_empty() {
                "(binary response, empty)".to_string()
            } else {
                body
            },
        });
    }
    let bytes = resp.bytes().map_err(|e| ToriiError::Network {
        provider: ctx.to_string(),
        message: format!("read error: {}", e),
    })?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ToriiError;
    use httpmock::prelude::*;

    #[test]
    fn send_json_returns_parsed_body_on_2xx() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET).path("/ok");
            then.status(200)
                .json_body(serde_json::json!({ "id": 7, "name": "torii" }));
        });
        let json = send_json(make_client().get(server.url("/ok")), "Test").unwrap();
        m.assert();
        assert_eq!(json["id"], 7);
        assert_eq!(json["name"], "torii");
    }

    #[test]
    fn send_json_maps_non_2xx_to_platform_api_with_status_and_message() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/missing");
            then.status(404)
                .json_body(serde_json::json!({ "message": "Not Found" }));
        });
        let err = send_json(make_client().get(server.url("/missing")), "Test").unwrap_err();
        match err {
            ToriiError::PlatformApi {
                provider,
                status,
                message,
            } => {
                assert_eq!(provider, "Test");
                assert_eq!(status, 404);
                assert_eq!(message, "Not Found");
            }
            other => panic!("expected PlatformApi, got: {other:?}"),
        }
    }

    #[test]
    fn send_json_maps_transport_failure_to_network() {
        // Port 1 has no listener — immediate connection refused.
        let err = send_json(make_client().get("http://127.0.0.1:1/x"), "Test").unwrap_err();
        assert!(
            matches!(err, ToriiError::Network { ref provider, .. } if provider == "Test"),
            "expected Network, got: {err:?}"
        );
    }

    #[test]
    fn send_empty_ok_on_2xx_platform_api_on_failure() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/del");
            then.status(204);
        });
        server.mock(|when, then| {
            when.method(POST).path("/forbidden");
            then.status(403).body("nope");
        });
        assert!(send_empty(make_client().post(server.url("/del")), "Test").is_ok());
        let err = send_empty(make_client().post(server.url("/forbidden")), "Test").unwrap_err();
        match err {
            ToriiError::PlatformApi {
                status, message, ..
            } => {
                assert_eq!(status, 403);
                assert_eq!(message, "nope");
            }
            other => panic!("expected PlatformApi, got: {other:?}"),
        }
    }

    #[test]
    fn extract_array_rejects_non_array_as_malformed_response() {
        let json = serde_json::json!({ "values": [] });
        let err = extract_array(&json, "Test").unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { ref provider, .. } if provider == "Test"),
            "expected MalformedResponse, got: {err:?}"
        );
        let arr_json = serde_json::json!([1, 2]);
        assert_eq!(extract_array(&arr_json, "Test").unwrap().len(), 2);
    }

    #[test]
    fn send_text_and_send_bytes_return_raw_bodies() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/log");
            then.status(200).body("line1\nline2\n");
        });
        let text = send_text(make_client().get(server.url("/log")), "Test").unwrap();
        assert_eq!(text, "line1\nline2\n");
        let bytes = send_bytes(make_client().get(server.url("/log")), "Test").unwrap();
        assert_eq!(bytes, b"line1\nline2\n");
    }
}
