use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::config::{self, WatchConfig};
use crate::error::DaemonError;

/// Maximum log file size before rotation (5 MB).
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Maximum number of pending events before dropping new ones.
const MAX_PENDING: usize = 10_000;

/// Spawn the daemon as a detached background process.
pub fn spawn_daemon() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            std::process::Command::new(exe)
                .arg("daemon")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                })
                .spawn()?;
        }
    }

    #[cfg(not(unix))]
    {
        std::process::Command::new(exe)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }

    Ok(())
}

/// Mutex protecting log rotation from concurrent access.
static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Append a timestamped log entry, with size-based rotation.
pub fn append_log(message: &str) {
    let Ok(path) = config::log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Rotate if over limit (keep 2 generations: .log.1, .log.2)
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let rot1 = path.with_extension("log.1");
            let rot2 = path.with_extension("log.2");
            let _ = std::fs::rename(&rot1, &rot2);
            let _ = std::fs::rename(&path, &rot1);
        }
    }

    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{ts}] {message}\n");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Check if a filename is a uninorm temp file.
fn is_temp_file(name: &str) -> bool {
    name.starts_with(".uninorm_tmp_")
}

/// Clean up stale temp files left by a previous crash.
fn cleanup_stale_temps(wc: &WatchConfig) {
    for entry in &wc.entries {
        let walker = walkdir::WalkDir::new(&entry.path).max_depth(if entry.recursive {
            usize::MAX
        } else {
            1
        });
        for dir_entry in walker.into_iter().flatten() {
            if let Some(name) = dir_entry.file_name().to_str() {
                if is_temp_file(name) {
                    let path = dir_entry.path();
                    // Preserve data: rename to .uninorm_orphan_ instead of deleting
                    let orphan_name = name.replace(".uninorm_tmp_", ".uninorm_orphan_");
                    let orphan_path = path.with_file_name(&orphan_name);
                    match std::fs::rename(path, &orphan_path) {
                        Ok(_) => append_log(&format!(
                            "Warning: recovered stale temp as orphan: {} (please review and rename manually)",
                            orphan_path.display()
                        )),
                        Err(_) => {
                            // If rename fails, leave the temp file as-is rather than deleting data
                            append_log(&format!(
                                "Warning: stale temp file found, could not recover: {}",
                                path.display()
                            ));
                        }
                    }
                }
            }
        }
    }
}

/// Pre-compiled watch entry with glob set for efficient matching.
struct CompiledEntry<'a> {
    entry: &'a config::WatchEntry,
    globs: globset::GlobSet,
}

/// Lightweight view for use inside spawn_blocking closures.
struct InlineCompiledEntry<'a> {
    entry_path: &'a Path,
    globs: &'a globset::GlobSet,
}

/// Rename variant that works with InlineCompiledEntry (for spawn_blocking).
fn rename_if_needed_inline(
    path: &Path,
    ce: &InlineCompiledEntry<'_>,
) -> (Option<String>, Option<PathBuf>) {
    let Some(file_name_os) = path.file_name() else {
        return (None, None);
    };
    let file_name = file_name_os.to_string_lossy();

    if uninorm_core::is_excluded(path, ce.entry_path, ce.globs) {
        return (None, None);
    }

    if !uninorm_core::needs_filename_conversion(&file_name) {
        return (None, None);
    }

    let nfc_name = uninorm_core::to_nfc_filename(&file_name);
    let new_path = path.with_file_name(&nfc_name);
    let Some(parent) = path.parent() else {
        return (None, None);
    };

    let is_conflict = new_path.exists() && !uninorm_core::same_inode(path, &new_path);
    if is_conflict {
        return (
            Some(format!(
                "Conflict: skipping {file_name} (NFC target already exists)"
            )),
            None,
        );
    }

    let tmp = parent.join(uninorm_core::temp_name());

    match std::fs::rename(path, &tmp) {
        Ok(_) => match std::fs::rename(&tmp, &new_path) {
            Ok(_) => (
                Some(format!("Renamed: {file_name} -> {nfc_name}")),
                Some(new_path),
            ),
            Err(e) => {
                if let Err(rb_err) = std::fs::rename(&tmp, path) {
                    return (
                        Some(format!(
                            "Error: rename failed for {file_name}: {e}; rollback also failed: {rb_err} (orphaned temp: {})",
                            tmp.display()
                        )),
                        None,
                    );
                }
                (
                    Some(format!("Error: rename failed for {file_name}: {e}")),
                    None,
                )
            }
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => (None, None),
        Err(e) => (
            Some(format!("Error: rename failed for {file_name}: {e}")),
            None,
        ),
    }
}

/// Convert file content from NFD to NFC if needed.
fn convert_content_if_needed(path: &Path, max_bytes: u64) -> Option<String> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            return Some(format!(
                "Error: cannot read metadata for {}: {e}",
                path.display()
            ));
        }
    };

    if !meta.is_file() || meta.len() > max_bytes {
        return None;
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(ref e) if e.kind() == std::io::ErrorKind::InvalidData => return None,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            return Some(format!("Error: cannot read {}: {e}", path.display()));
        }
    };

    let nfc = uninorm_core::convert_text(&content);
    if nfc == content {
        return None;
    }

    let parent = path.parent()?;
    let tmp_path = parent.join(uninorm_core::temp_name());

    match std::fs::write(&tmp_path, nfc.as_bytes()) {
        Ok(_) => {
            let _ = std::fs::set_permissions(&tmp_path, meta.permissions());
            match std::fs::rename(&tmp_path, path) {
                Ok(_) => Some(format!("Content converted: {}", path.display())),
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp_path);
                    Some(format!(
                        "Error: content write failed for {}: {e}",
                        path.display()
                    ))
                }
            }
        }
        Err(e) => Some(format!(
            "Error: content write failed for {}: {e}",
            path.display()
        )),
    }
}

/// Find which CompiledEntry a path belongs to.
fn find_entry_for_path<'a>(
    path: &Path,
    entries: &'a [CompiledEntry<'a>],
) -> Option<&'a CompiledEntry<'a>> {
    entries.iter().find(|ce| path.starts_with(&ce.entry.path))
}

/// Guard that removes the PID file when dropped (panic, early return, normal exit).
struct PidGuard;

impl Drop for PidGuard {
    fn drop(&mut self) {
        config::remove_pid();
    }
}

/// Main daemon loop. Called by the hidden daemon subcommand.
pub async fn run_daemon() -> std::result::Result<(), DaemonError> {
    config::write_pid(std::process::id())?;
    let _pid_guard = PidGuard;
    append_log("Daemon started");

    run_daemon_platform().await
}

#[cfg(unix)]
async fn run_daemon_platform() -> std::result::Result<(), DaemonError> {
    use notify::Watcher;
    use tokio::sync::mpsc;

    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    // Outer loop: reload config on SIGHUP
    loop {
        let watch_config = WatchConfig::load()?;
        if watch_config.enabled_count() == 0 {
            append_log("No enabled watch entries -- daemon exiting");
            return Ok(());
        }

        let debounce_ms = watch_config.debounce_ms.unwrap_or(300).max(10);

        {
            let cfg_ref = watch_config.clone();
            tokio::task::spawn_blocking(move || cleanup_stale_temps(&cfg_ref)).await?;
        }

        // Pre-compile glob sets for enabled entries only
        let compiled: Vec<CompiledEntry<'_>> = watch_config
            .entries
            .iter()
            .filter(|e| e.enabled)
            .map(|e| CompiledEntry {
                entry: e,
                globs: {
                    let (set, invalid) = uninorm_core::compile_excludes(&e.exclude);
                    for pat in &invalid {
                        append_log(&format!("Warning: invalid exclude pattern ignored: {pat}"));
                    }
                    set
                },
            })
            .collect();

        for ce in &compiled {
            append_log(&format!("Watching: {}", ce.entry.path.display()));
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        )?;

        let mut watch_ok = 0usize;
        for ce in &compiled {
            let mode = if ce.entry.recursive {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            };
            match watcher.watch(&ce.entry.path, mode) {
                Ok(_) => watch_ok += 1,
                Err(e) => append_log(&format!("Error watching {}: {e}", ce.entry.path.display())),
            }
        }

        if watch_ok == 0 {
            append_log("All watch paths failed -- daemon exiting");
            return Err(DaemonError::AllWatchesFailed);
        }

        // Inner loop: process events with debounce
        let mut pending_paths: HashMap<PathBuf, bool> = HashMap::new();
        let debounce_dur = std::time::Duration::from_millis(debounce_ms);
        let mut debounce_timer = tokio::time::interval(debounce_dur);
        debounce_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        debounce_timer.tick().await;

        loop {
            tokio::select! {
                Some(result) = rx.recv() => {
                    match result {
                        Ok(event) => {
                            use notify::EventKind;
                            let is_name_event = matches!(
                                event.kind,
                                EventKind::Create(_)
                                | EventKind::Modify(notify::event::ModifyKind::Name(_))
                                | EventKind::Any
                            );
                            let is_data_event = matches!(
                                event.kind,
                                EventKind::Modify(notify::event::ModifyKind::Data(_))
                            );

                            if !is_name_event && !is_data_event {
                                continue;
                            }

                            for path in event.paths {
                                if path.file_name()
                                    .is_some_and(|n: &std::ffi::OsStr| is_temp_file(&n.to_string_lossy()))
                                {
                                    continue;
                                }
                                if pending_paths.len() >= MAX_PENDING && !pending_paths.contains_key(&path) {
                                    append_log("Warning: pending event queue full, dropping event");
                                    continue;
                                }
                                let existing_is_name = pending_paths.get(&path).copied().unwrap_or(false);
                                pending_paths.insert(path, existing_is_name || is_name_event);
                            }
                        }
                        Err(e) => append_log(&format!("Watch error: {e}")),
                    }
                }
                _ = debounce_timer.tick() => {
                    if pending_paths.is_empty() {
                        continue;
                    }

                    let batch = std::mem::take(&mut pending_paths);

                    // Group by parent directory to serialize renames within each dir
                    // (prevents TOCTOU races on conflict checks), while parallelizing across dirs.
                    let mut by_dir: HashMap<PathBuf, Vec<(PathBuf, bool)>> = HashMap::new();
                    let compiled_ref = &compiled;
                    for (path, is_name_event) in batch {
                        let dir = path.parent().unwrap_or(&path).to_path_buf();
                        by_dir.entry(dir).or_default().push((path, is_name_event));
                    }

                    let mut handles = Vec::new();
                    for (_, dir_entries) in by_dir {
                        // Collect per-directory context
                        let mut tasks: Vec<(PathBuf, bool, PathBuf, globset::GlobSet, bool, bool, u64)> = Vec::new();
                        for (path, is_name) in dir_entries {
                            let Some(ce) = find_entry_for_path(&path, compiled_ref) else {
                                continue;
                            };
                            tasks.push((
                                path,
                                is_name,
                                ce.entry.path.clone(),
                                ce.globs.clone(),
                                ce.entry.content,
                                ce.entry.follow_symlinks,
                                ce.entry.max_content_bytes
                                    .unwrap_or(uninorm_core::DEFAULT_MAX_CONTENT_BYTES),
                            ));
                        }
                        if tasks.is_empty() {
                            continue;
                        }

                        // One spawn_blocking per directory — sequential within, parallel across
                        handles.push(tokio::task::spawn_blocking(move || {
                            let mut msgs = Vec::new();
                            for (path, is_name, entry_path, globs, content_enabled, follow_symlinks, max_bytes) in tasks {
                                if !follow_symlinks {
                                    if let Ok(m) = std::fs::symlink_metadata(&path) {
                                        if m.file_type().is_symlink() {
                                            continue;
                                        }
                                    }
                                }

                                let nfc_path = if is_name {
                                    let ce_inline = InlineCompiledEntry { entry_path: &entry_path, globs: &globs };
                                    let (msg, renamed_path) = rename_if_needed_inline(&path, &ce_inline);
                                    if let Some(msg) = msg {
                                        msgs.push(msg);
                                    }
                                    renamed_path
                                } else {
                                    None
                                };

                                if content_enabled {
                                    let target = nfc_path.as_deref().unwrap_or(&path);
                                    if !uninorm_core::is_excluded(target, &entry_path, &globs) {
                                        if let Some(msg) = convert_content_if_needed(target, max_bytes) {
                                            msgs.push(msg);
                                        }
                                    }
                                }
                            }
                            msgs
                        }));
                    }

                    for handle in handles {
                        let messages = handle.await.unwrap_or_else(|e| {
                            vec![format!("Error: spawn_blocking task failed: {e}")]
                        });
                        for msg in messages {
                            append_log(&msg);
                        }
                    }
                }
                _ = sighup.recv() => {
                    append_log("Received SIGHUP -- reloading config");
                    break;
                }
                _ = sigterm.recv() => {
                    append_log("Daemon stopped (SIGTERM)");
                    return Ok(());
                }
                _ = tokio::signal::ctrl_c() => {
                    append_log("Daemon stopped (SIGINT)");
                    return Ok(());
                }
            }
        }
        drop(watcher);
    }
}

#[cfg(not(unix))]
async fn run_daemon_platform() -> std::result::Result<(), DaemonError> {
    Err(DaemonError::UnsupportedPlatform)
}
