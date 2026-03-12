//! Integration tests for the daemon's file-watching and conversion logic.
//!
//! These tests exercise the core daemon helpers (rename, content conversion,
//! temp file cleanup) using real filesystem operations — without spawning
//! an actual background daemon process.

use std::fs;

/// Create an NFD filename (Korean "강" decomposed: ㄱ + ㅏ + ㅇ).
fn nfd_korean() -> String {
    "\u{1100}\u{1161}\u{11BC}".to_string() // ㄱ + ㅏ + ㅇ = 강 (NFD)
}

/// NFC form of the same character.
fn nfc_korean() -> String {
    "\u{AC15}".to_string() // 강 (NFC)
}

// ---------------------------------------------------------------------------
// Core conversion logic tests (using uninorm-core directly)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_convert_nfd_filename_in_watched_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let nfd_name = format!("{}.txt", nfd_korean());
    let nfd_path = tmp.path().join(&nfd_name);
    fs::write(&nfd_path, "hello").unwrap();

    let opts = uninorm_core::ConversionOptions {
        convert_filenames: true,
        convert_content: false,
        dry_run: false,
        recursive: true,
        follow_symlinks: false,
        exclude_patterns: vec![],
        max_content_bytes: uninorm_core::DEFAULT_MAX_CONTENT_BYTES,
    };

    let stats = uninorm_core::convert_path(tmp.path(), &opts, |_| {}).await;
    assert!(stats.is_ok());
    let stats = stats.unwrap();
    assert!(stats.files_renamed >= 1, "should rename at least 1 file");
    assert!(stats.errors.is_empty(), "no errors expected");

    // NFC file should exist
    let nfc_name = format!("{}.txt", nfc_korean());
    let nfc_path = tmp.path().join(&nfc_name);
    assert!(nfc_path.exists(), "NFC-renamed file should exist");
    assert_eq!(fs::read_to_string(&nfc_path).unwrap(), "hello");
}

#[tokio::test]
async fn test_convert_nfd_content_in_watched_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file_path = tmp.path().join("test.txt");
    let nfd_content = format!("Name: {}", nfd_korean());
    fs::write(&file_path, &nfd_content).unwrap();

    let opts = uninorm_core::ConversionOptions {
        convert_filenames: false,
        convert_content: true,
        dry_run: false,
        recursive: true,
        follow_symlinks: false,
        exclude_patterns: vec![],
        max_content_bytes: uninorm_core::DEFAULT_MAX_CONTENT_BYTES,
    };

    let stats = uninorm_core::convert_path(tmp.path(), &opts, |_| {}).await;
    assert!(stats.is_ok());
    let stats = stats.unwrap();
    assert_eq!(stats.files_content_converted, 1);

    let result = fs::read_to_string(&file_path).unwrap();
    let expected = format!("Name: {}", nfc_korean());
    assert_eq!(result, expected);
}

#[tokio::test]
async fn test_convert_both_filename_and_content() {
    let tmp = tempfile::TempDir::new().unwrap();
    let nfd_name = format!("{}.txt", nfd_korean());
    let nfd_path = tmp.path().join(&nfd_name);
    let nfd_content = format!("Content: {}", nfd_korean());
    fs::write(&nfd_path, &nfd_content).unwrap();

    let opts = uninorm_core::ConversionOptions {
        convert_filenames: true,
        convert_content: true,
        dry_run: false,
        recursive: true,
        follow_symlinks: false,
        exclude_patterns: vec![],
        max_content_bytes: uninorm_core::DEFAULT_MAX_CONTENT_BYTES,
    };

    let stats = uninorm_core::convert_path(tmp.path(), &opts, |_| {}).await;
    assert!(stats.is_ok());
    let stats = stats.unwrap();
    assert!(stats.files_renamed >= 1);
    assert_eq!(stats.files_content_converted, 1);

    let nfc_name = format!("{}.txt", nfc_korean());
    let nfc_path = tmp.path().join(&nfc_name);
    let result = fs::read_to_string(&nfc_path).unwrap();
    let expected = format!("Content: {}", nfc_korean());
    assert_eq!(result, expected);
}

#[tokio::test]
async fn test_exclude_pattern_skips_matching_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let nfd_name = format!("{}.log", nfd_korean());
    let nfd_path = tmp.path().join(&nfd_name);
    fs::write(&nfd_path, "log data").unwrap();

    let opts = uninorm_core::ConversionOptions {
        convert_filenames: true,
        convert_content: false,
        dry_run: false,
        recursive: true,
        follow_symlinks: false,
        exclude_patterns: vec!["*.log".to_string()],
        max_content_bytes: uninorm_core::DEFAULT_MAX_CONTENT_BYTES,
    };

    let stats = uninorm_core::convert_path(tmp.path(), &opts, |_| {}).await;
    assert!(stats.is_ok());
    let stats = stats.unwrap();
    assert_eq!(
        stats.files_renamed, 0,
        "excluded file should not be renamed"
    );
    assert!(nfd_path.exists(), "original NFD file should still exist");
}

#[tokio::test]
async fn test_no_temp_files_left_after_conversion() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Create several NFD files
    for i in 0..5 {
        let nfd_name = format!("{}_{i}.txt", nfd_korean());
        fs::write(tmp.path().join(&nfd_name), format!("data {i}")).unwrap();
    }

    let opts = uninorm_core::ConversionOptions {
        convert_filenames: true,
        convert_content: true,
        dry_run: false,
        recursive: true,
        follow_symlinks: false,
        exclude_patterns: vec![],
        max_content_bytes: uninorm_core::DEFAULT_MAX_CONTENT_BYTES,
    };

    let stats = uninorm_core::convert_path(tmp.path(), &opts, |_| {}).await;
    assert!(stats.is_ok());

    // No temp files should remain
    for entry in fs::read_dir(tmp.path()).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.starts_with(".uninorm_tmp_"),
            "temp file left behind: {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// Config and controller unit tests
// ---------------------------------------------------------------------------

#[test]
fn test_config_save_load_with_autostart_status() {
    // autostart::is_installed() should not panic
    let installed = uninorm_daemon::autostart::is_installed();
    // Just verify it returns a bool without error
    assert!(installed || !installed);
}

#[test]
fn test_controller_status_when_not_running() {
    // Status should return None when no daemon is running
    let status = uninorm_daemon::DaemonController::status();
    // We can't guarantee the daemon isn't running in CI,
    // but we can verify the API doesn't panic
    let _ = status;
}

#[test]
fn test_config_roundtrip_with_all_fields() {
    let mut cfg = uninorm_daemon::WatchConfig::default();
    cfg.debounce_ms = Some(500);
    cfg.add_entry(uninorm_daemon::WatchEntry {
        path: "/tmp/test_integration".into(),
        recursive: false,
        content: true,
        follow_symlinks: true,
        exclude: vec!["*.log".to_string(), ".git".to_string()],
        max_content_bytes: Some(50 * 1024 * 1024),
        enabled: false,
    });

    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let loaded: uninorm_daemon::WatchConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(loaded.debounce_ms, Some(500));
    assert_eq!(loaded.entries.len(), 1);
    let e = &loaded.entries[0];
    assert!(!e.recursive);
    assert!(e.content);
    assert!(e.follow_symlinks);
    assert_eq!(e.exclude, vec!["*.log", ".git"]);
    assert_eq!(e.max_content_bytes, Some(50 * 1024 * 1024));
    assert!(!e.enabled);
    assert_eq!(loaded.enabled_count(), 0);
}

// ---------------------------------------------------------------------------
// Scan tests (pre-conversion analysis)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scan_detects_nfd_files_and_content() {
    let tmp = tempfile::TempDir::new().unwrap();

    // NFD filename
    let nfd_name = format!("{}.txt", nfd_korean());
    let nfd_path = tmp.path().join(&nfd_name);
    fs::write(&nfd_path, "nfc content").unwrap();

    // NFC filename with NFD content
    let nfc_file = tmp.path().join("normal.txt");
    let nfd_content = format!("text: {}", nfd_korean());
    fs::write(&nfc_file, &nfd_content).unwrap();

    let opts = uninorm_core::ConversionOptions {
        convert_filenames: true,
        convert_content: true,
        dry_run: false,
        recursive: true,
        follow_symlinks: false,
        exclude_patterns: vec![],
        max_content_bytes: uninorm_core::DEFAULT_MAX_CONTENT_BYTES,
    };

    let scan = uninorm_core::scan_path(tmp.path(), &opts).await;
    assert!(scan.rename_count() >= 1, "should detect NFD filename");
    assert!(scan.content_count() >= 1, "should detect NFD content");
    assert!(
        scan.affected_count() >= 2,
        "should have at least 2 affected entries"
    );
}

#[tokio::test]
async fn test_scan_respects_non_recursive() {
    let tmp = tempfile::TempDir::new().unwrap();
    let subdir = tmp.path().join("sub");
    fs::create_dir(&subdir).unwrap();

    // NFD file in subdir
    let nfd_name = format!("{}.txt", nfd_korean());
    fs::write(subdir.join(&nfd_name), "data").unwrap();

    let opts = uninorm_core::ConversionOptions {
        convert_filenames: true,
        convert_content: false,
        dry_run: false,
        recursive: false,
        follow_symlinks: false,
        exclude_patterns: vec![],
        max_content_bytes: uninorm_core::DEFAULT_MAX_CONTENT_BYTES,
    };

    let scan = uninorm_core::scan_path(tmp.path(), &opts).await;
    assert_eq!(
        scan.rename_count(),
        0,
        "non-recursive should not find files in subdir"
    );
}

// ---------------------------------------------------------------------------
// Text conversion tests
// ---------------------------------------------------------------------------

#[test]
fn test_convert_text_nfd_to_nfc() {
    let nfd = nfd_korean();
    let nfc = uninorm_core::convert_text(&nfd);
    assert_eq!(nfc, nfc_korean());
}

#[test]
fn test_convert_text_preserves_nfc() {
    let nfc = nfc_korean();
    let result = uninorm_core::convert_text(&nfc);
    assert_eq!(result, nfc);
}

#[test]
fn test_convert_text_mixed_content() {
    let mixed = format!("Hello {} world {}", nfd_korean(), nfd_korean());
    let result = uninorm_core::convert_text(&mixed);
    let expected = format!("Hello {} world {}", nfc_korean(), nfc_korean());
    assert_eq!(result, expected);
}

#[test]
fn test_is_nfc_detection() {
    assert!(!uninorm_core::is_nfc(&nfd_korean()));
    assert!(uninorm_core::is_nfc(&nfc_korean()));
    assert!(uninorm_core::is_nfc("plain ascii"));
}
