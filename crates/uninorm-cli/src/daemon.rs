use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Result;

use crate::config::{self, WatchConfig};

/// Maximum log file size before rotation (5 MB).
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Spawn the daemon as a detached background process.
pub fn spawn_daemon() -> Result<()> {
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

/// Append a timestamped log entry, with size-based rotation.
fn append_log(message: &str) {
    let Ok(path) = config::log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

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

/// Check if a filename is a uninorm temp file (rename prefix or content suffix).
fn is_temp_file(name: &str) -> bool {
    name.starts_with(".uninorm_tmp_") || name.ends_with(".uninorm_tmp")
}

/// Clean up stale temp files left by a previous crash.
fn cleanup_stale_temps(config: &WatchConfig) {
    for entry in &config.entries {
        let walker = walkdir::WalkDir::new(&entry.path).max_depth(if entry.recursive {
            usize::MAX
        } else {
            1
        });
        for dir_entry in walker.into_iter().flatten() {
            if let Some(name) = dir_entry.file_name().to_str() {
                if is_temp_file(name) {
                    let _ = std::fs::remove_file(dir_entry.path());
                    append_log(&format!(
                        "Cleaned stale temp: {}",
                        dir_entry.path().display()
                    ));
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

/// Rename a single file if it needs NFD→NFC conversion.
/// Returns `(log_message, nfc_path_if_renamed)`.
fn rename_if_needed(path: &Path, ce: &CompiledEntry<'_>) -> (Option<String>, Option<PathBuf>) {
    let Some(file_name_os) = path.file_name() else {
        return (None, None);
    };
    let file_name = file_name_os.to_string_lossy();

    if uninorm_core::is_excluded(path, &ce.entry.path, &ce.globs) {
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
                Some(format!("Renamed: {} → {}", file_name, nfc_name)),
                Some(new_path),
            ),
            Err(e) => {
                let _ = std::fs::rename(&tmp, path);
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
        Err(_) => return None,
    };

    if !meta.is_file() || meta.len() > max_bytes {
        return None;
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return None,
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

/// Main daemon loop. Called by the hidden `daemon` subcommand.
pub async fn run_daemon() -> Result<()> {
    config::write_pid(std::process::id())?;
    append_log("Daemon started");

    run_daemon_platform().await
}

#[cfg(unix)]
async fn run_daemon_platform() -> Result<()> {
    use notify::Watcher;
    use tokio::sync::mpsc;

    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    // Outer loop: reload config on SIGHUP
    loop {
        let watch_config = WatchConfig::load()?;
        let has_enabled = watch_config.entries.iter().any(|e| e.enabled);
        if !has_enabled {
            append_log("No enabled watch entries — daemon exiting");
            config::remove_pid();
            return Ok(());
        }

        let debounce_ms = watch_config.debounce_ms.unwrap_or(300);

        cleanup_stale_temps(&watch_config);

        // Pre-compile glob sets for enabled entries only
        let compiled: Vec<CompiledEntry<'_>> = watch_config
            .entries
            .iter()
            .filter(|e| e.enabled)
            .map(|e| CompiledEntry {
                entry: e,
                globs: uninorm_core::compile_excludes(&e.exclude),
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
            append_log("All watch paths failed — daemon exiting");
            config::remove_pid();
            return Ok(());
        }

        // Inner loop: process events with debounce
        let mut pending_paths: HashMap<PathBuf, bool> = HashMap::new(); // path -> is_name_event
        let debounce_dur = std::time::Duration::from_millis(debounce_ms);
        let mut debounce_timer = tokio::time::interval(debounce_dur);
        debounce_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the first immediate tick
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
                                // Merge: if we already have a name event, keep it
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
                    for (path, is_name_event) in &batch {
                        let Some(ce) = find_entry_for_path(path, &compiled) else {
                            continue;
                        };

                        // Skip symlinks unless follow_symlinks is enabled
                        if !ce.entry.follow_symlinks {
                            if let Ok(m) = std::fs::symlink_metadata(path) {
                                if m.file_type().is_symlink() {
                                    continue;
                                }
                            }
                        }

                        // Rename on create/name events
                        let nfc_path = if *is_name_event {
                            let (msg, renamed_path) = rename_if_needed(path, ce);
                            if let Some(msg) = msg {
                                append_log(&msg);
                            }
                            renamed_path
                        } else {
                            None
                        };

                        // Convert content if enabled and not excluded
                        if ce.entry.content {
                            let target = nfc_path.as_deref().unwrap_or(path);
                            if !uninorm_core::is_excluded(target, &ce.entry.path, &ce.globs) {
                                let max_bytes = ce.entry.max_content_bytes
                                    .unwrap_or(uninorm_core::DEFAULT_MAX_CONTENT_BYTES);
                                if let Some(msg) = convert_content_if_needed(target, max_bytes) {
                                    append_log(&msg);
                                }
                            }
                        }
                    }
                }
                _ = sighup.recv() => {
                    append_log("Received SIGHUP — reloading config");
                    break;
                }
                _ = sigterm.recv() => {
                    append_log("Daemon stopped (SIGTERM)");
                    config::remove_pid();
                    return Ok(());
                }
                _ = tokio::signal::ctrl_c() => {
                    append_log("Daemon stopped (SIGINT)");
                    config::remove_pid();
                    return Ok(());
                }
            }
        }
        drop(watcher);
    }
}

#[cfg(not(unix))]
async fn run_daemon_platform() -> Result<()> {
    anyhow::bail!("Background daemon is only supported on Unix systems");
}
