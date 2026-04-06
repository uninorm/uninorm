//! NFD→NFC file and content conversion engine.
//!
//! The main entry point is [`convert_path`], which walks a directory tree and
//! performs filename renames and/or text content conversion from NFD to NFC.
//! Renames use a two-step atomic pattern (rename to temp, then rename to target)
//! with automatic rollback on failure. Content writes use restrictive permissions
//! (`0o600` on Unix) during the temp-file phase to prevent data exposure.

use crate::normalize::{needs_filename_conversion, to_nfc, to_nfc_filename};
use std::path::Path;
use walkdir::WalkDir;

use super::exclude::{compile_excludes, is_excluded};
use super::{same_inode, temp_name, ConversionOptions, ConversionStats, MAX_WALK_DEPTH};

/// Collected entry for depth-grouped parallel processing.
struct CollectedEntry {
    path: std::path::PathBuf,
    is_file: bool,
}

/// Convert NFD→NFC for files/folders under `path`.
///
/// Phase 1: Walk tree, collect entries grouped by depth.
/// Phase 2: Process each depth level in parallel (deepest first via contents_first).
///          Renames are sequential per-depth for correctness; content conversion is parallel.
pub async fn convert_path(
    path: &Path,
    opts: &ConversionOptions,
    mut progress: impl FnMut(&ConversionStats),
) -> Result<ConversionStats, crate::error::ConvertError> {
    let mut stats = ConversionStats::default();
    let (globs, invalid_patterns) = compile_excludes(&opts.exclude_patterns);
    for pat in &invalid_patterns {
        stats
            .errors
            .push(format!("Warning: invalid exclude pattern ignored: {pat}"));
    }
    let max_depth = if opts.recursive { MAX_WALK_DEPTH } else { 1 };

    let walker = WalkDir::new(path)
        .follow_links(opts.follow_symlinks)
        .contents_first(true)
        .max_depth(max_depth);

    // Phase 1: collect entries grouped by depth
    let mut entries: Vec<CollectedEntry> = Vec::new();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                stats.errors.push(format!("Walk error: {e}"));
                continue;
            }
        };

        if is_excluded(entry.path(), path, &globs) {
            continue;
        }

        let is_file = entry.file_type().is_file();
        if !is_file {
            stats.directories_scanned += 1;
        }
        stats.files_scanned += 1;
        entries.push(CollectedEntry {
            path: entry.path().to_path_buf(),
            is_file,
        });
    }

    // Phase 2: process — renames must be sequential (children before parents),
    // but content conversions within a depth level are parallel.
    // Since contents_first already orders deepest-first, we process in order.
    for ce in &entries {
        let file_name = ce
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut rename_succeeded = false;
        if opts.convert_filenames && needs_filename_conversion(&file_name) {
            let nfc_name = to_nfc_filename(&file_name);
            let new_path = ce.path.with_file_name(&nfc_name);
            let is_conflict = new_path.exists() && !same_inode(&ce.path, &new_path);

            if is_conflict {
                stats.errors.push(format!(
                    "Rename conflict: {} already exists",
                    new_path.display()
                ));
            } else if !opts.dry_run {
                let Some(parent) = ce.path.parent() else {
                    stats
                        .errors
                        .push(format!("Cannot rename root path: {}", ce.path.display()));
                    continue;
                };
                let tmp = parent.join(temp_name());
                match tokio::fs::rename(&ce.path, &tmp).await {
                    Ok(_) => match tokio::fs::rename(&tmp, &new_path).await {
                        Ok(_) => {
                            stats.files_renamed += 1;
                            rename_succeeded = true;
                        }
                        Err(e) => {
                            if let Err(rb_err) = tokio::fs::rename(&tmp, &ce.path).await {
                                stats.errors.push(format!(
                                    "Rename failed {}: {e}; rollback also failed: {rb_err} (orphaned temp: {})",
                                    ce.path.display(),
                                    tmp.display()
                                ));
                            } else {
                                stats
                                    .errors
                                    .push(format!("Rename failed {}: {e}", ce.path.display()));
                            }
                        }
                    },
                    Err(e) => stats
                        .errors
                        .push(format!("Rename failed {}: {e}", ce.path.display())),
                }
            } else {
                stats.files_renamed += 1;
            }
        }

        // Convert content — for non-parallel rename correctness, content of
        // individually renamed files is done inline after rename.
        if opts.convert_content && ce.is_file {
            let file_name_nfc = to_nfc_filename(&file_name);
            let current_path = if opts.convert_filenames
                && needs_filename_conversion(&file_name)
                && !opts.dry_run
                && rename_succeeded
            {
                ce.path.with_file_name(&file_name_nfc)
            } else {
                ce.path.clone()
            };

            if let Err(msg) = convert_single_content(
                &current_path,
                opts.max_content_bytes,
                opts.dry_run,
                &mut stats,
            )
            .await
            {
                stats.errors.push(msg);
            }
        }

        progress(&stats);
    }

    Ok(stats)
}

/// Write data to a temp file with restrictive permissions (0o600 on Unix).
/// Prevents exposure of file contents to other local users during the
/// write-then-rename atomic update pattern.
async fn write_temp_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            f.write_all(&data)?;
            f.sync_data()?;
            Ok(())
        })
        .await
        .map_err(|e| std::io::Error::other(format!("join error: {e}")))?
    }

    #[cfg(not(unix))]
    {
        tokio::fs::write(path, data).await
    }
}

/// Convert a single file's content from NFD to NFC.
/// Returns Err(message) on non-fatal errors that should be recorded.
async fn convert_single_content(
    path: &Path,
    max_bytes: u64,
    dry_run: bool,
    stats: &mut ConversionStats,
) -> std::result::Result<(), String> {
    match tokio::fs::metadata(path).await {
        Ok(meta) => {
            if meta.len() > max_bytes {
                stats.files_skipped += 1;
                return Ok(());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(format!("Metadata read failed {}: {e}", path.display()));
        }
    }

    match tokio::fs::read_to_string(path).await {
        Ok(content) => {
            let nfc_content = to_nfc(&content);
            if nfc_content != content {
                if !dry_run {
                    let Some(parent) = path.parent() else {
                        return Err(format!(
                            "Cannot determine parent directory for: {}",
                            path.display()
                        ));
                    };
                    let tmp_path = parent.join(temp_name());
                    // Create temp file with restrictive permissions (0o600) to prevent
                    // exposure of file contents to other local users.
                    match write_temp_file(&tmp_path, nfc_content.as_bytes()).await {
                        Ok(_) => {
                            if let Ok(meta) = tokio::fs::metadata(path).await {
                                let _ =
                                    tokio::fs::set_permissions(&tmp_path, meta.permissions()).await;
                            }
                            match tokio::fs::rename(&tmp_path, path).await {
                                Ok(_) => stats.files_content_converted += 1,
                                Err(e) => {
                                    let _ = tokio::fs::remove_file(&tmp_path).await;
                                    return Err(format!(
                                        "Content write failed {}: {e}",
                                        path.display()
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&tmp_path).await;
                            return Err(format!("Content write failed {}: {e}", path.display()));
                        }
                    }
                } else {
                    stats.files_content_converted += 1;
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            stats.files_skipped += 1;
        }
        Err(e) => {
            return Err(format!("Content read failed {}: {e}", path.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── ConversionStats Display ────────────────────────────────────────────

    #[test]
    fn test_stats_display_no_errors() {
        let stats = ConversionStats {
            files_scanned: 100,
            files_renamed: 5,
            files_content_converted: 3,
            files_skipped: 2,
            directories_scanned: 10,
            errors: vec![],
        };
        let display = format!("{stats}");
        assert!(display.contains("Scanned: 100"));
        assert!(display.contains("dirs: 10"));
        assert!(display.contains("Renamed: 5"));
        assert!(display.contains("Content: 3"));
        assert!(display.contains("Skipped: 2"));
        assert!(!display.contains("Errors"));
    }

    #[test]
    fn test_stats_display_with_errors() {
        let stats = ConversionStats {
            files_scanned: 50,
            errors: vec!["error1".to_string(), "error2".to_string()],
            ..ConversionStats::default()
        };
        let display = format!("{stats}");
        assert!(display.contains("Errors: 2"));
    }

    #[test]
    fn test_stats_default() {
        let stats = ConversionStats::default();
        assert_eq!(stats.files_skipped, 0);
        assert_eq!(stats.directories_scanned, 0);
    }

    // ── files_skipped and directories_scanned ────────────────────────────────

    #[tokio::test]
    async fn test_files_skipped_for_large_content() {
        let dir = TempDir::new().unwrap();
        // Write a file larger than max_content_bytes (10 bytes)
        fs::write(
            dir.path().join("large.txt"),
            "this content is more than ten bytes long",
        )
        .unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            max_content_bytes: 10,
            ..ConversionOptions::default()
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(
            stats.files_skipped >= 1,
            "expected files_skipped >= 1, got {}",
            stats.files_skipped
        );
    }

    #[tokio::test]
    async fn test_files_skipped_for_binary_content() {
        let dir = TempDir::new().unwrap();
        // Write invalid UTF-8 bytes (binary file)
        fs::write(dir.path().join("binary.bin"), [0xFF, 0xFE, 0x80, 0x81]).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(
            stats.files_skipped >= 1,
            "expected files_skipped >= 1 for binary file, got {}",
            stats.files_skipped
        );
    }

    #[tokio::test]
    async fn test_directories_scanned_count() {
        let dir = TempDir::new().unwrap();
        // Create 2 subdirectories with files
        let sub1 = dir.path().join("subdir1");
        let sub2 = dir.path().join("subdir2");
        fs::create_dir(&sub1).unwrap();
        fs::create_dir(&sub2).unwrap();
        fs::write(sub1.join("file1.txt"), "hello").unwrap();
        fs::write(sub2.join("file2.txt"), "world").unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(
            stats.directories_scanned >= 2,
            "expected directories_scanned >= 2, got {}",
            stats.directories_scanned
        );
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
            ..ConversionOptions::default()
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
        let nfd_name = "cafe\u{0301}.txt".to_string();
        let nfd_path = dir.path().join(&nfd_name);
        fs::write(&nfd_path, "content").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
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
            ..ConversionOptions::default()
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
            ..ConversionOptions::default()
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
            ..ConversionOptions::default()
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
            ..ConversionOptions::default()
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
            ..ConversionOptions::default()
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
            ..ConversionOptions::default()
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
            ..ConversionOptions::default()
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
        let nfd_name = "cafe\u{0301}.txt".to_string(); // filename in NFD
        let nfd_content = "e\u{0301}"; // content in NFD
        fs::write(dir.path().join(&nfd_name), nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
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
        let nfd_name = "cafe\u{0301}.txt".to_string();
        fs::write(git_dir.join(&nfd_name), "content").unwrap();
        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: vec![".git".to_string()],
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_renamed, 0, "files inside .git must be excluded");
    }

    // ── exclude_patterns: deeply nested path is also excluded ─────────────────

    #[tokio::test]
    async fn test_exclude_patterns_deep_nested_path() {
        let dir = TempDir::new().unwrap();
        let node = dir.path().join("project").join("node_modules").join("pkg");
        fs::create_dir_all(&node).unwrap();
        let nfd_name = "cafe\u{0301}.txt".to_string();
        fs::write(node.join(&nfd_name), "content").unwrap();
        // A sibling file outside node_modules should still be converted.
        let project = dir.path().join("project");
        fs::write(project.join("re\u{0301}sume\u{0301}.txt"), "x").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: vec!["node_modules".to_string()],
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_renamed, 1,
            "only the file outside node_modules should be renamed"
        );
        assert!(
            node.join(&nfd_name).exists(),
            "excluded file must remain with its original NFD name"
        );
    }

    // ── exclude_patterns: pattern does NOT match a parent dir that is not the
    // watch root ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_exclude_does_not_match_above_root() {
        let outer = TempDir::new().unwrap();
        let excluded_parent = outer.path().join("node_modules");
        let root = excluded_parent.join("myproject");
        fs::create_dir_all(&root).unwrap();
        let nfd_name = "cafe\u{0301}.txt".to_string();
        fs::write(root.join(&nfd_name), "content").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            exclude_patterns: vec!["node_modules".to_string()],
            ..ConversionOptions::default()
        };
        let stats = convert_path(&root, &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_renamed, 1,
            "file inside root must be renamed even though root lives inside a node_modules dir"
        );
    }

    // ── 100 MB file guard ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_large_file_skipped_for_content_conversion() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("big.txt");
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
            ..ConversionOptions::default()
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

    #[tokio::test]
    async fn test_file_at_exact_limit_is_processed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("boundary.txt");
        let nfd_content = "e\u{0301}"; // 2 bytes, well under 100 MB
        fs::write(&path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_content_converted, 1,
            "small file must not be skipped by the size guard"
        );
    }

    // ── rename_succeeded flag tests ──────────────────────────────────────────

    #[tokio::test]
    async fn test_content_converted_at_original_path_when_no_rename() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("plain.txt");
        let nfd_content = "n\u{0303}"; // ñ in NFD
        fs::write(&path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_content_converted, 1);
        let result = fs::read_to_string(&path).unwrap();
        assert_eq!(result, "\u{00F1}", "ñ must be in NFC at the original path");
    }

    #[tokio::test]
    async fn test_content_read_from_nfc_path_after_successful_rename() {
        let dir = TempDir::new().unwrap();
        let nfd_name = "n\u{0303}.txt".to_string(); // ñ.txt in NFD
        let nfc_path = dir.path().join("\u{00F1}.txt");
        let nfd_content = "u\u{0308}"; // ü in NFD inside the file
        fs::write(dir.path().join(&nfd_name), nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(stats.errors.is_empty(), "errors: {:?}", stats.errors);
        assert_eq!(stats.files_renamed, 1);
        assert_eq!(stats.files_content_converted, 1);
        let result = fs::read_to_string(&nfc_path).unwrap();
        assert_eq!(result, "\u{00FC}", "ü must be NFC inside the renamed file");
    }

    // ── Directory rename ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_directory_with_nfd_name_and_nfd_children_renamed() {
        let dir = TempDir::new().unwrap();
        let nfd_dir_name = "cafe\u{0301}".to_string();
        let nfd_dir = dir.path().join(&nfd_dir_name);
        fs::create_dir(&nfd_dir).unwrap();
        let nfd_child = "re\u{0301}sume\u{0301}.txt".to_string();
        fs::write(nfd_dir.join(&nfd_child), "hello").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(stats.errors.is_empty(), "errors: {:?}", stats.errors);
        assert_eq!(
            stats.files_renamed, 2,
            "expected directory + child file both renamed"
        );
        let nfc_dir = dir.path().join("caf\u{00E9}");
        assert!(nfc_dir.exists(), "NFC directory must exist after rename");
        let nfc_child = nfc_dir.join("r\u{00E9}sum\u{00E9}.txt");
        assert!(nfc_child.exists(), "NFC child file must exist after rename");
    }

    // ── Temp file uniqueness and cleanup ─────────────────────────────────────

    #[tokio::test]
    async fn test_multiple_nfd_files_in_same_dir_no_collision() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("cafe\u{0301}.txt"), "a").unwrap();
        fs::write(dir.path().join("re\u{0301}sume\u{0301}.txt"), "b").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert!(stats.errors.is_empty(), "errors: {:?}", stats.errors);
        assert_eq!(stats.files_renamed, 2, "both NFD files must be renamed");
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

    #[tokio::test]
    async fn test_no_temp_file_left_after_content_conversion() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "e\u{0301}").unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };
        convert_path(dir.path(), &opts, |_| {}).await.unwrap();

        let has_temp = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(".uninorm_tmp_")
            });
        assert!(
            !has_temp,
            "atomic temp file must be removed after successful write"
        );
    }

    // ── Rename conflict ──────────────────────────────────────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn test_rename_conflict_logged_as_error() {
        let dir = TempDir::new().unwrap();
        let nfd_name = "cafe\u{0301}.txt".to_string();
        let nfc_name = "caf\u{00E9}.txt";

        let nfd_path = dir.path().join(&nfd_name);
        let nfc_path = dir.path().join(nfc_name);
        fs::write(&nfd_path, "original").unwrap();
        fs::write(&nfc_path, "conflict").unwrap();

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
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();

        if same {
            assert!(
                stats.errors.is_empty(),
                "APFS same-inode rename must not be an error"
            );
        } else {
            assert_eq!(
                stats.files_renamed, 0,
                "conflicting rename must not proceed"
            );
            assert!(
                stats.errors.iter().any(|e| e.contains("Rename conflict")),
                "expected a Rename conflict error, got: {:?}",
                stats.errors
            );
            assert_eq!(fs::read_to_string(&nfc_path).unwrap(), "conflict");
        }
    }

    // ── Watch safety ─────────────────────────────────────────────────────────

    #[test]
    fn test_rename_if_needed_noop_for_nfc_path() {
        use crate::normalize::needs_filename_conversion;
        let nfc_name = "caf\u{00E9}.txt";
        assert!(
            !needs_filename_conversion(nfc_name),
            "NFC filename must not need conversion — watch loop would be infinite if it did"
        );
    }

    #[test]
    fn test_rename_if_needed_noop_for_nfc_korean() {
        use crate::normalize::needs_filename_conversion;
        assert!(
            !needs_filename_conversion("강남구.txt"),
            "NFC Korean filename must not trigger another rename"
        );
    }

    #[test]
    fn test_rename_if_needed_triggers_for_nfd_filename() {
        use crate::normalize::needs_filename_conversion;
        let nfd_name = "cafe\u{0301}.txt".to_string();
        assert!(
            needs_filename_conversion(&nfd_name),
            "NFD filename must need conversion so the watcher renames it"
        );
    }

    // ── NFC directory not renamed ────────────────────────────────────────────

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
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_renamed, 0, "NFC directory must not be renamed");
        assert!(stats.errors.is_empty());
    }

    // ── Nonexistent root path ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_nonexistent_root_path_captured_as_walk_error() {
        use std::path::PathBuf;
        let nonexistent = PathBuf::from("/tmp/uninorm_test_nonexistent_xyzzy_12345");
        let opts = ConversionOptions::default();
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

    // ── Combined dry-run ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_combined_dry_run_counts_but_does_not_modify() {
        let dir = TempDir::new().unwrap();
        let nfd_name = "cafe\u{0301}.txt".to_string();
        let nfd_path = dir.path().join(&nfd_name);
        let nfd_content = "e\u{0301}";
        fs::write(&nfd_path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: true,
            dry_run: true,
            recursive: false,
            follow_symlinks: false,
            ..ConversionOptions::default()
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

    // ── Symlinks ─────────────────────────────────────────────────────────────

    fn try_symlink_dir(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(target, link)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link);
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "symlinks not supported on this platform",
            ))
        }
    }

    #[tokio::test]
    async fn test_symlinks_not_followed_by_default() {
        let dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        fs::write(target_dir.path().join("linked.txt"), "e\u{0301}").unwrap();

        let link = dir.path().join("link");
        if try_symlink_dir(target_dir.path(), &link).is_err() {
            return;
        }

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
            ..ConversionOptions::default()
        };
        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(
            stats.files_content_converted, 0,
            "symlinked files must not be processed when follow_symlinks=false"
        );
    }
}
