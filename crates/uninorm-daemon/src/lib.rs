//! Background daemon for automatic NFD→NFC file conversion.
//!
//! `uninorm-daemon` provides a file-system watcher that monitors configured directories
//! and automatically converts NFD-encoded filenames and content to NFC in real time.
//! It supports per-entry configuration, debouncing, autostart registration
//! (macOS LaunchAgent / Linux systemd), and graceful lifecycle management.

pub mod autostart;
pub mod config;
pub mod controller;
pub mod daemon;
pub mod error;

pub use config::{WatchConfig, WatchEntry};
pub use controller::DaemonController;
pub use error::{ConfigError, DaemonError};
