//! GitLab — CI/CD variable client.
//!
//! Project-level variables only. Group- and instance-level variables sit
//! behind different paths and different permissions; a project is what
//! `owner/repo` names, so that is what this covers.

use crate::error::{Result, ToriiError};
use crate::platforms::variable::*;
use reqwest::blocking::Client;

pub struct GitLabVariableClient {
    token: String,
    pub(crate) base_url: String,
}

impl GitLabVariableClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("gitlab", ".")
            .value
            .ok_or_else(|| ToriiError::Auth {
                provider: "gitlab".into(),
                message: "GitLab token not found. Run: torii auth oauth gitlab".to_string(),
            })?;
        Ok(Self {
            token,
            base_url: "https://gitlab.com/api/v4".to_string(),
        })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }
    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    fn project_path(owner: &str, repo: &str) -> String {
        crate::url::encode(&format!("{}/{}", owner, repo))
    }

    fn collection_url(&self, owner: &str, repo: &str) -> String {
        format!(
            "{}/projects/{}/variables",
            self.base_url,
            Self::project_path(owner, repo)
        )
    }

    /// One variable's URL. A key can exist several times under different
    /// environment scopes, and without the filter GitLab picks for you —
    /// so a scoped edit made without it silently rewrites the wrong one.
    fn item_url(&self, owner: &str, repo: &str, key: &str, scope: Option<&str>) -> String {
        let base = format!(
            "{}/{}",
            self.collection_url(owner, repo),
            crate::url::encode(key)
        );
        match scope {
            Some(s) => format!(
                "{}?filter%5Benvironment_scope%5D={}",
                base,
                crate::url::encode(s)
            ),
            None => base,
        }
    }

    fn body(var: &VariableWrite) -> serde_json::Value {
        // Every flag is sent explicitly, none left to the platform's
        // default. `raw` is the reason why: the API docs say it defaults
        // to true on create, and a variable created against gitlab.com on
        // 04-09-2026 without it came back `raw: false`. Whichever is
        // right, a value the platform expands is a value with a
        // `$fragment` quietly replaced, which for a credential means one
        // that no longer works and a job log that does not say why.
        serde_json::json!({
            "key": var.key,
            "value": var.value,
            "variable_type": var.kind.as_api_str(),
            "protected": var.protected,
            "masked": var.masked,
            "raw": var.raw,
            "environment_scope": var.scope.clone().unwrap_or_else(|| "*".to_string()),
            "description": var.description,
        })
    }
}

impl VariableClient for GitLabVariableClient {
    fn list(&self, owner: &str, repo: &str) -> Result<Vec<CiVariable>> {
        let url = format!("{}?per_page=100", self.collection_url(owner, repo));
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        crate::http::extract_array(&json, &url)?
            .iter()
            .map(parse_gitlab_variable)
            .collect()
    }

    fn get(&self, owner: &str, repo: &str, key: &str, scope: Option<&str>) -> Result<CiVariable> {
        let url = self.item_url(owner, repo, key, scope);
        let req = self.client().get(&url).header("Authorization", self.auth());
        let json = crate::http::send_json(req, &format!("GitLab (url: {})", url))?;
        parse_gitlab_variable(&json)
    }

    fn set(&self, owner: &str, repo: &str, var: &VariableWrite) -> Result<CiVariable> {
        // GitLab splits create and update across POST and PUT, and each
        // fails on the other's case: POST on an existing key is a 400
        // "has already been taken", PUT on a missing one is a 404. Look
        // first rather than guess, and treat only a 404 as "absent" so a
        // 403 on a token without Maintainer still surfaces as itself.
        let exists = match self.get(owner, repo, &var.key, var.scope.as_deref()) {
            Ok(_) => true,
            Err(ToriiError::PlatformApi { status: 404, .. }) => false,
            Err(e) => return Err(e),
        };

        let (req, ctx) = if exists {
            let url = self.item_url(owner, repo, &var.key, var.scope.as_deref());
            (
                self.client().put(&url),
                format!("GitLab update variable {}", var.key),
            )
        } else {
            let url = self.collection_url(owner, repo);
            (
                self.client().post(&url),
                format!("GitLab create variable {}", var.key),
            )
        };

        let req = req
            .header("Authorization", self.auth())
            .json(&Self::body(var));
        let json = crate::http::send_json(req, &ctx)?;
        parse_gitlab_variable(&json)
    }

    fn remove(&self, owner: &str, repo: &str, key: &str, scope: Option<&str>) -> Result<()> {
        let url = self.item_url(owner, repo, key, scope);
        let req = self
            .client()
            .delete(&url)
            .header("Authorization", self.auth());
        crate::http::send_empty(req, &format!("GitLab delete variable {}", key))
    }

    fn masking_rejection(&self, value: &str) -> Option<String> {
        gitlab_masking_rejection(value)
    }
}

fn parse_gitlab_variable(v: &serde_json::Value) -> Result<CiVariable> {
    let key = v["key"]
        .as_str()
        .ok_or_else(|| ToriiError::MalformedResponse {
            provider: "gitlab".into(),
            message: format!("GitLab variable has no `key`: {}", v),
        })?
        .to_string();

    // `hidden` arrives on GitLab 17.4 and later for variables created as
    // masked_and_hidden. Those never carry a value, and an older GitLab
    // simply omits the field.
    let hidden = v["hidden"].as_bool().unwrap_or(false);
    let value = if hidden {
        None
    } else {
        v["value"].as_str().map(String::from)
    };

    // `*` is GitLab's spelling of "every environment", which is the
    // absence of a scope rather than a scope named `*`.
    let scope = match v["environment_scope"].as_str() {
        Some("*") | None => None,
        Some(s) => Some(s.to_string()),
    };

    Ok(CiVariable {
        key,
        value,
        kind: VariableKind::from_api_str(v["variable_type"].as_str().unwrap_or("env_var")),
        protected: v["protected"].as_bool().unwrap_or(false),
        masked: v["masked"].as_bool().unwrap_or(false),
        hidden,
        raw: v["raw"].as_bool().unwrap_or(false),
        scope,
        description: v["description"].as_str().map(String::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    // ── parser ───────────────────────────────────────────────────────────

    #[test]
    fn parse_file_variable_full() {
        let json = serde_json::json!({
            "key": "AUR_SSH_PRIVATE_KEY",
            "value": "-----BEGIN OPENSSH PRIVATE KEY-----",
            "variable_type": "file",
            "protected": true,
            "masked": false,
            "raw": true,
            "environment_scope": "*",
            "description": "AUR publishing key"
        });
        let v = parse_gitlab_variable(&json).unwrap();
        assert_eq!(v.key, "AUR_SSH_PRIVATE_KEY");
        assert_eq!(v.kind, VariableKind::File);
        assert!(v.protected);
        assert!(!v.masked);
        assert!(v.raw);
        assert_eq!(v.scope, None, "`*` is the absence of a scope");
        assert_eq!(v.description.as_deref(), Some("AUR publishing key"));
    }

    #[test]
    fn a_hidden_variable_reports_no_value() {
        let json = serde_json::json!({
            "key": "SECRET", "value": "", "masked": true, "hidden": true
        });
        let v = parse_gitlab_variable(&json).unwrap();
        assert!(v.hidden);
        assert!(
            v.value.is_none(),
            "a hidden variable must not look like one set to the empty string"
        );
    }

    #[test]
    fn an_empty_value_is_not_the_same_as_no_value() {
        let json = serde_json::json!({ "key": "EMPTY", "value": "" });
        let v = parse_gitlab_variable(&json).unwrap();
        assert_eq!(v.value.as_deref(), Some(""));
    }

    #[test]
    fn a_real_scope_survives() {
        let json =
            serde_json::json!({ "key": "K", "value": "v", "environment_scope": "production" });
        let v = parse_gitlab_variable(&json).unwrap();
        assert_eq!(v.scope.as_deref(), Some("production"));
    }

    #[test]
    fn parse_missing_key_is_malformed_response() {
        let err = parse_gitlab_variable(&serde_json::json!({ "value": "v" })).unwrap_err();
        assert!(
            matches!(err, ToriiError::MalformedResponse { .. }),
            "expected MalformedResponse, got: {err:?}"
        );
    }

    // ── client (httpmock) ────────────────────────────────────────────────

    fn client(server: &MockServer) -> GitLabVariableClient {
        GitLabVariableClient {
            token: "test-token".into(),
            base_url: server.base_url(),
        }
    }

    #[test]
    fn list_parses_variables() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/variables")
                .header("Authorization", "Bearer test-token");
            then.status(200).json_body(serde_json::json!([
                { "key": "A", "value": "1", "variable_type": "env_var" },
                { "key": "B", "value": "2", "variable_type": "file", "protected": true }
            ]));
        });
        let vars = client(&server).list("acme", "widget").unwrap();
        m.assert();
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[1].kind, VariableKind::File);
        assert!(vars[1].protected);
    }

    #[test]
    fn set_creates_with_post_when_the_key_is_absent() {
        let server = MockServer::start();
        let probe = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/variables/NEW");
            then.status(404)
                .json_body(serde_json::json!({ "message": "404 Variable Not Found" }));
        });
        let create = server.mock(|when, then| {
            when.method(POST)
                .path("/projects/acme%2Fwidget/variables")
                .json_body(serde_json::json!({
                    "key": "NEW",
                    "value": "s3cret-value",
                    "variable_type": "file",
                    "protected": true,
                    "masked": false,
                    "raw": true,
                    "environment_scope": "*",
                    "description": null
                }));
            then.status(201).json_body(serde_json::json!({
                "key": "NEW", "value": "s3cret-value", "variable_type": "file", "protected": true
            }));
        });

        let mut w = VariableWrite::new("NEW", "s3cret-value");
        w.kind = VariableKind::File;
        w.protected = true;
        let v = client(&server).set("acme", "widget", &w).unwrap();

        probe.assert();
        create.assert();
        assert_eq!(v.key, "NEW");
        assert_eq!(v.kind, VariableKind::File);
    }

    #[test]
    fn set_updates_with_put_when_the_key_exists() {
        let server = MockServer::start();
        let probe = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/variables/OLD");
            then.status(200)
                .json_body(serde_json::json!({ "key": "OLD", "value": "before" }));
        });
        let update = server.mock(|when, then| {
            when.method(PUT)
                .path("/projects/acme%2Fwidget/variables/OLD");
            then.status(200)
                .json_body(serde_json::json!({ "key": "OLD", "value": "after" }));
        });

        let v = client(&server)
            .set("acme", "widget", &VariableWrite::new("OLD", "after"))
            .unwrap();

        probe.assert();
        update.assert();
        assert_eq!(v.value.as_deref(), Some("after"));
    }

    #[test]
    fn set_propagates_a_permission_error_instead_of_treating_it_as_absent() {
        // A token without Maintainer gets 403 on the probe. Creating in
        // response would report "has already been taken" and bury the
        // real cause.
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/projects/acme%2Fwidget/variables/K");
            then.status(403)
                .json_body(serde_json::json!({ "message": "403 Forbidden" }));
        });
        let err = client(&server)
            .set("acme", "widget", &VariableWrite::new("K", "v"))
            .unwrap_err();
        assert!(
            matches!(err, ToriiError::PlatformApi { status: 403, .. }),
            "expected a 403 PlatformApi, got: {err:?}"
        );
    }

    #[test]
    fn a_scoped_get_sends_the_environment_filter() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/projects/acme%2Fwidget/variables/K")
                .query_param("filter[environment_scope]", "production");
            then.status(200).json_body(
                serde_json::json!({ "key": "K", "value": "v", "environment_scope": "production" }),
            );
        });
        let v = client(&server)
            .get("acme", "widget", "K", Some("production"))
            .unwrap();
        m.assert();
        assert_eq!(v.scope.as_deref(), Some("production"));
    }

    #[test]
    fn remove_deletes_the_key() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(DELETE)
                .path("/projects/acme%2Fwidget/variables/K")
                .header("Authorization", "Bearer test-token");
            then.status(204);
        });
        client(&server).remove("acme", "widget", "K", None).unwrap();
        m.assert();
    }
}
