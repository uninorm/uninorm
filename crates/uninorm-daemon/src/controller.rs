use crate::config::{self, WatchConfig};
use crate::error::DaemonError;

pub type Result<T> = std::result::Result<T, DaemonError>;

/// High-level daemon lifecycle controller.
/// Usable from CLI, TUI, or any other frontend.
pub struct DaemonController;

impl DaemonController {
    /// Start the daemon. Returns the PID on success.
    pub fn start() -> Result<u32> {
        #[cfg(not(unix))]
        {
            return Err(DaemonError::UnsupportedPlatform);
        }

        #[cfg(unix)]
        {
            let cfg = WatchConfig::load()?;
            if cfg.enabled_count() == 0 {
                return Err(DaemonError::NoEnabledEntries);
            }

            if config::is_daemon_running() {
                let pid = config::read_pid().unwrap_or(0);
                return Err(DaemonError::AlreadyRunning { pid });
            }

            crate::daemon::spawn_daemon().map_err(DaemonError::Spawn)?;
            std::thread::sleep(std::time::Duration::from_millis(300));

            config::read_pid().ok_or_else(|| {
                DaemonError::Spawn(std::io::Error::other(
                    "daemon did not write PID file after spawn",
                ))
            })
        }
    }

    /// Stop the running daemon. Returns Ok(()) even if already stopped.
    pub fn stop() -> Result<()> {
        #[cfg(not(unix))]
        {
            return Err(DaemonError::UnsupportedPlatform);
        }

        #[cfg(unix)]
        {
            if !config::is_daemon_running() {
                return Err(DaemonError::NotRunning);
            }
            config::signal_daemon(libc::SIGTERM);
            Ok(())
        }
    }

    /// Reload daemon config (SIGHUP). No-op if daemon not running.
    pub fn reload() -> Result<()> {
        #[cfg(not(unix))]
        {
            return Err(DaemonError::UnsupportedPlatform);
        }

        #[cfg(unix)]
        {
            if config::is_daemon_running() {
                config::signal_daemon(libc::SIGHUP);
            }
            Ok(())
        }
    }

    /// Check if daemon is running. Returns Some(pid) if running.
    pub fn status() -> Option<u32> {
        if config::is_daemon_running() {
            config::read_pid()
        } else {
            None
        }
    }

    /// Stop daemon if running and remove all config.
    pub fn reset() -> Result<()> {
        // Stop daemon first if running
        if config::is_daemon_running() {
            let _ = Self::stop();
        }

        let path = config::config_path()?;
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| {
                DaemonError::Config(crate::error::ConfigError::Io {
                    path: path.clone(),
                    source: e,
                })
            })?;
        }
        Ok(())
    }

    /// Reload or stop daemon based on whether enabled entries remain.
    pub fn reload_or_stop() -> Result<()> {
        if !config::is_daemon_running() {
            return Ok(());
        }

        let cfg = WatchConfig::load()?;
        if cfg.enabled_count() > 0 {
            Self::reload()
        } else {
            Self::stop()
        }
    }
}
