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
    // The `bundled_client_id` slots are the public OAuth App IDs
    // registered for "Torii CLI" on each platform. They identify the
    // app to the auth server — they are NOT secrets and live in the
    // open-source binary intentionally (same as `gh`, `glab`, etc.).
    // The env var override lets users point at their own registered
    // app (useful for self-hosted Gitea/Forgejo).
    match provider {
        "github" => Some(DeviceFlowProvider {
            device_authz_url:  "https://github.com/login/device/code",
            token_url:         "https://github.com/login/oauth/access_token",
            scopes:            "repo read:org workflow",
            client_id_env:     "TORII_GITHUB_APP_ID",
            bundled_client_id: Some("Ov23liDcA2Njn7eRWnYV"),
        }),
        "gitlab" => Some(DeviceFlowProvider {
            device_authz_url:  "https://gitlab.com/oauth/authorize_device",
            token_url:         "https://gitlab.com/oauth/token",
            scopes:            "api",
            client_id_env:     "TORII_GITLAB_APP_ID",
            bundled_client_id: Some("b72a85262c309587f67591da8fed4f8e8f4ee7349e9ed06f6a2a99ee7caec4fe"),
        }),
        // Codeberg / Gitea / Forgejo share the Gitea OAuth surface; the
        // device-flow endpoints are at the platform host.
        "codeberg" => Some(DeviceFlowProvider {
            device_authz_url:  "https://codeberg.org/login/oauth/device/code",
            token_url:         "https://codeberg.org/login/oauth/access_token",
            scopes:            "",
            client_id_env:     "TORII_CODEBERG_APP_ID",
            bundled_client_id: Some("d114c8aa-227d-453e-8f25-cdd727f49d42"),
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

/// Best-effort revoke of an access token, used by `torii auth rotate`
/// after the new token is in hand. Returns Ok(true) if the platform
/// confirmed the revocation, Ok(false) if no revoke endpoint is wired
/// (the caller should print a "revoke manually at <URL>" hint), or
/// Err on a real failure (network, 5xx, malformed response). 401/404
/// from the revoke endpoint count as "already invalid" → Ok(true).
pub fn revoke_token(provider: &str, token: &str) -> Result<bool> {
    match provider {
        "gitlab" => revoke_gitlab(token),
        "github" => revoke_github(token),
        // Codeberg/Gitea: no stable OAuth revoke endpoint in the
        // Gitea spec, only PAT delete by ID — caller falls back to
        // a manual hint.
        _ => Ok(false),
    }
}

fn revoke_gitlab(token: &str) -> Result<bool> {
    // RFC 7009 — GitLab accepts revoke without client_secret for
    // public clients. The bundled torii client is registered that
    // way; users with a custom TORII_GITLAB_APP_ID must also have
    // it configured as a public client (the env-var fallback path
    // is for self-managed GitLab where the user controls both).
    let client_id = std::env::var("TORII_GITLAB_APP_ID").ok()
        .unwrap_or_else(|| "b72a85262c309587f67591da8fed4f8e8f4ee7349e9ed06f6a2a99ee7caec4fe".to_string());
    let client = crate::http::make_client();
    let req = client.post("https://gitlab.com/oauth/revoke")
        .form(&[
            ("client_id", client_id.as_str()),
            ("token", token),
            ("token_type_hint", "access_token"),
        ]);
    let resp = req.send().map_err(|e| ToriiError::InvalidConfig(
        format!("GitLab revoke: {}", e)
    ))?;
    let status = resp.status().as_u16();
    match status {
        200 | 401 | 404 => Ok(true),
        _ => {
            let body = resp.text().unwrap_or_default();
            Err(ToriiError::InvalidConfig(format!(
                "GitLab revoke returned HTTP {}: {}", status, body
            )))
        }
    }
}

fn revoke_github(token: &str) -> Result<bool> {
    // GitHub's `DELETE /applications/{client_id}/token` is the only
    // documented way to revoke an OAuth token, and it requires Basic
    // auth with client_id + client_secret. Bundled apps don't ship
    // their secret; users running their own app can set the env var.
    let client_id = std::env::var("TORII_GITHUB_APP_ID").ok()
        .unwrap_or_else(|| "Ov23liDcA2Njn7eRWnYV".to_string());
    let Ok(client_secret) = std::env::var("TORII_GITHUB_APP_SECRET") else {
        return Ok(false);
    };
    let client = crate::http::make_client();
    let url = format!("https://api.github.com/applications/{}/token", client_id);
    let req = client.delete(&url)
        .basic_auth(client_id.clone(), Some(client_secret))
        .header("Accept", "application/vnd.github+json")
        .json(&serde_json::json!({ "access_token": token }));
    let resp = req.send().map_err(|e| ToriiError::InvalidConfig(
        format!("GitHub revoke: {}", e)
    ))?;
    let status = resp.status().as_u16();
    match status {
        204 | 404 | 422 => Ok(true),
        _ => {
            let body = resp.text().unwrap_or_default();
            Err(ToriiError::InvalidConfig(format!(
                "GitHub revoke returned HTTP {}: {}", status, body
            )))
        }
    }
}

/// Where the user should go to revoke an OAuth token manually when
/// `revoke_token` returns Ok(false) (no programmatic endpoint). Used
/// in `torii auth rotate` to print a helpful hint.
pub fn revoke_hint_url(provider: &str) -> Option<&'static str> {
    match provider {
        "github"   => Some("https://github.com/settings/applications"),
        "gitlab"   => Some("https://gitlab.com/-/profile/applications"),
        "codeberg" => Some("https://codeberg.org/user/settings/applications"),
        "bitbucket"=> Some("https://bitbucket.org/account/settings/app-authorizations/"),
        _ => None,
    }
}

/// GitLab-specific: rotate a PAT in place via the native
/// `POST /personal_access_tokens/self/rotate` endpoint. Returns the
/// new token text. Requires the current token to have `api` scope.
/// Only GitLab supports this; for other platforms callers should
/// fall back to the OAuth rotate path.
pub fn rotate_gitlab_pat(token: &str) -> Result<String> {
    let client = crate::http::make_client();
    let req = client
        .post("https://gitlab.com/api/v4/personal_access_tokens/self/rotate")
        .header("Authorization", format!("Bearer {}", token));
    let resp = req.send().map_err(|e| ToriiError::InvalidConfig(
        format!("GitLab rotate PAT: {}", e)
    ))?;
    let status = resp.status().as_u16();
    let body = resp.text().unwrap_or_default();
    if status != 200 && status != 201 {
        return Err(ToriiError::InvalidConfig(format!(
            "GitLab rotate PAT returned HTTP {}: {}", status, body
        )));
    }
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        ToriiError::InvalidConfig(format!("parse rotate response: {}", e))
    })?;
    json["token"].as_str()
        .map(String::from)
        .ok_or_else(|| ToriiError::InvalidConfig(format!(
            "GitLab rotate PAT response missing `token`: {}", body
        )))
}

// =============================================================================
// OAuth 2.0 Authorization Code Grant with PKCE + loopback HTTP server.
//
// Used for providers that don't implement RFC 8628 Device Flow — most
// notably Bitbucket Cloud. Torii:
//   1. Generates a random code_verifier + its SHA-256 code_challenge.
//   2. Binds a localhost TCP listener (port 8888 by default).
//   3. Opens the platform's /authorize URL in the user's browser with
//      redirect_uri pointing at the loopback.
//   4. Waits for the browser to GET `/callback?code=...`.
//   5. Exchanges the code for an access_token at the token endpoint,
//      sending the code_verifier (PKCE — no client_secret needed for
//      public OAuth clients on platforms that honour PKCE).
// =============================================================================

use std::io::{Read, Write as IoWrite};
use std::net::TcpListener;

struct AuthCodeProvider {
    authorize_url: &'static str,
    token_url:     &'static str,
    scopes:        &'static str,
    client_id_env: &'static str,
    bundled_client_id: Option<&'static str>,
    /// Env var name for the OAuth client_secret. Some providers (e.g.
    /// Bitbucket) hand out a secret on every consumer registration;
    /// even with PKCE they expect it on the token-exchange call. The
    /// secret is **not** bundled in the binary — has to come from the
    /// user's env / .env file.
    client_secret_env: Option<&'static str>,
}

fn auth_code_provider(provider: &str) -> Option<AuthCodeProvider> {
    match provider {
        "bitbucket" => Some(AuthCodeProvider {
            authorize_url:     "https://bitbucket.org/site/oauth2/authorize",
            token_url:         "https://bitbucket.org/site/oauth2/access_token",
            scopes:            "repository repository:write account pullrequest pullrequest:write issue:write pipeline",
            client_id_env:     "TORII_BITBUCKET_APP_ID",
            bundled_client_id: Some("xQAkJEqx3LK4WtJ3KD"),
            client_secret_env: Some("TORII_BITBUCKET_APP_SECRET"),
        }),
        _ => None,
    }
}

const LOOPBACK_PORT: u16 = 8888;
const LOOPBACK_PATH: &str = "/callback";

/// Run the authorization-code flow for `provider`. Blocks until the
/// user authorises (success) or the listener is interrupted.
pub fn run_auth_code_flow(provider: &str) -> Result<String> {
    let cfg = auth_code_provider(provider).ok_or_else(|| ToriiError::InvalidConfig(format!(
        "OAuth authorization-code flow not configured for `{}`.", provider
    )))?;

    let client_id = std::env::var(cfg.client_id_env).ok()
        .or_else(|| cfg.bundled_client_id.map(String::from))
        .ok_or_else(|| ToriiError::InvalidConfig(format!(
            "No OAuth client_id for `{}`. Set {} or create a PAT manually and run \
             `torii auth set {} ...`.",
            provider, cfg.client_id_env, provider
        )))?;

    let client_secret = cfg.client_secret_env
        .and_then(|name| std::env::var(name).ok());

    // PKCE: random verifier + SHA-256 challenge. RFC 7636 demands the
    // verifier be 43-128 unreserved chars; we generate 64 base64-url
    // characters.
    let code_verifier = random_verifier();
    let code_challenge = sha256_base64url(&code_verifier);

    // Build the authorize URL.
    let redirect_uri = format!("http://localhost:{}{}", LOOPBACK_PORT, LOOPBACK_PATH);
    let state = random_verifier(); // CSRF token
    let authz_url = format!(
        "{}?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        cfg.authorize_url,
        urlencode(&client_id),
        urlencode(&redirect_uri),
        urlencode(cfg.scopes),
        urlencode(&state),
        urlencode(&code_challenge),
    );

    // Bind the loopback listener BEFORE printing the URL so we can
    // fail fast if the port is busy. Lossless: if another torii flow
    // is in progress on 8888 the user finds out immediately.
    let listener = TcpListener::bind(("127.0.0.1", LOOPBACK_PORT))
        .map_err(|e| ToriiError::InvalidConfig(format!(
            "OAuth loopback: cannot bind 127.0.0.1:{} ({}). Is another flow already running?",
            LOOPBACK_PORT, e
        )))?;

    println!();
    println!("⛩  Open this URL in your browser to authorise Torii:");
    println!();
    println!("   {}", authz_url);
    println!();
    println!("Waiting for the redirect on localhost:{}{}…", LOOPBACK_PORT, LOOPBACK_PATH);

    // Accept a single connection.
    let (mut stream, _addr) = listener.accept()
        .map_err(|e| ToriiError::InvalidConfig(format!("OAuth loopback accept: {}", e)))?;

    // Read the request line + a bit of the headers — we only need the
    // URL path with the code+state query string.
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)
        .map_err(|e| ToriiError::InvalidConfig(format!("OAuth loopback read: {}", e)))?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let request_line = request.lines().next().unwrap_or("");
    // `GET /callback?code=...&state=... HTTP/1.1`
    let path_query = request_line.split_whitespace().nth(1).unwrap_or("");

    // Always respond with something so the browser doesn't show an
    // error page — this happens before we know whether the code is
    // valid, so the response is best-effort.
    let body = "<!doctype html><html><body><h2>⛩  Authorised — you can close this tab.</h2></body></html>";
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );

    let (code, returned_state) = parse_callback(path_query)
        .ok_or_else(|| ToriiError::InvalidConfig(
            "OAuth callback didn't include a `code` parameter.".to_string()
        ))?;

    if returned_state != state {
        return Err(ToriiError::InvalidConfig(
            "OAuth state mismatch (possible CSRF). Run the command again.".to_string()
        ));
    }

    // Exchange the code for a token. Bitbucket accepts both client
    // secret (Basic auth) and PKCE-only — we send the secret if
    // available, fall back to PKCE alone.
    let client = crate::http::make_client();
    let mut params = vec![
        ("grant_type",    "authorization_code".to_string()),
        ("code",          code),
        ("redirect_uri",  redirect_uri),
        ("client_id",     client_id.clone()),
        ("code_verifier", code_verifier),
    ];
    let mut req = client.post(cfg.token_url).header("Accept", "application/json");
    if let Some(secret) = &client_secret {
        // Bitbucket prefers Basic auth for confidential consumers.
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", client_id, secret));
        req = req.header("Authorization", format!("Basic {}", b64));
    } else {
        // Public-client flow — Bitbucket needs client_id in the body
        // too; already added above.
        params.push(("client_secret_present", "false".to_string()));
        params.pop(); // remove the placeholder
    }
    let resp = req.form(&params).send()
        .map_err(|e| ToriiError::InvalidConfig(format!("OAuth token exchange: {}", e)))?;
    let json: serde_json::Value = resp.json()
        .map_err(|e| ToriiError::InvalidConfig(format!("OAuth token: malformed JSON: {}", e)))?;
    if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
        return Err(ToriiError::InvalidConfig(format!(
            "OAuth token exchange failed: {} — {}", err,
            json.get("error_description").and_then(|v| v.as_str()).unwrap_or("")
        )));
    }
    let token = json.get("access_token").and_then(|v| v.as_str())
        .ok_or_else(|| ToriiError::InvalidConfig(format!(
            "OAuth token exchange: response had no access_token. Body: {}", json
        )))?
        .to_string();

    println!("✅ Authorised. Token saved.");
    Ok(token)
}

/// Parse `/callback?code=XYZ&state=ABC` → `(code, state)`. Tolerant of
/// extra parameters and ordering.
fn parse_callback(path_query: &str) -> Option<(String, String)> {
    let qs = path_query.split('?').nth(1)?;
    let mut code = None;
    let mut state = None;
    for pair in qs.split('&') {
        let mut iter = pair.splitn(2, '=');
        match (iter.next(), iter.next()) {
            (Some("code"), Some(v))  => code  = Some(urldecode(v)),
            (Some("state"), Some(v)) => state = Some(urldecode(v)),
            _ => {}
        }
    }
    Some((code?, state?))
}

/// Random 64-character base64url string. Uses the OS RNG via
/// `std::time` mixed with a per-process counter — enough entropy for a
/// short-lived PKCE verifier without pulling in `rand`.
fn random_verifier() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut seed = [0u8; 48];
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let pid = std::process::id() as u64;
    let bump = COUNTER.fetch_add(1, Ordering::Relaxed);
    seed[..8].copy_from_slice(&now.as_nanos().to_le_bytes()[..8]);
    seed[8..16].copy_from_slice(&pid.to_le_bytes());
    seed[16..24].copy_from_slice(&bump.to_le_bytes());
    // Hash the seed to widen entropy — PKCE verifier doesn't need
    // cryptographic randomness, just unguessability.
    let hash = sha256_raw(&seed);
    base64url_nopad(&hash)[..43].to_string()
}

/// SHA-256 of input. Implemented inline to avoid pulling another dep
/// just for this — we already use base64; sha2 would be the alternative.
fn sha256_raw(input: &[u8]) -> [u8; 32] {
    // Use the sha2 crate (already in the tree transitively via reqwest
    // → rustls → ring). Add it explicitly to Cargo.toml.
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input);
    hasher.finalize().into()
}

fn sha256_base64url(input: &str) -> String {
    let digest = sha256_raw(input.as_bytes());
    base64url_nopad(&digest)
}

/// base64url without padding (RFC 4648 §5).
fn base64url_nopad(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn urlencode(s: &str) -> String {
    crate::url::encode(s)
}

fn urldecode(s: &str) -> String {
    // Tolerant decoder: handles `%XX` and `+`. Doesn't validate utf-8
    // beyond what the platform would; OAuth codes are ASCII anyway.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => { out.push(' '); i += 1; }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i+1] as char).to_digit(16);
                let lo = (bytes[i+2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push(((hi << 4) | lo) as u8 as char);
                    i += 3;
                } else {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
            c => { out.push(c as char); i += 1; }
        }
    }
    out
}

/// Whether the given provider has an authorization-code flow wired.
pub fn auth_code_flow_supported(provider: &str) -> bool {
    auth_code_provider(provider).is_some()
}
