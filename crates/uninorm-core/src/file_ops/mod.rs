//! File and directory operations for NFD→NFC conversion.
//!
//! This module provides the core file-system operations for detecting and converting
//! Unicode NFD-encoded filenames and text content to NFC. It is organized into
//! three submodules:
//!
//! - [`convert`] — the main conversion engine ([`convert_path`])
//! - [`exclude`] — glob-based path exclusion ([`compile_excludes`], [`is_excluded`])
//! - [`scan`] — read-only pre-scan ([`scan_path`], [`ScanEntry`], [`ScanResult`])
//!
//! The top-level types [`ConversionOptions`] and [`ConversionStats`] configure and
//! report on conversion operations respectively.

mod convert;
mod exclude;
mod scan;

pub use convert::convert_path;
pub use exclude::{compile_excludes, is_excluded};
pub use scan::{scan_path, ScanEntry, ScanResult};

use crate::normalize::to_nfc;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Default maximum file size for content conversion (100 MB).
pub const DEFAULT_MAX_CONTENT_BYTES: u64 = 100 * 1024 * 1024;

/// Maximum directory traversal depth for recursive walks.
pub const MAX_WALK_DEPTH: usize = 256;

/// Global atomic counter for unique temp file names.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique temp file name that won't collide across processes or restarts.
pub fn temp_name() -> String {
    let pid = std::process::id();
    let count = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(".uninorm_tmp_{pid}_{ts}_{count}")
}

/// Check if two paths refer to the same filesystem inode.
/// Used to detect the APFS case where NFD and NFC forms of the same name
/// both resolve to the same file (normalization-insensitive lookup).
#[cfg(unix)]
pub fn same_inode(p1: &Path, p2: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(p1), std::fs::metadata(p2)) {
        (Ok(m1), Ok(m2)) => m1.ino() == m2.ino() && m1.dev() == m2.dev(),
        _ => false,
    }
}

#[cfg(windows)]
pub fn same_inode(p1: &Path, p2: &Path) -> bool {
    // NTFS doesn't have NFD/NFC aliasing like APFS, so canonical path comparison suffices.
    match (std::fs::canonicalize(p1), std::fs::canonicalize(p2)) {
        (Ok(c1), Ok(c2)) => c1 == c2,
        _ => false,
    }
}

#[cfg(not(any(unix, windows)))]
pub fn same_inode(_p1: &Path, _p2: &Path) -> bool {
    false
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ConversionOptions {
    pub convert_filenames: bool,
    pub convert_content: bool,
    pub dry_run: bool,
    pub recursive: bool,
    pub follow_symlinks: bool,
    /// Entry names (files or directories) to skip entirely.
    /// Supports glob patterns (e.g. `*.log`, `.git`, `build*`).
    /// Matched against each path component.
    #[cfg_attr(feature = "serde", serde(default))]
    pub exclude_patterns: Vec<String>,
    /// Maximum file size (bytes) for content conversion. Files larger than this
    /// are silently skipped. Defaults to [`DEFAULT_MAX_CONTENT_BYTES`] (100 MB).
    #[cfg_attr(feature = "serde", serde(default = "default_max_content_bytes"))]
    pub max_content_bytes: u64,
}

#[cfg(feature = "serde")]
fn default_max_content_bytes() -> u64 {
    DEFAULT_MAX_CONTENT_BYTES
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
            max_content_bytes: DEFAULT_MAX_CONTENT_BYTES,
        }
    }
}

#[derive(Debug, Default)]
pub struct ConversionStats {
    /// Total entries scanned (files + directories).
    pub files_scanned: usize,
    pub files_renamed: usize,
    pub files_content_converted: usize,
    /// Files skipped due to binary content, size limits, or non-UTF-8 encoding.
    pub files_skipped: usize,
    /// Number of directories encountered during traversal.
    pub directories_scanned: usize,
    pub errors: Vec<String>,
}

impl std::fmt::Display for ConversionStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Scanned: {} (dirs: {})  Renamed: {}  Content: {}  Skipped: {}",
            self.files_scanned,
            self.directories_scanned,
            self.files_renamed,
            self.files_content_converted,
            self.files_skipped,
        )?;
        if !self.errors.is_empty() {
            write!(f, "  Errors: {}", self.errors.len())?;
        }
        Ok(())
    }
}

/// Convert a single string (e.g. from clipboard) from NFD to NFC.
///
/// # Examples
///
/// ```
/// use uninorm_core::convert_text;
///
/// assert_eq!(convert_text("cafe\u{0301}"), "café");
/// assert_eq!(convert_text("already NFC"), "already NFC");
/// ```
pub fn convert_text(s: &str) -> String {
    to_nfc(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── convert_text() ────────────────────────────────────────────────────────

    #[test]
    fn test_convert_text_nfd_to_nfc() {
        assert_eq!(convert_text("e\u{0301}"), "\u{00E9}"); // é NFD → NFC
    }

    #[test]
    fn test_convert_text_already_nfc() {
        assert_eq!(convert_text("café"), "café");
    }

    #[test]
    fn test_convert_text_empty() {
        assert_eq!(convert_text(""), "");
    }

    #[test]
    fn test_convert_text_ascii_unchanged() {
        let s = "hello world 123";
        assert_eq!(convert_text(s), s);
    }

    #[test]
    fn test_convert_text_japanese_nfd() {
        // が in NFD
        let nfd = "\u{304B}\u{3099}";
        assert_eq!(convert_text(nfd), "が");
    }
}
