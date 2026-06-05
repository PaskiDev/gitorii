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

    // 0.7.22 introduced precise variants to replace the catch-all
    // InvalidConfig; the bulk migration (~415 sites) is done. The
    // remaining InvalidConfig uses are *genuine* configuration errors
    // (config keys/values, missing user.name/email, config dirs,
    // policy files). New code should never add InvalidConfig for
    // network/API/auth/subprocess/fs/repo-state failures.

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

    /// The API answered 2xx but the body wasn't what we expected
    /// (unparseable JSON, missing field, wrong shape). Distinct from
    /// PlatformApi: the platform didn't *reject* us, it confused us.
    #[error("Malformed {provider} response: {message}")]
    MalformedResponse { provider: String, message: String },

    /// An external tool we shell out to (git, gpg, docker, $EDITOR,
    /// $SHELL, …) could not be spawned, or exited unsuccessfully.
    #[error("`{tool}` failed: {message}")]
    Subprocess { tool: String, message: String },

    /// Filesystem operation failure with context (what we were doing
    /// and on which path). Wraps the raw io::Error message — use
    /// instead of the bare `Io` variant when context is available.
    #[error("Filesystem error: {0}")]
    Fs(String),

    /// The repository is in a state that doesn't allow the requested
    /// operation (bare repo, detached/unborn HEAD, mid-rebase, …).
    #[error("Repository state error: {0}")]
    RepoState(String),

    /// Multi-repo workspace bookkeeping problem (unknown workspace,
    /// missing member path, malformed workspaces.toml entry).
    #[error("Workspace error: {0}")]
    Workspace(String),

    /// The platform (or torii) doesn't support the requested
    /// operation — capability gaps ("Bitbucket has no log-erase"),
    /// not-yet-wired surfaces, unsupported platform names. The
    /// message explains the workaround when one exists.
    #[error("Not supported: {0}")]
    Unsupported(String),

    /// The command was invoked incorrectly — missing argument,
    /// conflicting flags, a path that isn't what the verb expects.
    /// Message is self-explanatory, no prefix added.
    #[error("{0}")]
    Usage(String),
}

pub type Result<T> = std::result::Result<T, ToriiError>;
