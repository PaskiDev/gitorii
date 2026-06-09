//! Auth view key handling.

use super::Action;
use crate::tui::app::*;
use crossterm::event::{self, KeyCode, KeyModifiers};

// 0.7.30 — Auth view: contextual operations on the selected provider
// (oauth re-auth / rotate / set-token paste / remove). Same dropdown-
// driven pattern as Platform: `o` opens the menu, Enter dispatches,
// Esc closes. OAuth and rotate are externalised to a subprocess (the
// CLI variants) because they require a browser dance — the main loop
// suspends the TUI for them, same as the job-log pager.
pub(super) fn handle_auth(key: event::KeyEvent, app: &mut App) -> Option<Action> {
    use crate::tui::app::{AuthFocus, AuthPendingOp};

    match app.auth_view.focus {
        AuthFocus::List => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.auth_view.idx > 0 => {
                    app.auth_view.idx -= 1;
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                    if app.auth_view.idx + 1 < app.auth_view.items.len() =>
                {
                    app.auth_view.idx += 1;
                }
                (_, KeyCode::Char('o')) => {
                    if let Some(e) = app.auth_view.items.get(app.auth_view.idx) {
                        app.auth_view.pending_provider = e.provider.clone();
                        app.auth_view.dropdown_idx = 0;
                        app.auth_view.focus = AuthFocus::OpsDropdown;
                    }
                }
                _ => {}
            }
            None
        }
        AuthFocus::OpsDropdown => {
            let ops = crate::tui::views::auth::ops_for(&app.auth_view);
            match (key.modifiers, key.code) {
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) if app.auth_view.dropdown_idx > 0 => {
                    app.auth_view.dropdown_idx -= 1;
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j'))
                    if app.auth_view.dropdown_idx + 1 < ops.len() =>
                {
                    app.auth_view.dropdown_idx += 1;
                }
                (_, KeyCode::Enter) => {
                    return dispatch_auth_op(app);
                }
                _ => {}
            }
            None
        }
        AuthFocus::InputToken => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Enter) => {
                    let token = std::mem::take(&mut app.auth_view.input_buffer);
                    let provider = app.auth_view.pending_provider.clone();
                    let op = app.auth_view.pending_op.clone();
                    app.auth_view.focus = AuthFocus::List;
                    app.auth_view.pending_op = AuthPendingOp::None;
                    if token.trim().is_empty() {
                        app.set_status("✗ empty token, aborted");
                        return None;
                    }
                    match op {
                        AuthPendingOp::SetToken => {
                            match crate::auth::set_token(&provider, token.trim(), None) {
                                Ok(_) => app.set_status(format!("✓ {} token saved", provider)),
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                        }
                        AuthPendingOp::Login => {
                            let endpoint = crate::auth::default_endpoint();
                            match crate::auth::save_cloud(token.trim(), &endpoint) {
                                Ok(_) => app.set_status("✓ cloud key saved"),
                                Err(e) => app.set_status(format!("✗ {}", e)),
                            }
                        }
                        AuthPendingOp::None => {}
                    }
                    crate::tui::views::auth::refresh(app);
                }
                (_, KeyCode::Backspace) => {
                    app.auth_view.input_buffer.pop();
                }
                (_, KeyCode::Char(c)) if key.modifiers != KeyModifiers::CONTROL => {
                    app.auth_view.input_buffer.push(c);
                }
                _ => {}
            }
            None
        }
        AuthFocus::OauthFlow => {
            // While the worker is in flight (Starting/Waiting/Saving)
            // we ignore keys — the user can still hit Esc, which is
            // handled by the global Esc path above to clear the focus.
            // On Done/Error any key closes the modal so the user can
            // dismiss it without hunting for a specific binding.
            use crate::tui::app::OauthStatus;
            if let Some(state) = app.auth_view.oauth_flow.as_ref() {
                match state.status {
                    OauthStatus::Done(_) | OauthStatus::Error(_) => {
                        app.auth_view.oauth_flow = None;
                        app.auth_view.focus = AuthFocus::List;
                    }
                    _ => {}
                }
            }
            None
        }
        AuthFocus::ConfirmRemove => {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    let provider = app.auth_view.pending_provider.clone();
                    match crate::auth::remove_token(&provider, None) {
                        Ok(true) => app.set_status(format!("✓ {} token removed", provider)),
                        Ok(false) => app.set_status(format!("(no {} token was set)", provider)),
                        Err(e) => app.set_status(format!("✗ {}", e)),
                    }
                    app.auth_view.focus = AuthFocus::List;
                    crate::tui::views::auth::refresh(app);
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    app.auth_view.focus = AuthFocus::List;
                }
                _ => {}
            }
            None
        }
    }
}

pub(super) fn dispatch_auth_op(app: &mut App) -> Option<Action> {
    use crate::tui::app::{AuthFocus, AuthPendingOp};
    let provider = app.auth_view.pending_provider.clone();
    let ops = crate::tui::views::auth::ops_for(&app.auth_view);
    let idx = app.auth_view.dropdown_idx;
    let label = ops.get(idx).map(|o| o.0).unwrap_or("");
    match label {
        "Set token (paste)" => {
            app.auth_view.focus = AuthFocus::InputToken;
            app.auth_view.input_buffer.clear();
            app.auth_view.input_prompt = format!("Paste token for {}:", provider);
            app.auth_view.pending_op = AuthPendingOp::SetToken;
        }
        "Remove token" => {
            app.auth_view.focus = AuthFocus::ConfirmRemove;
        }
        "OAuth re-auth" => {
            // Driven entirely inside the TUI now: spawn the worker
            // and let the modal pump status updates.
            let prov = provider.clone();
            app.start_oauth_flow(prov, false, None);
        }
        "Rotate (OAuth)" => {
            // Capture the old token before kicking the worker so it
            // can revoke it after the new one lands.
            let old = crate::auth::resolve_token(&provider, ".").value;
            app.start_oauth_flow(provider.clone(), true, old);
        }
        "Rotate as PAT (GitLab)" => {
            // PAT rotate is one synchronous HTTP call — no need for
            // a modal, but we keep the user inside the TUI by
            // dispatching it here instead of suspending the screen.
            let old = crate::auth::resolve_token(&provider, ".").value;
            match old {
                None => app.set_status("✗ no GitLab token to rotate"),
                Some(old_tok) => match crate::oauth::rotate_gitlab_pat(&old_tok) {
                    Ok(new_tok) => match crate::auth::set_token(&provider, &new_tok, None) {
                        Ok(_) => {
                            crate::auth::drop_token_cache();
                            app.set_status("✓ GitLab PAT rotated");
                            crate::tui::views::auth::refresh(app);
                        }
                        Err(e) => app.set_status(format!("✗ save: {}", e)),
                    },
                    Err(e) => app.set_status(format!("✗ rotate: {}", e)),
                },
            }
            app.auth_view.focus = AuthFocus::List;
        }
        "Login (paste cloud key)" => {
            app.auth_view.focus = AuthFocus::InputToken;
            app.auth_view.input_buffer.clear();
            app.auth_view.input_prompt = "Paste gitorii_sk_… key:".to_string();
            app.auth_view.pending_op = AuthPendingOp::Login;
        }
        "Logout (cloud)" => {
            match crate::auth::delete() {
                Ok(_) => app.set_status("✓ cloud key removed"),
                Err(e) => app.set_status(format!("✗ {}", e)),
            }
            app.auth_view.focus = AuthFocus::List;
            crate::tui::views::auth::refresh(app);
        }
        _ => {
            app.auth_view.focus = AuthFocus::List;
        }
    }
    None
}
