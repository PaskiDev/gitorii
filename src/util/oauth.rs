//! OAuth 2.0 Device Authorization Grant (RFC 8628) — the flow CLIs use
//! to avoid asking the user to copy-paste a Personal Access Token.
//!
//! User experience:
//!   1. torii prints a short code + the verification URL.
//!   2. User opens the URL in any browser (no callback required), types
//!      the code, authorises.
//!   3. torii polls the token endpoint every `interval` seconds until
//!      the server returns `access_token` (success) or `expired_token` /
//!      `access_denied` (fail).
//!
//! The same shape works on GitHub, GitLab, Codeberg/Gitea/Forgejo. Each
//! one needs a registered OAuth App so torii has a `client_id` to send
//! — those are listed in [`device_flow_provider`].
//!
//! Bitbucket Cloud does **not** implement RFC 8628 — it only supports
//! Authorization Code Grant, which needs a `localhost:PORT` callback.
//! That flow is tracked separately and lives next to this module when
//! implemented; for now Bitbucket continues to ask for an app password
//! via `torii auth set bitbucket USERNAME:APP_PASSWORD`.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::error::{Result, ToriiError};

/// Per-provider URLs + scopes for the device-flow request. Add new
/// entries here; nothing else needs to change to wire a new provider
/// (assuming it implements RFC 8628).
struct DeviceFlowProvider {
    /// Where to POST the initial device-code request.
    device_authz_url: &'static str,
    /// Where to POST the token poll.
    token_url: &'static str,
    /// Default OAuth scopes string sent with the device-code request.
    scopes: &'static str,
    /// Env var the user can set to override the bundled client_id at
    /// runtime — useful for self-hosted Gitea/Forgejo where the user
    /// registers their own OAuth app.
    client_id_env: &'static str,
    /// Bundled (public) client_id. **Has to be filled in once an OAuth
    /// app is registered on each platform**; until then we fall back
    /// to the env var and bail with a helpful error if it's missing.
    bundled_client_id: Option<&'static str>,
}

fn device_flow_provider(provider: &str) -> Option<DeviceFlowProvider> {
    // Once the Torii project owner registers OAuth apps on each
    // platform, the `bundled_client_id` slot stops being `None` and
    // the env var becomes optional.
    match provider {
        "github" => Some(DeviceFlowProvider {
            device_authz_url:  "https://github.com/login/device/code",
            token_url:         "https://github.com/login/oauth/access_token",
            scopes:            "repo read:org workflow",
            client_id_env:     "TORII_GITHUB_CLIENT_ID",
            bundled_client_id: None,
        }),
        "gitlab" => Some(DeviceFlowProvider {
            device_authz_url:  "https://gitlab.com/oauth/authorize_device",
            token_url:         "https://gitlab.com/oauth/token",
            scopes:            "api",
            client_id_env:     "TORII_GITLAB_CLIENT_ID",
            bundled_client_id: None,
        }),
        // Codeberg / Gitea / Forgejo share the Gitea OAuth surface; the
        // device-flow endpoints are at the platform host.
        "codeberg" => Some(DeviceFlowProvider {
            device_authz_url:  "https://codeberg.org/login/oauth/device/code",
            token_url:         "https://codeberg.org/login/oauth/access_token",
            scopes:            "",
            client_id_env:     "TORII_CODEBERG_CLIENT_ID",
            bundled_client_id: None,
        }),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code:               String,
    user_code:                 String,
    verification_uri:          String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in:                u64,
    #[serde(default = "default_interval")]
    interval:                  u64,
}

fn default_interval() -> u64 { 5 }

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TokenResponse {
    Success { access_token: String, #[serde(default)] #[allow(dead_code)] token_type: Option<String> },
    Error   { error: String, #[serde(default)] error_description: Option<String> },
}

/// Run the device flow for `provider`. Blocks until the user
/// authorises (success) or the device code expires (failure).
/// Returns the access token, ready to hand to
/// [`crate::auth::set_token`].
pub fn run_device_flow(provider: &str) -> Result<String> {
    let cfg = device_flow_provider(provider).ok_or_else(|| ToriiError::InvalidConfig(format!(
        "OAuth device flow not configured for `{}`. Supported: github, gitlab, codeberg. \
         Bitbucket needs the (separate) Authorization Code flow.", provider
    )))?;

    let client_id = std::env::var(cfg.client_id_env).ok()
        .or_else(|| cfg.bundled_client_id.map(String::from))
        .ok_or_else(|| ToriiError::InvalidConfig(format!(
            "No OAuth client_id available for `{}`. Set the {} env var, or wait until the \
             bundled client_id ships in a future torii release. As a workaround, create a \
             Personal Access Token in the platform's web UI and run: \
             torii auth set {} YOUR_TOKEN",
            provider, cfg.client_id_env, provider
        )))?;

    let client = crate::http::make_client();

    // Step 1: request device + user codes.
    let init_req = client.post(cfg.device_authz_url)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id.as_str()),
            ("scope",     cfg.scopes),
        ]);
    let init: DeviceCodeResponse = crate::http::send_json(init_req, "OAuth device init")
        .and_then(|v| serde_json::from_value(v).map_err(|e| ToriiError::InvalidConfig(
            format!("OAuth device init: cannot parse response: {}", e)
        )))?;

    let display_uri = init.verification_uri_complete.as_deref().unwrap_or(&init.verification_uri);
    println!();
    println!("⛩  Open this URL in your browser:");
    println!("   {}", display_uri);
    if init.verification_uri_complete.is_none() {
        // Only print the code separately when the URL doesn't already
        // embed it — otherwise the user copies a code that's also in
        // the link, which is noisy.
        println!();
        println!("   And enter the code: {}", init.user_code);
    }
    println!();
    println!("Waiting for authorisation… (Ctrl-C to abort)");

    // Step 2: poll the token endpoint.
    let mut interval = Duration::from_secs(init.interval.max(1));
    let deadline = Instant::now() + Duration::from_secs(init.expires_in.max(60));
    loop {
        std::thread::sleep(interval);
        if Instant::now() >= deadline {
            return Err(ToriiError::InvalidConfig(
                "OAuth device flow: code expired before authorisation. Run the command again.".to_string()
            ));
        }

        let poll_req = client.post(cfg.token_url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id",   client_id.as_str()),
                ("device_code", init.device_code.as_str()),
                ("grant_type",  "urn:ietf:params:oauth:grant-type:device_code"),
            ]);

        // We bypass send_json here because the token endpoint returns
        // 200 for "still pending" responses — only the body
        // distinguishes success from in-flight, so the standard
        // is_success() check would mis-handle the error variants.
        let resp = poll_req.send()
            .map_err(|e| ToriiError::InvalidConfig(format!("OAuth poll: {}", e)))?;
        let body: TokenResponse = resp.json()
            .map_err(|e| ToriiError::InvalidConfig(format!("OAuth poll: malformed JSON: {}", e)))?;
        match body {
            TokenResponse::Success { access_token, .. } => {
                println!("✅ Authorised. Token saved.");
                return Ok(access_token);
            }
            TokenResponse::Error { error, error_description } => match error.as_str() {
                "authorization_pending" => continue,
                "slow_down" => {
                    interval += Duration::from_secs(5);
                    continue;
                }
                "expired_token" => return Err(ToriiError::InvalidConfig(
                    "OAuth device flow: code expired. Run the command again.".to_string()
                )),
                "access_denied" => return Err(ToriiError::InvalidConfig(
                    "OAuth device flow: user denied authorisation.".to_string()
                )),
                other => return Err(ToriiError::InvalidConfig(format!(
                    "OAuth device flow error '{}': {}",
                    other, error_description.unwrap_or_default()
                ))),
            }
        }
    }
}

/// Whether the given provider has device flow wired (without checking
/// whether a client_id is actually available — that's the runtime
/// concern of [`run_device_flow`]).
pub fn device_flow_supported(provider: &str) -> bool {
    device_flow_provider(provider).is_some()
}
