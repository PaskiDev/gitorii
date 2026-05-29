use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToriiError {
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),

    #[error("Branch not found: {0}")]
    #[allow(dead_code)]
    BranchNotFound(String),

    #[error("Snapshot error: {0}")]
    Snapshot(String),

    #[error("Mirror error: {0}")]
    Mirror(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    // 0.7.22: new variants intended to replace the catch-all
    // InvalidConfig over time. New code should prefer these; the
    // migration of the ~430 existing InvalidConfig sites is tracked
    // under "Validation and polish" in ROADMAP.

    /// Networking / HTTP transport failure (DNS, connect, timeout,
    /// unexpected I/O — *not* a non-2xx response, see PlatformApi).
    #[error("Network error ({provider}): {message}")]
    #[allow(dead_code)]
    Network { provider: String, message: String },

    /// Platform-side rejection — the API returned a non-success
    /// status with a structured body.
    #[error("{provider} API {status}: {message}")]
    #[allow(dead_code)]
    PlatformApi { provider: String, status: u16, message: String },

    /// Credential / authorisation problem (missing token, expired
    /// PAT, 401, scope mismatch). Surfaces separately from
    /// "the config file is malformed".
    #[error("Auth error ({provider}): {message}")]
    #[allow(dead_code)]
    Auth { provider: String, message: String },
}

pub type Result<T> = std::result::Result<T, ToriiError>;
