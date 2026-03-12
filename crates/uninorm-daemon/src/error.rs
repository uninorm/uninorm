use std::path::PathBuf;

/// Errors related to configuration file operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config directory not found")]
    NoConfigDir,

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid config: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Errors related to daemon lifecycle operations.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("daemon not running")]
    NotRunning,

    #[error("daemon already running (PID {pid})")]
    AlreadyRunning { pid: u32 },

    #[error("watch daemon is only available on macOS")]
    UnsupportedPlatform,

    #[error("no enabled watch entries")]
    NoEnabledEntries,

    #[error("failed to spawn daemon: {0}")]
    Spawn(std::io::Error),

    #[error("all watch paths failed")]
    AllWatchesFailed,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("watcher error: {0}")]
    Notify(#[from] notify::Error),

    #[error("task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
}
