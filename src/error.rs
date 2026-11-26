use std::path::PathBuf;
use thiserror::Error;

/// Erros de domínio e infraestrutura do Crab.
#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("failed to resolve application directories")]
    DirectoriesUnavailable,

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse JSON at {path}: {source}")]
    JsonParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to serialize JSON: {0}")]
    JsonSerialize(#[from] serde_json::Error),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("manifest unavailable: network failed ({network}) and no valid local cache was found")]
    ManifestUnavailable { network: String },

    #[error("version '{0}' was not found in the Minecraft manifest")]
    VersionNotFound(String),

    #[error("invalid instance name: {0}")]
    InvalidInstanceName(String),

    #[error("invalid group name: {0}")]
    InvalidGroupName(String),

    #[error("group not found: {0}")]
    GroupNotFound(String),

    #[error("invalid Minecraft version: {0}")]
    InvalidVersion(String),

    #[error("instance not found: {0}")]
    InstanceNotFound(String),

    #[error("invalid metadata or resource: {0}")]
    InvalidMetadata(String),

    #[error("unsafe archive entry blocked (Zip Slip): {entry}")]
    ZipSlipAttempt { entry: String },

    #[error("unsafe path detected: {0}")]
    UnsafePath(String),

    #[error("integrity check failed for {path}: {reason}")]
    IntegrityFailed { path: PathBuf, reason: String },

    #[error("download failed for {url}: {reason}")]
    DownloadFailed { url: String, reason: String },

    #[error("failed to extract archive {path}: {reason}")]
    ArchiveExtract { path: PathBuf, reason: String },

    #[error("Java runtime error: {0}")]
    JavaRuntime(String),

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("launch error: {0}")]
    Launch(String),

    #[error("instance '{0}' is already running")]
    AlreadyRunning(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error("failed to open folder {path}: {source}")]
    OpenFolder {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, LauncherError>;
