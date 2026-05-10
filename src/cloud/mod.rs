//! Authenticated HTTP client for the gitorii.com backend.
//!
//! Every call carries `Authorization: Bearer gitorii_sk_<key>` from
//! `crate::auth`. Translates HTTP status into actionable `ToriiError`s the
//! CLI can surface verbatim:
//!   401  → "API key rejected. Run `torii auth login` to refresh."
//!   402  → "current plan <p> insufficient. Upgrade at <url>."
//!   403  → "organization suspended."
//!   5xx  → "server error: …".

pub mod whoami;

use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::AUTHORIZATION;
use std::time::Duration;

use crate::auth::ApiKey;
use crate::error::{Result, ToriiError};

const UA: &str = concat!("torii/", env!("CARGO_PKG_VERSION"));

pub struct CloudClient {
    http: Client,
    key: ApiKey,
}

impl CloudClient {
    pub fn new(key: ApiKey) -> Self {
        let http = Client::builder()
            .user_agent(UA)
            .timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client builds");
        Self { http, key }
    }

    pub fn endpoint(&self) -> &str {
        &self.key.endpoint
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.key.endpoint.trim_end_matches('/'), path)
    }

    fn get(&self, path: &str) -> RequestBuilder {
        self.http
            .get(self.url(path))
            .header(AUTHORIZATION, format!("Bearer {}", self.key.key))
    }

    #[allow(dead_code)] // used by future endpoints (transpile etc.)
    fn post(&self, path: &str) -> RequestBuilder {
        self.http
            .post(self.url(path))
            .header(AUTHORIZATION, format!("Bearer {}", self.key.key))
    }
}

/// Convert HTTP status into a friendly error before the caller sees raw body.
/// On 200..=299 returns the response unchanged.
pub(crate) fn check_status(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().unwrap_or_default();
    let msg = match status.as_u16() {
        401 => "API key rejected. Run `torii auth login` to refresh.".to_string(),
        402 => format!(
            "your plan does not include this feature. Upgrade at https://gitorii.com/upgrade ({})",
            short_body(&body)
        ),
        403 => "organization suspended. Contact support@gitorii.com.".to_string(),
        404 => "endpoint not found — CLI may be outdated.".to_string(),
        s if (500..=599).contains(&s) => format!("server error {}: {}", s, short_body(&body)),
        s => format!("unexpected HTTP {}: {}", s, short_body(&body)),
    };
    Err(ToriiError::InvalidConfig(msg))
}

fn short_body(body: &str) -> String {
    let trimmed = body.trim();
    // Cap by chars (not bytes) so multi-byte UTF-8 server messages don't
    // panic on the slice. 200 chars ≈ enough to debug an API rejection.
    if trimmed.chars().count() > 200 {
        let head: String = trimmed.chars().take(200).collect();
        format!("{}…", head)
    } else {
        trimmed.to_string()
    }
}
