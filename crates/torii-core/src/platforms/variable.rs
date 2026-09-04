//! CI/CD variables and secrets — list / get / set / remove.
//!
//! Two consumers share this type. `torii variable` and the TUI variables
//! view manage them; the CI agent (see `docs/CI_AGENT.md`) receives them
//! attached to a job, where [`CiVariable::masked`] decides what is struck
//! out of the trace and [`VariableKind::File`] decides what is spilled to
//! a temporary file. Defining it once means the agent's job ingestion and
//! the editing surface cannot drift apart.
//!
//! The awkward part of the model, and the reason `value` is an `Option`:
//! **most forges will not read a secret back.** GitHub Actions encrypts
//! secrets against the repository's public key and never returns the
//! plaintext; Gitea and Forgejo behave the same; GitLab does return
//! values, except for variables created as `masked_and_hidden`, which are
//! write-once there too. So the honest shape is "this key exists, and
//! here is what we know about it", not a table of values.

use super::gitlab::GitLabVariableClient;
use crate::error::{Result, ToriiError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableKind {
    /// Exported into the job's environment.
    EnvVar,
    /// Written to a temporary file; the variable holds that file's path.
    /// An SSH key or a kubeconfig has to be this — a PEM body in an
    /// environment variable arrives with its newlines mangled.
    File,
}

impl VariableKind {
    /// GitLab's wire spelling, also accepted by Gitea and Forgejo.
    pub fn as_api_str(self) -> &'static str {
        match self {
            VariableKind::EnvVar => "env_var",
            VariableKind::File => "file",
        }
    }

    pub fn from_api_str(s: &str) -> Self {
        match s {
            "file" => VariableKind::File,
            _ => VariableKind::EnvVar,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiVariable {
    pub key: String,
    /// `None` when the platform refuses to disclose it. Never confuse
    /// this with an empty value: `Some(String::new())` is a variable that
    /// really is set to the empty string.
    pub value: Option<String>,
    pub kind: VariableKind,
    /// Only handed to pipelines running on a protected branch or tag.
    /// A protected variable on a project with no protected tag is
    /// invisible to every tag pipeline — the failure mode is a green
    /// pipeline that silently skipped the job that needed it.
    pub protected: bool,
    /// Struck out of job logs by the platform, and by our own agent.
    pub masked: bool,
    /// Masked *and* undisclosable: set once, never read back.
    pub hidden: bool,
    /// When false the platform expands `$OTHER_VAR` inside the value.
    pub raw: bool,
    /// Environment scope. `None` means every environment (`*`).
    pub scope: Option<String>,
    pub description: Option<String>,
}

/// A write. Separate from [`CiVariable`] because a write always carries a
/// value while a read frequently cannot — an `Option` that callers must
/// remember to fill in would be the same bug waiting in a nicer shape.
#[derive(Debug, Clone)]
pub struct VariableWrite {
    pub key: String,
    pub value: String,
    pub kind: VariableKind,
    pub protected: bool,
    pub masked: bool,
    pub raw: bool,
    pub scope: Option<String>,
    pub description: Option<String>,
}

impl VariableWrite {
    /// A plain, unprotected, unmasked environment variable — the shape
    /// most callers start from and then adjust.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            kind: VariableKind::EnvVar,
            protected: false,
            masked: false,
            raw: true,
            scope: None,
            description: None,
        }
    }
}

pub trait VariableClient: Send {
    fn list(&self, owner: &str, repo: &str) -> Result<Vec<CiVariable>>;
    /// `scope` disambiguates keys defined more than once for different
    /// environments; `None` asks for the unscoped one.
    fn get(&self, owner: &str, repo: &str, key: &str, scope: Option<&str>) -> Result<CiVariable>;
    /// Create or replace. Implementations decide which by looking first —
    /// the platforms split creation and update across two verbs, and
    /// guessing wrong is a 400 rather than the intended write.
    fn set(&self, owner: &str, repo: &str, var: &VariableWrite) -> Result<CiVariable>;
    fn remove(&self, owner: &str, repo: &str, key: &str, scope: Option<&str>) -> Result<()>;
    /// Why this value cannot be masked on this platform, or `None` if it
    /// can. Worth calling before a write that asks for masking: the
    /// platforms reject an unmaskable value with a generic 400, and the
    /// specific reason is the useful half.
    fn masking_rejection(&self, value: &str) -> Option<String>;
}

/// GitLab's masking rules, as documented: a single line, no whitespace,
/// at least 8 characters. A third rule — the value must not equal the
/// name of an existing variable — needs the project's variable list, so
/// it is left to the API to reject.
///
/// The practical consequence, and the reason this is worth a function:
/// **a PEM private key can never be masked.** It is multi-line by
/// construction. Store it as a [`VariableKind::File`] variable and rely
/// on nothing echoing it, rather than believing masking has your back.
pub fn gitlab_masking_rejection(value: &str) -> Option<String> {
    if value.chars().count() < 8 {
        return Some("shorter than the 8 characters masking requires".to_string());
    }
    if value.contains('\n') || value.contains('\r') {
        return Some(
            "spans more than one line — a PEM key or certificate cannot be masked; \
             store it as a file variable instead"
                .to_string(),
        );
    }
    if value.chars().any(char::is_whitespace) {
        return Some("contains whitespace, which masking does not allow".to_string());
    }
    None
}

pub fn get_variable_client(platform: &str) -> Result<Box<dyn VariableClient>> {
    match platform {
        "gitlab" => Ok(Box::new(GitLabVariableClient::new()?)),
        other => Err(ToriiError::Unsupported(format!(
            "CI variables not implemented for `{}` yet. Supported: gitlab.",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trips_through_the_wire_spelling() {
        assert_eq!(VariableKind::File.as_api_str(), "file");
        assert_eq!(VariableKind::EnvVar.as_api_str(), "env_var");
        assert_eq!(VariableKind::from_api_str("file"), VariableKind::File);
        assert_eq!(VariableKind::from_api_str("env_var"), VariableKind::EnvVar);
    }

    #[test]
    fn an_unknown_kind_is_read_as_an_env_var() {
        // A forge that grows a third type should not make us drop the
        // variable on the floor; the common case stays the default.
        assert_eq!(
            VariableKind::from_api_str("something_new"),
            VariableKind::EnvVar
        );
    }

    #[test]
    fn a_pem_key_is_rejected_for_masking_with_the_file_advice() {
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk=\n-----END OPENSSH PRIVATE KEY-----";
        let reason = gitlab_masking_rejection(key).expect("a multi-line key cannot be masked");
        assert!(reason.contains("more than one line"), "got: {reason}");
        assert!(reason.contains("file variable"), "got: {reason}");
    }

    #[test]
    fn short_and_spaced_values_are_rejected_for_masking() {
        assert!(gitlab_masking_rejection("short").is_some());
        assert!(gitlab_masking_rejection("has a space in it").is_some());
        assert!(gitlab_masking_rejection("trailing\r").is_some());
    }

    #[test]
    fn an_ordinary_token_can_be_masked() {
        assert_eq!(gitlab_masking_rejection("glpat-xxxxxxxxxxxxxxxxxxxx"), None);
    }

    #[test]
    fn a_new_write_defaults_to_a_plain_unprotected_env_var() {
        let w = VariableWrite::new("KEY", "value");
        assert_eq!(w.kind, VariableKind::EnvVar);
        assert!(!w.protected);
        assert!(!w.masked);
        // raw is set explicitly rather than left to the platform, whose
        // documented default and observed default disagree, and an
        // expanded `$` in a secret is a corrupted secret.
        assert!(w.raw);
        assert!(w.scope.is_none());
    }

    #[test]
    fn get_variable_client_names_the_supported_platform_when_refusing() {
        // Not `unwrap_err`: the Ok side is a trait object with no Debug.
        let msg = match get_variable_client("bitbucket") {
            Err(e) => e.to_string(),
            Ok(_) => panic!("bitbucket has no variable client yet"),
        };
        assert!(msg.contains("bitbucket"), "got: {msg}");
        assert!(msg.contains("gitlab"), "got: {msg}");
    }
}
