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

    #[error("watch daemon is only available on macOS and Linux")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_no_config_dir() {
        let err = ConfigError::NoConfigDir;
        assert_eq!(err.to_string(), "config directory not found");
    }

    #[test]
    fn test_config_error_io() {
        let err = ConfigError::Io {
            path: PathBuf::from("/some/path/config.json"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        };
        let msg = err.to_string();
        assert!(msg.contains("/some/path/config.json"), "message was: {msg}");
    }

    #[test]
    fn test_daemon_error_not_running() {
        let err = DaemonError::NotRunning;
        assert_eq!(err.to_string(), "daemon not running");
    }

    #[test]
    fn test_daemon_error_already_running() {
        let err = DaemonError::AlreadyRunning { pid: 12345 };
        let msg = err.to_string();
        assert!(msg.contains("12345"), "message was: {msg}");
    }

    #[test]
    fn test_daemon_error_unsupported_platform() {
        let err = DaemonError::UnsupportedPlatform;
        let msg = err.to_string();
        assert!(msg.contains("macOS and Linux"), "message was: {msg}");
    }

    #[test]
    fn test_daemon_error_no_enabled_entries() {
        let err = DaemonError::NoEnabledEntries;
        assert_eq!(err.to_string(), "no enabled watch entries");
    }

    #[test]
    fn test_daemon_error_spawn() {
        let err = DaemonError::Spawn(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        ));
        let msg = err.to_string();
        assert!(msg.contains("failed to spawn daemon"), "message was: {msg}");
    }

    #[test]
    fn test_daemon_error_all_watches_failed() {
        let err = DaemonError::AllWatchesFailed;
        assert_eq!(err.to_string(), "all watch paths failed");
    }

    #[test]
    fn test_daemon_error_from_config() {
        let config_err = ConfigError::NoConfigDir;
        let daemon_err = DaemonError::from(config_err);
        // transparent variant: DaemonError::Config delegates to ConfigError's Display
        assert_eq!(daemon_err.to_string(), "config directory not found");
    }
}
