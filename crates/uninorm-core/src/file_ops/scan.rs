//! Pre-scan for NFD entries without modifying anything.
//!
//! [`scan_path`] walks a directory tree and identifies files and directories that
//! would be affected by NFD→NFC conversion. Content reads are parallelized
//! (up to 8 concurrent) while remaining read-only — no files are renamed or written.

use crate::normalize::{needs_filename_conversion, to_nfc, to_nfc_filename};
use futures::stream::{self, StreamExt};
use std::path::Path;
use walkdir::WalkDir;

use super::exclude::{compile_excludes, is_excluded};
use super::{ConversionOptions, MAX_WALK_DEPTH};

/// A single entry discovered during a pre-scan that would be affected.
#[derive(Debug, Clone)]
pub struct ScanEntry {
    /// Original path (with NFD filename)
    pub path: std::path::PathBuf,
    /// Whether the filename needs NFD→NFC conversion
    pub needs_rename: bool,
    /// New filename (NFC) if rename is needed
    pub new_name: Option<String>,
    /// Whether text content contains NFD sequences
    pub needs_content_conversion: bool,
}

/// Result of a pre-scan: lists affected entries without modifying anything.
#[derive(Debug, Default)]
pub struct ScanResult {
    pub total_scanned: usize,
    pub entries: Vec<ScanEntry>,
    pub errors: Vec<String>,
}

impl ScanResult {
    pub fn rename_count(&self) -> usize {
        self.entries.iter().filter(|e| e.needs_rename).count()
    }
    pub fn content_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.needs_content_conversion)
            .count()
    }
    pub fn affected_count(&self) -> usize {
        self.entries.len()
    }
}

/// Pre-scan a path to discover which files/directories would be affected by
/// conversion, without modifying anything. Content reads are parallelized.
pub async fn scan_path(path: &Path, opts: &ConversionOptions) -> ScanResult {
    let mut result = ScanResult::default();
    let (globs, invalid_patterns) = compile_excludes(&opts.exclude_patterns);
    for pat in &invalid_patterns {
        result
            .errors
            .push(format!("Invalid exclude pattern ignored: {pat}"));
    }
    let max_depth = if opts.recursive { MAX_WALK_DEPTH } else { 1 };
    let walker = WalkDir::new(path)
        .follow_links(opts.follow_symlinks)
        .contents_first(true)
        .max_depth(max_depth);

    // Phase 1: collect entries and determine rename needs (cheap, no I/O)
    struct PendingEntry {
        path: std::path::PathBuf,
        needs_rename: bool,
        new_name: Option<String>,
        is_file: bool,
    }

    let mut pending = Vec::new();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                result.errors.push(format!("Walk error: {e}"));
                continue;
            }
        };

        if is_excluded(entry.path(), path, &globs) {
            continue;
        }

        result.total_scanned += 1;

        let file_name = entry.file_name().to_string_lossy();
        let needs_rename = opts.convert_filenames && needs_filename_conversion(&file_name);
        let new_name = if needs_rename {
            Some(to_nfc_filename(&file_name))
        } else {
            None
        };

        // If rename-only (no content check needed), add directly
        if !opts.convert_content || !entry.file_type().is_file() {
            if needs_rename {
                result.entries.push(ScanEntry {
                    path: entry.path().to_path_buf(),
                    needs_rename,
                    new_name,
                    needs_content_conversion: false,
                });
            }
            continue;
        }

        pending.push(PendingEntry {
            path: entry.path().to_path_buf(),
            needs_rename,
            new_name,
            is_file: entry.file_type().is_file(),
        });
    }

    // Phase 2: parallel content check for files
    let max_bytes = opts.max_content_bytes;
    let content_results: Vec<_> = stream::iter(pending)
        .map(|pe| async move {
            let mut needs_content = false;
            if pe.is_file {
                if let Ok(meta) = tokio::fs::metadata(&pe.path).await {
                    if meta.len() <= max_bytes {
                        if let Ok(content) = tokio::fs::read_to_string(&pe.path).await {
                            let nfc = to_nfc(&content);
                            if nfc != content {
                                needs_content = true;
                            }
                        }
                    }
                }
            }
            (pe, needs_content)
        })
        .buffer_unordered(8)
        .collect()
        .await;

    for (pe, needs_content) in content_results {
        if pe.needs_rename || needs_content {
            result.entries.push(ScanEntry {
                path: pe.path,
                needs_rename: pe.needs_rename,
                new_name: pe.new_name,
                needs_content_conversion: needs_content,
            });
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_scan_empty_directory() {
        let dir = TempDir::new().unwrap();
        let opts = ConversionOptions::default();
        let result = scan_path(dir.path(), &opts).await;
        // Only the root dir itself is scanned
        assert!(result.entries.is_empty());
        assert_eq!(result.affected_count(), 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_scan_detects_nfd_filenames() {
        let dir = TempDir::new().unwrap();
        // Latin e + combining acute (NFD)
        fs::write(dir.path().join("e\u{0301}.txt"), "").unwrap();
        // Already NFC
        fs::write(dir.path().join("hello.txt"), "").unwrap();

        let opts = ConversionOptions::default();
        let result = scan_path(dir.path(), &opts).await;

        assert_eq!(result.rename_count(), 1);
        assert_eq!(result.content_count(), 0);

        let entry = &result.entries[0];
        assert!(entry.needs_rename);
        assert_eq!(entry.new_name.as_deref(), Some("\u{00E9}.txt"));
    }

    #[tokio::test]
    async fn test_scan_detects_nfd_content() {
        let dir = TempDir::new().unwrap();
        // File with NFC name but NFD content
        fs::write(dir.path().join("data.txt"), "caf\u{0065}\u{0301}").unwrap();
        // File with NFC content
        fs::write(dir.path().join("ok.txt"), "hello").unwrap();

        let opts = ConversionOptions {
            convert_content: true,
            ..ConversionOptions::default()
        };
        let result = scan_path(dir.path(), &opts).await;

        assert_eq!(result.content_count(), 1);
        let entry = result
            .entries
            .iter()
            .find(|e| e.needs_content_conversion)
            .unwrap();
        assert!(!entry.needs_rename);
        assert!(entry.needs_content_conversion);
    }

    #[tokio::test]
    async fn test_scan_non_recursive() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("e\u{0301}.txt"), "").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("a\u{0300}.txt"), "").unwrap();

        let opts = ConversionOptions {
            recursive: false,
            ..ConversionOptions::default()
        };
        let result = scan_path(dir.path(), &opts).await;

        // Only the top-level NFD file, not the one in sub/
        assert_eq!(result.rename_count(), 1);
    }

    #[tokio::test]
    async fn test_scan_with_excludes() {
        let dir = TempDir::new().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir(&git).unwrap();
        fs::write(git.join("e\u{0301}.txt"), "").unwrap();
        fs::write(dir.path().join("a\u{0300}.txt"), "").unwrap();

        let opts = ConversionOptions {
            exclude_patterns: vec![".git".to_string()],
            ..ConversionOptions::default()
        };
        let result = scan_path(dir.path(), &opts).await;

        // Only the top-level file, .git/ excluded
        assert_eq!(result.rename_count(), 1);
    }

    #[tokio::test]
    async fn test_scan_binary_file_skipped_for_content() {
        let dir = TempDir::new().unwrap();
        // Binary file (invalid UTF-8)
        fs::write(dir.path().join("bin.dat"), [0xFF, 0xFE, 0x00, 0x01]).unwrap();

        let opts = ConversionOptions {
            convert_content: true,
            ..ConversionOptions::default()
        };
        let result = scan_path(dir.path(), &opts).await;

        assert_eq!(result.content_count(), 0);
    }

    #[tokio::test]
    async fn test_scan_combined_rename_and_content() {
        let dir = TempDir::new().unwrap();
        // NFD filename + NFD content
        fs::write(dir.path().join("e\u{0301}.txt"), "caf\u{0065}\u{0301}").unwrap();

        let opts = ConversionOptions {
            convert_content: true,
            ..ConversionOptions::default()
        };
        let result = scan_path(dir.path(), &opts).await;

        assert_eq!(result.rename_count(), 1);
        assert_eq!(result.content_count(), 1);
        assert_eq!(result.affected_count(), 1); // same entry has both flags
    }

    #[tokio::test]
    async fn test_scan_all_nfc_returns_empty() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "world").unwrap();
        fs::write(dir.path().join("café.txt"), "normal").unwrap();

        let opts = ConversionOptions {
            convert_content: true,
            ..ConversionOptions::default()
        };
        let result = scan_path(dir.path(), &opts).await;

        assert_eq!(result.affected_count(), 0);
        assert!(result.total_scanned > 0);
    }

    #[tokio::test]
    async fn test_scan_result_helpers() {
        let result = ScanResult {
            total_scanned: 10,
            entries: vec![
                ScanEntry {
                    path: std::path::PathBuf::from("/a"),
                    needs_rename: true,
                    new_name: Some("b".to_string()),
                    needs_content_conversion: false,
                },
                ScanEntry {
                    path: std::path::PathBuf::from("/c"),
                    needs_rename: false,
                    new_name: None,
                    needs_content_conversion: true,
                },
                ScanEntry {
                    path: std::path::PathBuf::from("/d"),
                    needs_rename: true,
                    new_name: Some("e".to_string()),
                    needs_content_conversion: true,
                },
            ],
            errors: vec![],
        };

        assert_eq!(result.rename_count(), 2);
        assert_eq!(result.content_count(), 2);
        assert_eq!(result.affected_count(), 3);
    }
}
