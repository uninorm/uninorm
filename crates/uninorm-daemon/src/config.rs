use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

pub type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEntry {
    pub path: PathBuf,
    #[serde(default = "default_true")]
    pub recursive: bool,
    #[serde(default)]
    pub content: bool,
    #[serde(default)]
    pub follow_symlinks: bool,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Maximum file size for content conversion (bytes). None = use global default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_content_bytes: Option<u64>,
    /// Whether this entry is active. Disabled entries are skipped by the daemon.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether to apply global ignore patterns to this entry. Default: true.
    #[serde(default = "default_true")]
    pub use_global_ignore: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    pub entries: Vec<WatchEntry>,
    /// Event debounce interval in milliseconds. None = 300ms default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debounce_ms: Option<u64>,
}

impl WatchConfig {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path).map_err(|e| ConfigError::Io {
            path: path.clone(),
            source: e,
        })?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ConfigError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        // Atomic write: tmp → rename
        let tmp = path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, content).map_err(|e| ConfigError::Io {
            path: tmp.clone(),
            source: e,
        })?;
        std::fs::rename(&tmp, &path).map_err(|e| ConfigError::Io {
            path: path.clone(),
            source: e,
        })?;
        Ok(())
    }

    /// Add or update an entry. Returns true if a new entry was added (vs updated).
    pub fn add_entry(&mut self, entry: WatchEntry) -> bool {
        if let Some(existing) = self.entries.iter_mut().find(|e| e.path == entry.path) {
            *existing = entry;
            false
        } else {
            self.entries.push(entry);
            true
        }
    }

    /// Remove an entry by path. Returns true if an entry was removed.
    pub fn remove_entry(&mut self, path: &Path) -> bool {
        let len = self.entries.len();
        self.entries.retain(|e| e.path != path);
        self.entries.len() < len
    }

    /// Count of enabled entries.
    pub fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|e| e.enabled).count()
    }
}

fn config_dir() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|d| d.join("uninorm"))
        .ok_or(ConfigError::NoConfigDir)
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("watch.json"))
}

pub fn pid_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("daemon.pid"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("uninorm.log"))
}

pub fn ignore_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("ignore"))
}

/// Load global ignore patterns from `~/.config/uninorm/ignore`.
/// Returns `(patterns, warning)`. Warning is `Some` if the file exists but
/// could not be read (e.g. permission denied). Missing file returns empty patterns
/// with no warning.
/// Format: one glob pattern per line, `#` for comments, blank lines ignored.
pub fn load_global_ignore() -> (Vec<String>, Option<String>) {
    let path = match ignore_path() {
        Ok(p) => p,
        Err(_) => return (Vec::new(), None),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), None),
        Err(e) => {
            return (
                Vec::new(),
                Some(format!("Warning: could not read global ignore file: {e}")),
            );
        }
    };
    (parse_ignore_patterns(&content), None)
}

/// Parse ignore file content into a list of glob patterns.
/// Handles UTF-8 BOM, comments (`#`), and blank lines.
pub fn parse_ignore_patterns(content: &str) -> Vec<String> {
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(content);
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect()
}

pub fn read_pid() -> Option<u32> {
    let path = pid_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    content.trim().parse().ok()
}

pub fn write_pid(pid: u32) -> Result<()> {
    let path = pid_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ConfigError::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    // Reject symlinks at PID path to prevent symlink-following attacks
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            return Err(ConfigError::Io {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "PID file path is a symlink",
                ),
            });
        }
        // Stale PID file exists (not a symlink) — remove before exclusive create
        let _ = std::fs::remove_file(&path);
    }

    // Use exclusive create (O_CREAT | O_EXCL) to prevent race between concurrent starts
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| ConfigError::Io {
            path: path.clone(),
            source: e,
        })?;
    write!(f, "{pid}").map_err(|e| ConfigError::Io {
        path: path.clone(),
        source: e,
    })?;
    Ok(())
}

pub fn remove_pid() {
    if let Ok(path) = pid_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Check if the daemon is running by verifying PID is alive AND
/// the process is actually our daemon (not a recycled PID).
pub fn is_daemon_running() -> bool {
    let Some(pid) = read_pid() else {
        return false;
    };
    if is_our_daemon(pid) {
        return true;
    }
    // Stale PID — clean up
    remove_pid();
    false
}

/// Verify that a PID belongs to our uninorm daemon, not a recycled PID.
#[cfg(unix)]
fn is_our_daemon(pid: u32) -> bool {
    // First check: is the process alive?
    let Some(pid_t) = libc::pid_t::try_from(pid).ok() else {
        return false;
    };
    let alive = unsafe { libc::kill(pid_t, 0) == 0 };
    if !alive {
        return false;
    }
    // Second check: verify the process is our daemon binary.
    #[cfg(target_os = "macos")]
    {
        let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let ret = unsafe {
            libc::proc_pidpath(
                pid_t,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len() as u32,
            )
        };
        if ret > 0 {
            let exe_path =
                std::path::Path::new(std::str::from_utf8(&buf[..ret as usize]).unwrap_or(""));
            if let Some(name) = exe_path.file_name().and_then(|n| n.to_str()) {
                return name.contains("uninorm");
            }
        }
        // If proc_pidpath fails, do not trust the PID (could be a recycled process)
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Linux: read /proc/<pid>/exe symlink
        if let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) {
            if let Some(name) = exe.file_name().and_then(|n| n.to_str()) {
                return name.contains("uninorm");
            }
        }
        // If /proc check fails, do not trust the PID (could be a recycled process)
        false
    }
}

#[cfg(not(unix))]
fn is_our_daemon(_pid: u32) -> bool {
    false
}

pub fn signal_daemon(sig: i32) -> bool {
    let Some(pid) = read_pid() else {
        return false;
    };
    if !is_our_daemon(pid) {
        return false;
    }
    #[cfg(unix)]
    {
        let Some(pid_t) = libc::pid_t::try_from(pid).ok() else {
            return false;
        };
        unsafe { libc::kill(pid_t, sig) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = sig;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_config_in(dir: &std::path::Path) -> WatchConfig {
        let mut cfg = WatchConfig::default();
        cfg.add_entry(WatchEntry {
            path: dir.join("downloads"),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![".git".to_string()],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        cfg
    }

    #[test]
    fn test_watch_config_default_is_empty() {
        let cfg = WatchConfig::default();
        assert!(cfg.entries.is_empty());
    }

    #[test]
    fn test_add_entry_new() {
        let mut cfg = WatchConfig::default();
        let is_new = cfg.add_entry(WatchEntry {
            path: "/tmp/test".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        assert!(is_new);
        assert_eq!(cfg.entries.len(), 1);
    }

    #[test]
    fn test_add_entry_update_existing() {
        let mut cfg = WatchConfig::default();
        cfg.add_entry(WatchEntry {
            path: "/tmp/test".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        let is_new = cfg.add_entry(WatchEntry {
            path: "/tmp/test".into(),
            recursive: false,
            content: true,
            follow_symlinks: false,
            exclude: vec!["node_modules".to_string()],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        assert!(!is_new, "should be update, not new");
        assert_eq!(cfg.entries.len(), 1);
        assert!(!cfg.entries[0].recursive);
        assert!(cfg.entries[0].content);
        assert_eq!(cfg.entries[0].exclude, vec!["node_modules"]);
    }

    #[test]
    fn test_remove_entry() {
        let mut cfg = WatchConfig::default();
        cfg.add_entry(WatchEntry {
            path: "/tmp/a".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        cfg.add_entry(WatchEntry {
            path: "/tmp/b".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        assert_eq!(cfg.entries.len(), 2);

        let removed = cfg.remove_entry(Path::new("/tmp/a"));
        assert!(removed);
        assert_eq!(cfg.entries.len(), 1);
        assert_eq!(cfg.entries[0].path, std::path::PathBuf::from("/tmp/b"));
    }

    #[test]
    fn test_remove_entry_not_found() {
        let mut cfg = WatchConfig::default();
        let removed = cfg.remove_entry(Path::new("/tmp/nonexistent"));
        assert!(!removed);
    }

    #[test]
    fn test_serde_roundtrip() {
        let dir = std::path::Path::new("/tmp");
        let cfg = test_config_in(dir);

        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let parsed: WatchConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].path, dir.join("downloads"));
        assert!(parsed.entries[0].recursive);
        assert!(!parsed.entries[0].content);
        assert_eq!(parsed.entries[0].exclude, vec![".git"]);
        assert!(parsed.entries[0].enabled);
    }

    #[test]
    fn test_serde_defaults() {
        let json = r#"{"entries": [{"path": "/tmp/x"}]}"#;
        let cfg: WatchConfig = serde_json::from_str(json).unwrap();

        assert!(cfg.entries[0].recursive);
        assert!(!cfg.entries[0].content);
        assert!(!cfg.entries[0].follow_symlinks);
        assert!(cfg.entries[0].exclude.is_empty());
        assert!(cfg.entries[0].enabled);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_file = tmp.path().join("watch.json");

        let mut cfg = WatchConfig::default();
        cfg.add_entry(WatchEntry {
            path: "/tmp/test_path".into(),
            recursive: true,
            content: true,
            follow_symlinks: false,
            exclude: vec![".git".to_string(), "node_modules".to_string()],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });

        let content = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write(&config_file, &content).unwrap();

        let loaded_content = fs::read_to_string(&config_file).unwrap();
        let loaded: WatchConfig = serde_json::from_str(&loaded_content).unwrap();

        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(
            loaded.entries[0].path,
            std::path::PathBuf::from("/tmp/test_path")
        );
        assert!(loaded.entries[0].content);
        assert_eq!(loaded.entries[0].exclude.len(), 2);
    }

    #[test]
    fn test_pid_file_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let pid_file = tmp.path().join("daemon.pid");

        fs::write(&pid_file, "12345").unwrap();
        let content = fs::read_to_string(&pid_file).unwrap();
        let pid: u32 = content.trim().parse().unwrap();
        assert_eq!(pid, 12345);

        fs::remove_file(&pid_file).unwrap();
        assert!(!pid_file.exists());
    }

    #[test]
    fn test_multiple_entries() {
        let mut cfg = WatchConfig::default();
        for i in 0..5 {
            cfg.add_entry(WatchEntry {
                path: format!("/tmp/path_{i}").into(),
                recursive: true,
                content: false,
                follow_symlinks: false,
                exclude: vec![],
                max_content_bytes: None,
                enabled: true,
                use_global_ignore: true,
            });
        }
        assert_eq!(cfg.entries.len(), 5);

        cfg.remove_entry(Path::new("/tmp/path_2"));
        assert_eq!(cfg.entries.len(), 4);
        assert!(cfg
            .entries
            .iter()
            .all(|e| e.path.as_path() != Path::new("/tmp/path_2")));
    }

    #[test]
    fn test_enable_disable_toggle() {
        let mut cfg = WatchConfig::default();
        cfg.add_entry(WatchEntry {
            path: "/tmp/toggle".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        assert!(cfg.entries[0].enabled);

        cfg.entries[0].enabled = false;
        assert!(!cfg.entries[0].enabled);

        cfg.entries[0].enabled = true;
        assert!(cfg.entries[0].enabled);
    }

    #[test]
    fn test_serde_enabled_roundtrip() {
        let mut cfg = WatchConfig::default();
        cfg.add_entry(WatchEntry {
            path: "/tmp/a".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        cfg.add_entry(WatchEntry {
            path: "/tmp/b".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: false,
            use_global_ignore: true,
        });

        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let parsed: WatchConfig = serde_json::from_str(&json).unwrap();

        assert!(parsed.entries[0].enabled);
        assert!(!parsed.entries[1].enabled);
    }

    #[test]
    fn test_serde_enabled_defaults_to_true() {
        let json = r#"{"entries": [{"path": "/tmp/old"}]}"#;
        let cfg: WatchConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.entries[0].enabled);
    }

    #[test]
    fn test_debounce_ms_serde() {
        let json = r#"{"entries": [], "debounce_ms": 500}"#;
        let cfg: WatchConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.debounce_ms, Some(500));

        let json2 = r#"{"entries": []}"#;
        let cfg2: WatchConfig = serde_json::from_str(json2).unwrap();
        assert_eq!(cfg2.debounce_ms, None);
    }

    #[test]
    fn test_enabled_count() {
        let mut cfg = WatchConfig::default();
        cfg.add_entry(WatchEntry {
            path: "/tmp/a".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: true,
            use_global_ignore: true,
        });
        cfg.add_entry(WatchEntry {
            path: "/tmp/b".into(),
            recursive: true,
            content: false,
            follow_symlinks: false,
            exclude: vec![],
            max_content_bytes: None,
            enabled: false,
            use_global_ignore: true,
        });
        assert_eq!(cfg.enabled_count(), 1);
    }

    #[test]
    fn test_parse_ignore_patterns_basic() {
        let content = "# comment\n\n.git\nnode_modules\n  *.pyc  \n# another comment\n";
        let patterns = super::parse_ignore_patterns(content);
        assert_eq!(patterns, vec![".git", "node_modules", "*.pyc"]);
    }

    #[test]
    fn test_parse_ignore_patterns_empty() {
        let content = "# only comments\n\n";
        let patterns = super::parse_ignore_patterns(content);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_parse_ignore_patterns_strips_bom() {
        let content = "\u{FEFF}# BOM file\n.git\ntarget\n";
        let patterns = super::parse_ignore_patterns(content);
        assert_eq!(patterns, vec![".git", "target"]);
    }

    #[test]
    fn test_parse_ignore_patterns_windows_line_endings() {
        let content = ".git\r\nnode_modules\r\n*.log\r\n";
        let patterns = super::parse_ignore_patterns(content);
        assert_eq!(patterns, vec![".git", "node_modules", "*.log"]);
    }
}
