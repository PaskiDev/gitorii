//! Auth view state + OAuth flow.

use super::*;

#[derive(Clone)]
pub struct AuthEntry {
    pub provider: String,
    pub masked: Option<String>,
    pub source: String, // "global" / "local" / "env: $VAR" / "(not set)"
}

pub struct AuthState {
    pub items: Vec<AuthEntry>,
    pub idx: usize,
    pub status: Option<String>,
    pub cloud_key_set: bool,
    pub cloud_endpoint: String,

    /// 0.7.30 — interactive ops state. `focus` says what overlay (if
    /// any) is currently active; `dropdown_idx` is the selection in
    /// the ops menu; `input_buffer` holds the pasted token while in
    /// `InputToken`; `pending_provider` captures which provider the
    /// in-flight operation applies to.
    pub focus: AuthFocus,
    pub dropdown_idx: usize,
    pub input_buffer: String,
    pub input_prompt: String,
    pub pending_provider: String,
    pub pending_op: AuthPendingOp,

    /// 0.7.32 — in-flight OAuth modal. Some when an OAuth/rotate flow
    /// is running or just finished and waiting for the user to close
    /// the dialog.
    pub oauth_flow: Option<OauthFlowState>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthFocus {
    List,
    OpsDropdown,
    InputToken,
    ConfirmRemove,
    /// 0.7.32 — OAuth flow modal driven from inside the TUI.
    /// Shows the verification URL + user code while a background
    /// worker polls the platform.
    OauthFlow,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthPendingOp {
    None,
    SetToken,
    Login,
}

/// 0.7.32 — visible state of the in-TUI OAuth flow modal. Updated
/// from the background worker over a channel and read by the
/// `auth` view's renderer + the global hint bar.
#[derive(Debug, Clone)]
pub enum OauthStatus {
    /// Doing the initial POST that asks the platform for a device
    /// code (typically <1s). Nothing for the user to copy yet.
    Starting,
    /// User should open `display_uri` and (if needed) paste `user_code`.
    /// We are polling the token endpoint every `interval` seconds in
    /// the background.
    Waiting {
        display_uri: String,
        user_code: String,
    },
    /// Token in hand; storing it now + (for rotate) revoking the old.
    Saving,
    /// All done; modal shows the success and the next keystroke closes
    /// it.
    Done(String),
    /// Something blew up; same idea — keystroke closes.
    Error(String),
}

#[derive(Debug, Clone)]
pub struct OauthFlowState {
    pub provider: String,
    /// When false this is a plain re-auth; when true the worker has
    /// captured the old token and will best-effort revoke it after
    /// the new one is saved. The old value lives only inside the
    /// worker's closure, never in this struct.
    pub rotate: bool,
    pub status: OauthStatus,
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            idx: 0,
            status: None,
            cloud_key_set: false,
            cloud_endpoint: String::new(),
            focus: AuthFocus::List,
            dropdown_idx: 0,
            input_buffer: String::new(),
            input_prompt: String::new(),
            pending_provider: String::new(),
            pending_op: AuthPendingOp::None,
            oauth_flow: None,
        }
    }
}

// -- Platform view (0.7.12) ------------------------------------------------
//
// Unified surface for the per-platform CI/CD objects exposed by the CLI
// (`torii pipeline|job|release|package`). The view groups the four into
// horizontal sub-tabs and lets you drill from a pipeline into its jobs
// and from a job into its trace.

impl App {
    /// 0.7.32 — start an in-TUI OAuth flow for `provider`. Worker
    /// thread does init → emits `Waiting{url, code}` → polls every
    /// `interval` → emits `Saving` → calls `auth::set_token` →
    /// (optionally) revokes `old_token` → emits `Done(masked)` /
    /// `Error(msg)`. The view stays open while this runs; the user
    /// can copy the URL/code and authorise in the browser without
    /// the TUI ever leaving the alt screen.
    pub fn start_oauth_flow(&mut self, provider: String, rotate: bool, old_token: Option<String>) {
        self.auth_view.oauth_flow = Some(crate::tui::app::OauthFlowState {
            provider: provider.clone(),
            rotate,
            status: crate::tui::app::OauthStatus::Starting,
        });
        self.auth_view.focus = crate::tui::app::AuthFocus::OauthFlow;

        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_oauth_rx = Some(rx);

        std::thread::spawn(move || {
            use crate::tui::app::OauthStatus;

            // 1. Init.
            let mut session = match crate::oauth::start_device_flow(&provider) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(OauthStatus::Error(e.to_string()));
                    return;
                }
            };
            let _ = tx.send(OauthStatus::Waiting {
                display_uri: session.display_uri.clone(),
                user_code: session.user_code.clone(),
            });

            // 2. Poll until done or error.
            let (token, refresh, expires_in) = loop {
                std::thread::sleep(crate::oauth::device_flow_interval(&session));
                match crate::oauth::poll_device_flow(&mut session) {
                    Ok(crate::oauth::DeviceFlowStep::Pending)
                    | Ok(crate::oauth::DeviceFlowStep::SlowDown) => continue,
                    Ok(crate::oauth::DeviceFlowStep::Done {
                        access_token,
                        refresh_token,
                        expires_in,
                    }) => break (access_token, refresh_token, expires_in),
                    Err(e) => {
                        let _ = tx.send(OauthStatus::Error(e.to_string()));
                        return;
                    }
                }
            };

            // 3. Save (+ refresh_token / expiry hint) + optional revoke.
            let _ = tx.send(OauthStatus::Saving);
            if let Err(e) = crate::auth::set_token_with_refresh(
                &provider,
                &token,
                refresh.as_deref(),
                expires_in,
            ) {
                let _ = tx.send(OauthStatus::Error(format!("save: {}", e)));
                return;
            }
            if rotate {
                if let Some(old) = old_token {
                    let _ = crate::oauth::revoke_token(&provider, &old);
                }
            }
            // Drop the in-process cache so the rest of the TUI picks
            // up the new value on the next resolve_token (the parent
            // process didn't see set_token invalidate it).
            crate::auth::drop_token_cache();

            let chars: Vec<char> = token.chars().collect();
            let masked = if chars.len() < 12 {
                "****".to_string()
            } else {
                let head: String = chars.iter().take(6).collect();
                let tail: String = chars.iter().skip(chars.len() - 4).collect();
                format!("{}…{}", head, tail)
            };
            let _ = tx.send(OauthStatus::Done(masked));
        });
    }
}
