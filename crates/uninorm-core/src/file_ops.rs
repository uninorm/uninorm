use crate::normalize::{needs_filename_conversion, to_nfc, to_nfc_filename};
use anyhow::Result;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::path::Path;
use walkdir::WalkDir;

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
    use std::os::windows::fs::MetadataExt;
    match (std::fs::metadata(p1), std::fs::metadata(p2)) {
        (Ok(m1), Ok(m2)) => matches!(
            (m1.file_index(), m1.volume_serial_number(),
             m2.file_index(), m2.volume_serial_number()),
            (Some(i1), Some(s1), Some(i2), Some(s2)) if i1 == i2 && s1 == s2
        ),
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
    /// Matched against each path component; matching directories are not descended into.
    #[cfg_attr(feature = "serde", serde(default))]
    pub exclude_patterns: Vec<String>,
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
        }
    }
}

#[derive(Debug, Default)]
pub struct ConversionStats {
    pub files_scanned: usize,
    pub files_renamed: usize,
    pub files_content_converted: usize,
    pub errors: Vec<String>,
}

/// Convert NFD→NFC for files/folders under `path`.
///
/// Uses `contents_first(true)` so children are renamed before their parent
/// directory, preventing path invalidation during traversal.
pub async fn convert_path(
    path: &Path,
    opts: &ConversionOptions,
    mut progress: impl FnMut(&ConversionStats),
) -> Result<ConversionStats> {
    let mut stats = ConversionStats::default();

    let max_depth = if opts.recursive { usize::MAX } else { 1 };

    let walker = WalkDir::new(path)
        .follow_links(opts.follow_symlinks)
        .contents_first(true) // rename children before parents
        .max_depth(max_depth);

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                stats.errors.push(format!("Walk error: {e}"));
                continue;
            }
        };

        // Skip entries whose path contains an excluded component.
        // Checked against every component of the relative path so that both
        // the excluded directory itself and all its descendants are skipped.
        // Note: filter_entry cannot be used here because contents_first(true)
        // yields children before their parent directory, making early pruning
        // impossible — we must check each entry individually instead.
        if !opts.exclude_patterns.is_empty() {
            let relative = entry.path().strip_prefix(path).unwrap_or(entry.path());
            let excluded = relative.components().any(|c| {
                if let std::path::Component::Normal(name) = c {
                    let s = name.to_string_lossy();
                    opts.exclude_patterns
                        .iter()
                        .any(|pat| s.as_ref() == pat.as_str())
                } else {
                    false
                }
            });
            if excluded {
                continue;
            }
        }

        stats.files_scanned += 1;

        let file_name = entry.file_name().to_string_lossy();

        // Rename file/folder if needed. Track whether rename actually succeeded
        // so content conversion uses the correct path (NFC path only if rename succeeded).
        let mut rename_succeeded = false;
        if opts.convert_filenames && needs_filename_conversion(&file_name) {
            let nfc_name = to_nfc_filename(&file_name);
            let new_path = entry.path().with_file_name(&nfc_name);

            // On APFS/HFS+ the NFC form resolves to the same inode as the NFD
            // form (normalization-insensitive lookup), so new_path.exists() is
            // true even though no *different* file is there.  Only treat it as
            // a conflict when it resolves to a genuinely different file.
            let is_conflict = new_path.exists() && !same_inode(entry.path(), &new_path);

            if is_conflict {
                stats.errors.push(format!(
                    "Rename conflict: {} already exists",
                    new_path.display()
                ));
            } else if !opts.dry_run {
                // Two-step rename: NFD → tmp → NFC
                // A direct NFD→NFC rename is a no-op on normalization-insensitive
                // filesystems (APFS default volume).  Moving through a neutral
                // temp name forces the directory entry to be rewritten with the
                // NFC bytes.
                let parent = entry.path().parent().unwrap_or(entry.path());
                let tmp = parent.join(format!(".uninorm_tmp_{}", stats.files_scanned));
                match tokio::fs::rename(entry.path(), &tmp).await {
                    Ok(_) => match tokio::fs::rename(&tmp, &new_path).await {
                        Ok(_) => {
                            stats.files_renamed += 1;
                            rename_succeeded = true;
                        }
                        Err(e) => {
                            let _ = tokio::fs::rename(&tmp, entry.path()).await; // restore
                            stats
                                .errors
                                .push(format!("Rename failed {}: {e}", entry.path().display()));
                        }
                    },
                    Err(e) => stats
                        .errors
                        .push(format!("Rename failed {}: {e}", entry.path().display())),
                }
            } else {
                stats.files_renamed += 1; // count in dry-run too
            }
        }

        // Convert file content (text files only)
        if opts.convert_content && entry.file_type().is_file() {
            // Use NFC path only if we actually renamed to it; otherwise file is still at entry.path()
            let file_name_nfc = to_nfc_filename(&file_name);
            let current_path = if opts.convert_filenames
                && needs_filename_conversion(&file_name)
                && !opts.dry_run
                && rename_succeeded
            {
                entry.path().with_file_name(&file_name_nfc)
            } else {
                entry.path().to_path_buf()
            };

            const MAX_CONTENT_BYTES: u64 = 100 * 1024 * 1024;
            if let Ok(meta) = tokio::fs::metadata(&current_path).await {
                if meta.len() > MAX_CONTENT_BYTES {
                    progress(&stats);
                    continue;
                }
            }

            match tokio::fs::read_to_string(&current_path).await {
                Ok(content) => {
                    let nfc_content = to_nfc(&content);
                    if nfc_content != content {
                        if !opts.dry_run {
                            // Append ".uninorm_tmp" to the full filename (not replace extension)
                            // so "file.txt" and "file.pdf" don't both map to "file.uninorm_tmp".
                            let fname = current_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            let tmp_path =
                                current_path.with_file_name(format!("{fname}.uninorm_tmp"));
                            match tokio::fs::write(&tmp_path, nfc_content.as_bytes()).await {
                                Ok(_) => match tokio::fs::rename(&tmp_path, &current_path).await {
                                    Ok(_) => stats.files_content_converted += 1,
                                    Err(e) => {
                                        let _ = tokio::fs::remove_file(&tmp_path).await;
                                        stats.errors.push(format!(
                                            "Content write failed {}: {e}",
                                            current_path.display()
                                        ));
                                    }
                                },
                                Err(e) => stats.errors.push(format!(
                                    "Content write failed {}: {e}",
                                    current_path.display()
                                )),
                            }
                        } else {
                            stats.files_content_converted += 1;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    // Not valid UTF-8 — binary file, skip silently
                }
                Err(e) => {
                    stats.errors.push(format!(
                        "Content read failed {}: {e}",
                        current_path.display()
                    ));
                }
            }
        }

        progress(&stats);
    }

    Ok(stats)
}

/// Convert a single string (e.g. from clipboard) from NFD to NFC.
pub fn convert_text(s: &str) -> String {
    to_nfc(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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

    // ── ConversionOptions defaults ────────────────────────────────────────────

    #[test]
    fn test_default_options() {
        let opts = ConversionOptions::default();
        assert!(opts.convert_filenames);
        assert!(!opts.convert_content);
        assert!(!opts.dry_run);
        assert!(opts.recursive);
        assert!(!opts.follow_symlinks);
    }

    // ── Filename conversion: dry run ──────────────────────────────────────────

    #[tokio::test]
    async fn test_convert_filename_dry_run() {
        let dir = TempDir::new().unwrap();
        let nfc_path = dir.path().join("강남구.txt");
        fs::write(&nfc_path, "test content").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: true,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_scanned, 2); // dir + file
        assert_eq!(stats.errors, Vec::<String>::new());
    }

    // ── Filename conversion: actual rename (Latin combining chars work on all FSes)

    #[tokio::test]
    async fn test_convert_filename_actual_rename() {
        let dir = TempDir::new().unwrap();
        // "cafe\u{0301}.txt" = "café.txt" in NFD — needs_filename_conversion returns true
        let nfd_name = format!("cafe\u{0301}.txt");
        let nfd_path = dir.path().join(&nfd_name);
        fs::write(&nfd_path, "content").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(
            stats.errors.is_empty(),
            "unexpected errors: {:?}",
            stats.errors
        );
        assert_eq!(stats.files_renamed, 1);

        // The NFC-named file should be readable
        let nfc_path = dir.path().join("caf\u{00E9}.txt");
        let content = fs::read_to_string(&nfc_path).unwrap();
        assert_eq!(content, "content");
    }

    // ── Content conversion ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_convert_content() {
        let dir = TempDir::new().unwrap();
        let nfd_content = "e\u{0301}"; // é in NFD
        let path = dir.path().join("test.txt");
        fs::write(&path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_content_converted, 1);
        let result = fs::read_to_string(&path).unwrap();
        assert_eq!(result, "\u{00E9}"); // é in NFC
    }

    #[tokio::test]
    async fn test_convert_content_dry_run_does_not_write() {
        let dir = TempDir::new().unwrap();
        let nfd_content = "e\u{0301}";
        let path = dir.path().join("test.txt");
        fs::write(&path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: true,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_content_converted, 1); // counted but not written
        let result = fs::read_to_string(&path).unwrap();
        assert_eq!(result, nfd_content, "dry run must not modify file");
    }

    #[tokio::test]
    async fn test_already_nfc_content_not_counted() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.txt"), "café 강남구").unwrap(); // already NFC

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_content_converted, 0);
    }

    // ── Binary / unreadable files skipped silently ────────────────────────────

    #[tokio::test]
    async fn test_binary_file_silently_skipped() {
        let dir = TempDir::new().unwrap();
        // Write invalid UTF-8 bytes
        fs::write(dir.path().join("binary.bin"), [0xFF, 0xFE, 0x00, 0x01]).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(
            stats.errors.is_empty(),
            "binary files must not produce errors"
        );
        assert_eq!(stats.files_content_converted, 0);
    }

    // ── Recursive vs non-recursive ────────────────────────────────────────────

    #[tokio::test]
    async fn test_non_recursive_skips_subdirs() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("sub");
        fs::create_dir(&subdir).unwrap();
        let nfd = "e\u{0301}";
        fs::write(dir.path().join("top.txt"), nfd).unwrap();
        fs::write(subdir.join("nested.txt"), nfd).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_content_converted, 1,
            "only top-level file should be converted"
        );
    }

    #[tokio::test]
    async fn test_recursive_descends_into_subdirs() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("sub");
        fs::create_dir(&subdir).unwrap();
        let nfd = "e\u{0301}";
        fs::write(dir.path().join("top.txt"), nfd).unwrap();
        fs::write(subdir.join("nested.txt"), nfd).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_content_converted, 2,
            "recursive should convert files in subdirs"
        );
    }

    // ── Empty directory ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_empty_directory() {
        let dir = TempDir::new().unwrap();
        let opts = ConversionOptions::default();
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_scanned, 1); // root dir itself
        assert_eq!(stats.files_renamed, 0);
        assert_eq!(stats.files_content_converted, 0);
        assert!(stats.errors.is_empty());
    }

    // ── Progress callback is invoked per entry ────────────────────────────────

    #[tokio::test]
    async fn test_progress_callback_invoked() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        fs::write(dir.path().join("b.txt"), "x").unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: false,
            dry_run: true,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let mut calls = 0usize;
        convert_path(dir.path(), &opts, |_| calls += 1)
            .await
            .unwrap();
        assert!(
            calls >= 3,
            "expected at least 3 progress calls (2 files + dir), got {calls}"
        );
    }

    // ── files_scanned counts files and directories ────────────────────────────

    #[tokio::test]
    async fn test_files_scanned_counts_dirs_and_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        fs::write(dir.path().join("b.txt"), "x").unwrap();

        let opts = ConversionOptions::default();
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_scanned, 3); // 2 files + 1 root dir
    }

    // ── Combined: rename + content in one pass ────────────────────────────────

    #[tokio::test]
    async fn test_combined_filename_and_content_conversion() {
        let dir = TempDir::new().unwrap();
        let nfd_name = format!("cafe\u{0301}.txt"); // filename in NFD
        let nfd_content = "e\u{0301}"; // content in NFD
        fs::write(dir.path().join(&nfd_name), nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(stats.errors.is_empty(), "errors: {:?}", stats.errors);
        assert_eq!(stats.files_renamed, 1);
        assert_eq!(stats.files_content_converted, 1);

        let nfc_path = dir.path().join("caf\u{00E9}.txt");
        let content = fs::read_to_string(&nfc_path).unwrap();
        assert_eq!(content, "\u{00E9}");
    }

    // ── exclude_patterns skips matched directories ────────────────────────────

    #[tokio::test]
    async fn test_exclude_patterns_skips_git() {
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir(&git_dir).unwrap();
        let nfd_name = format!("cafe\u{0301}.txt");
        fs::write(git_dir.join(&nfd_name), "content").unwrap();
        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: vec![".git".to_string()],
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_renamed, 0, "files inside .git must be excluded");
    }

    // ── exclude_patterns: deeply nested path is also excluded ─────────────────
    // Regression guard: a pattern must match any component in the relative path,
    // not just the immediate parent.

    #[tokio::test]
    async fn test_exclude_patterns_deep_nested_path() {
        let dir = TempDir::new().unwrap();
        let node = dir.path().join("project").join("node_modules").join("pkg");
        fs::create_dir_all(&node).unwrap();
        let nfd_name = format!("cafe\u{0301}.txt");
        fs::write(node.join(&nfd_name), "content").unwrap();
        // A sibling file outside node_modules should still be converted.
        let project = dir.path().join("project");
        fs::write(project.join(format!("re\u{0301}sume\u{0301}.txt")), "x").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: vec!["node_modules".to_string()],
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        // Only the file outside node_modules gets renamed.
        assert_eq!(
            stats.files_renamed, 1,
            "only the file outside node_modules should be renamed"
        );
        // The NFD file inside node_modules must not have been touched.
        assert!(
            node.join(&nfd_name).exists(),
            "excluded file must remain with its original NFD name"
        );
    }

    // ── exclude_patterns: pattern does NOT match a parent dir that is not the
    // watch root — guard against accidentally skipping unrelated paths whose
    // absolute ancestor happens to contain the excluded name.

    #[tokio::test]
    async fn test_exclude_does_not_match_above_root() {
        // Structure: /tmp/node_modules/<root>/file.txt
        // Root passed to convert_path is <root>; "node_modules" is ABOVE the
        // root so must not be excluded.
        let outer = TempDir::new().unwrap();
        let excluded_parent = outer.path().join("node_modules");
        let root = excluded_parent.join("myproject");
        fs::create_dir_all(&root).unwrap();
        let nfd_name = format!("cafe\u{0301}.txt");
        fs::write(root.join(&nfd_name), "content").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: vec!["node_modules".to_string()],
        };
        // Scan starting at `root`, not at `outer`.  The "node_modules" component
        // is above the scan root and must not cause exclusion.
        let stats = convert_path(&root, &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_renamed, 1,
            "file inside root must be renamed even though root lives inside a node_modules dir"
        );
    }

    // ── 100 MB file guard: oversized files are silently skipped for content ───

    #[tokio::test]
    async fn test_large_file_skipped_for_content_conversion() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("big.txt");

        // Write a file that is exactly MAX_CONTENT_BYTES + 1.
        // We use set_len (sparse file) so the test is fast.
        {
            let f = fs::File::create(&path).unwrap();
            f.set_len(100 * 1024 * 1024 + 1).unwrap();
        }

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_content_converted, 0,
            "file over 100 MB must be skipped"
        );
        assert!(
            stats.errors.is_empty(),
            "oversized file must not produce an error entry"
        );
    }

    // ── 100 MB boundary: file exactly at the limit is processed ──────────────

    #[tokio::test]
    async fn test_file_at_exact_limit_is_processed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("boundary.txt");
        // Build exactly MAX_CONTENT_BYTES of NFD content so it triggers conversion.
        // We use a 2-byte NFD sequence (e + combining acute) repeated to fill the limit.
        // 100 MB / 2 bytes = 50 M repetitions — too slow.  Instead write a 1-byte
        // ASCII file that is well under the guard and confirm it IS converted.
        // (The "at exact limit" guard uses >, so a file of exactly MAX bytes is allowed.)
        let nfd_content = "e\u{0301}"; // 2 bytes, well under 100 MB
        fs::write(&path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_content_converted, 1,
            "small file must not be skipped by the size guard"
        );
    }

    // ── rename_succeeded flag: content uses the original path when rename fails

    // Simulate a rename-then-content pass where convert_filenames=false so
    // rename_succeeded stays false. The content must still be read and converted
    // from the original path.
    #[tokio::test]
    async fn test_content_converted_at_original_path_when_no_rename() {
        let dir = TempDir::new().unwrap();
        // File has NFC name but NFD content — only content conversion matters.
        let path = dir.path().join("plain.txt");
        let nfd_content = "n\u{0303}"; // ñ in NFD
        fs::write(&path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false, // no rename attempted → rename_succeeded stays false
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_content_converted, 1);
        let result = fs::read_to_string(&path).unwrap();
        assert_eq!(result, "\u{00F1}", "ñ must be in NFC at the original path");
    }

    // ── rename_succeeded flag: combined pass uses the NFC path for content ────

    // When both filename and content conversion are on and rename succeeds,
    // the content read must target the new (NFC) path, not the original NFD path.
    #[tokio::test]
    async fn test_content_read_from_nfc_path_after_successful_rename() {
        let dir = TempDir::new().unwrap();
        let nfd_name = format!("n\u{0303}.txt"); // ñ.txt in NFD
        let nfc_path = dir.path().join("\u{00F1}.txt");
        let nfd_content = "u\u{0308}"; // ü in NFD inside the file
        fs::write(dir.path().join(&nfd_name), nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(stats.errors.is_empty(), "errors: {:?}", stats.errors);
        assert_eq!(stats.files_renamed, 1);
        assert_eq!(stats.files_content_converted, 1);
        // Content must be at the NFC-named file.
        let result = fs::read_to_string(&nfc_path).unwrap();
        assert_eq!(result, "\u{00FC}", "ü must be NFC inside the renamed file");
    }

    // ── Directory rename: children renamed before parent (contents_first) ─────

    // Verifies that a directory whose own name is NFD is renamed correctly even
    // when it contains files — WalkDir contents_first guarantees children are
    // processed before their parent, so the parent rename does not invalidate
    // child paths.
    #[tokio::test]
    async fn test_directory_with_nfd_name_and_nfd_children_renamed() {
        let dir = TempDir::new().unwrap();
        // Directory name in NFD: "cafe\u{0301}" → NFC "café"
        let nfd_dir_name = format!("cafe\u{0301}");
        let nfd_dir = dir.path().join(&nfd_dir_name);
        fs::create_dir(&nfd_dir).unwrap();
        // Child file inside the NFD directory — name also needs conversion.
        let nfd_child = format!("re\u{0301}sume\u{0301}.txt");
        fs::write(nfd_dir.join(&nfd_child), "hello").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(stats.errors.is_empty(), "errors: {:?}", stats.errors);
        // Both the directory and the file inside it must be renamed (2 renames).
        assert_eq!(
            stats.files_renamed, 2,
            "expected directory + child file both renamed"
        );
        // The NFC-named path must now be accessible.
        let nfc_dir = dir.path().join("caf\u{00E9}");
        assert!(nfc_dir.exists(), "NFC directory must exist after rename");
        let nfc_child = nfc_dir.join("r\u{00E9}sum\u{00E9}.txt");
        assert!(nfc_child.exists(), "NFC child file must exist after rename");
    }

    // ── Temp file name is unique per original filename ────────────────────────

    // Two NFD files in the same directory must produce distinct temp file names
    // so concurrent or sequential renames do not collide.  The temp name is
    // ".uninorm_tmp_<files_scanned_counter>" in convert_path (counter-based),
    // and ".uninorm_tmp_<original_filename>" in rename_if_needed (name-based).
    // This test covers convert_path: each entry gets a different counter value.
    #[tokio::test]
    async fn test_multiple_nfd_files_in_same_dir_no_collision() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(format!("cafe\u{0301}.txt")), "a").unwrap();
        fs::write(dir.path().join(format!("re\u{0301}sume\u{0301}.txt")), "b").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(stats.errors.is_empty(), "errors: {:?}", stats.errors);
        assert_eq!(stats.files_renamed, 2, "both NFD files must be renamed");
        // No stale temp files left behind.
        let stale: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".uninorm_tmp_"))
            .collect();
        assert!(
            stale.is_empty(),
            "temp files must be cleaned up: {:?}",
            stale
        );
    }

    // ── Atomic write: no temp file left after successful content conversion ────

    #[tokio::test]
    async fn test_no_temp_file_left_after_content_conversion() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "e\u{0301}").unwrap(); // NFD content

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        convert_path(dir.path(), &opts, |_| {}).await.unwrap();

        // The temp file used during atomic write is "{original}.uninorm_tmp".
        let tmp = dir.path().join("test.txt.uninorm_tmp");
        assert!(
            !tmp.exists(),
            "atomic temp file must be removed after successful write"
        );
    }

    // ── Rename conflict: NFC target already exists as a different file ─────────

    // On a case-sensitive filesystem (Linux tmpfs, APFS in case-sensitive mode)
    // both NFD and NFC names can coexist.  The code must detect the conflict via
    // same_inode and log it as an error rather than silently overwriting.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_rename_conflict_logged_as_error() {
        let dir = TempDir::new().unwrap();
        let nfd_name = format!("cafe\u{0301}.txt");
        let nfc_name = "caf\u{00E9}.txt";

        // On APFS the two names resolve to the same inode, so same_inode()
        // returns true and no conflict is recorded. Only run the conflict
        // assertion on case-sensitive filesystems where they are distinct inodes.
        let nfd_path = dir.path().join(&nfd_name);
        let nfc_path = dir.path().join(nfc_name);
        fs::write(&nfd_path, "original").unwrap();
        fs::write(&nfc_path, "conflict").unwrap();

        // Check if they are the same inode (APFS normalization-insensitive).
        let same = {
            use std::os::unix::fs::MetadataExt;
            let m1 = fs::metadata(&nfd_path).unwrap();
            let m2 = fs::metadata(&nfc_path).unwrap();
            m1.ino() == m2.ino() && m1.dev() == m2.dev()
        };

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();

        if same {
            // APFS: normalization-insensitive — treated as same file, rename succeeds.
            assert!(
                stats.errors.is_empty(),
                "APFS same-inode rename must not be an error"
            );
        } else {
            // Case-sensitive FS: distinct inodes → conflict must be logged.
            assert_eq!(
                stats.files_renamed, 0,
                "conflicting rename must not proceed"
            );
            assert!(
                stats.errors.iter().any(|e| e.contains("Rename conflict")),
                "expected a Rename conflict error, got: {:?}",
                stats.errors
            );
            // The conflict file must be untouched.
            assert_eq!(fs::read_to_string(&nfc_path).unwrap(), "conflict");
        }
    }

    // ── Watch safety: rename_if_needed returns None for already-NFC path ──────

    // After convert_path renames a file to its NFC form, the filesystem watcher
    // fires another event for the new NFC path.  rename_if_needed must return
    // None for that path so no second rename is attempted (no infinite loop).
    #[test]
    fn test_rename_if_needed_noop_for_nfc_path() {
        use crate::normalize::needs_filename_conversion;
        // Simulate the watch handler logic: if rename_if_needed returns None for
        // an already-NFC filename, no second rename fires.
        let nfc_name = "caf\u{00E9}.txt"; // already NFC
        assert!(
            !needs_filename_conversion(nfc_name),
            "NFC filename must not need conversion — watch loop would be infinite if it did"
        );
    }

    // ── Watch safety: rename_if_needed returns None for NFC Korean ───────────

    #[test]
    fn test_rename_if_needed_noop_for_nfc_korean() {
        use crate::normalize::needs_filename_conversion;
        assert!(
            !needs_filename_conversion("강남구.txt"),
            "NFC Korean filename must not trigger another rename"
        );
    }

    // ── Watch safety: rename_if_needed returns Some for NFD path ─────────────

    // Confirms the positive case: an NFD-named file does need conversion so the
    // watcher correctly fires the rename on the first event.
    #[test]
    fn test_rename_if_needed_triggers_for_nfd_filename() {
        use crate::normalize::needs_filename_conversion;
        let nfd_name = format!("cafe\u{0301}.txt");
        assert!(
            needs_filename_conversion(&nfd_name),
            "NFD filename must need conversion so the watcher renames it"
        );
    }

    // ── Exclude patterns: relative-path stripping prevents false positives ────

    // Guard against a watch-root whose absolute path contains a component that
    // matches an exclude pattern.  Before the fix this would cause all files
    // under that root to be excluded.
    //
    // Example: root = /home/user/node_modules/myproject
    // Pattern = "node_modules"
    // The relative path from root to any child never contains "node_modules",
    // so nothing should be excluded.
    #[test]
    fn test_exclude_relative_path_strips_watch_root_prefix() {
        use crate::normalize::needs_filename_conversion;
        use std::path::PathBuf;

        // Replicate the relative-path logic from rename_if_needed.
        let watch_root = PathBuf::from("/home/user/node_modules/myproject");
        let file_path = watch_root.join(format!("cafe\u{0301}.txt"));

        let exclude = vec!["node_modules".to_string()];
        let relative = [watch_root.as_path()]
            .iter()
            .find_map(|root| file_path.strip_prefix(root).ok())
            .unwrap_or(&file_path);

        let excluded = relative.components().any(|c| {
            if let std::path::Component::Normal(n) = c {
                let s = n.to_string_lossy();
                exclude.iter().any(|pat| s.as_ref() == pat.as_str())
            } else {
                false
            }
        });

        assert!(
            !excluded,
            "file directly under watch root must not be excluded even if root path contains 'node_modules'"
        );
        // And confirm the file itself needs conversion (watch should rename it).
        assert!(needs_filename_conversion(&format!("cafe\u{0301}.txt")));
    }

    // ── Exclude patterns: component inside relative path IS excluded ──────────

    #[test]
    fn test_exclude_relative_path_matches_subdir_component() {
        use std::path::PathBuf;

        let watch_root = PathBuf::from("/home/user/myproject");
        let file_path = watch_root
            .join("node_modules")
            .join("some_pkg")
            .join(format!("cafe\u{0301}.txt"));

        let exclude = vec!["node_modules".to_string()];
        let relative = [watch_root.as_path()]
            .iter()
            .find_map(|root| file_path.strip_prefix(root).ok())
            .unwrap_or(&file_path);

        let excluded = relative.components().any(|c| {
            if let std::path::Component::Normal(n) = c {
                let s = n.to_string_lossy();
                exclude.iter().any(|pat| s.as_ref() == pat.as_str())
            } else {
                false
            }
        });

        assert!(
            excluded,
            "path with 'node_modules' component under watch root must be excluded"
        );
    }

    // ── convert_path: NFC directory name not counted as rename ───────────────

    #[tokio::test]
    async fn test_nfc_directory_not_renamed() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("café"); // already NFC
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("file.txt"), "x").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_renamed, 0, "NFC directory must not be renamed");
        assert!(stats.errors.is_empty());
    }

    // ── convert_path on a nonexistent path propagates walk error ─────────────

    // WalkDir returns an error for the first entry when the root does not exist.
    // The error must be captured in stats.errors, not panic.
    #[tokio::test]
    async fn test_nonexistent_root_path_captured_as_walk_error() {
        use std::path::PathBuf;
        let nonexistent = PathBuf::from("/tmp/uninorm_test_nonexistent_xyzzy_12345");
        let opts = ConversionOptions::default();
        // convert_path itself returns Ok(stats); the walk error goes into stats.errors.
        let stats = convert_path(&nonexistent, &opts, |_| {}).await.unwrap();
        assert!(
            !stats.errors.is_empty(),
            "walking a nonexistent path must produce at least one error entry"
        );
        assert!(
            stats.errors.iter().any(|e| e.contains("Walk error")),
            "expected 'Walk error' in errors, got: {:?}",
            stats.errors
        );
    }

    // ── convert_path: content conversion dry-run with NFD filename ───────────

    // When both convert_filenames and convert_content are true but dry_run is
    // set, neither the rename nor the write must happen.  The counts must still
    // reflect what would have changed.
    #[tokio::test]
    async fn test_combined_dry_run_counts_but_does_not_modify() {
        let dir = TempDir::new().unwrap();
        let nfd_name = format!("cafe\u{0301}.txt");
        let nfd_path = dir.path().join(&nfd_name);
        let nfd_content = "e\u{0301}";
        fs::write(&nfd_path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: true,
            dry_run: true,
            recursive: false,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_renamed, 1,
            "dry-run should count filename changes"
        );
        assert_eq!(
            stats.files_content_converted, 1,
            "dry-run should count content changes"
        );
        // Original NFD file must still exist and be unchanged.
        assert!(
            nfd_path.exists(),
            "original NFD file must not be renamed in dry-run"
        );
        assert_eq!(
            fs::read_to_string(&nfd_path).unwrap(),
            nfd_content,
            "file content must be unchanged in dry-run"
        );
    }

    // ── Symlinks: not followed by default ─────────────────────────────────────

    #[tokio::test]
    async fn test_symlinks_not_followed_by_default() {
        let dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        // Write an NFD-content file in the target.
        fs::write(target_dir.path().join("linked.txt"), "e\u{0301}").unwrap();
        // Symlink inside dir pointing to target_dir.
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(target_dir.path(), &link).unwrap();
        #[cfg(not(unix))]
        {
            // Symlink test only meaningful on Unix; skip silently on Windows.
            return;
        }

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: Vec::new(),
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_content_converted, 0,
            "symlinked files must not be processed when follow_symlinks=false"
        );
    }
}
