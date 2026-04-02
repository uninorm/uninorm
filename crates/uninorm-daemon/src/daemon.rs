use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

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
                .arg("daemon-run")
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
            .arg("daemon-run")
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

/// Owned entry for initial scan (needed for spawn_blocking which requires 'static).
struct InitialScanEntry {
    path: PathBuf,
    recursive: bool,
    content: bool,
    follow_symlinks: bool,
    globs: Arc<globset::GlobSet>,
    max_content_bytes: u64,
}

/// Compile exclude patterns for a single entry, merging global ignore.
/// Exposed for testing.
pub fn compile_entry_excludes(
    global_ignore: &[String],
    entry_exclude: &[String],
    use_global_ignore: bool,
) -> (globset::GlobSet, Vec<String>) {
    let mut patterns: Vec<String> = if use_global_ignore {
        global_ignore.to_vec()
    } else {
        Vec::new()
    };
    patterns.extend(entry_exclude.iter().cloned());
    uninorm_core::compile_excludes(&patterns)
}

/// Perform an initial scan of all watch entries, converting existing NFD files.
/// Called once on daemon start (and on config reload) so that pre-existing files
/// are normalised without waiting for a filesystem event.
/// Messages are streamed via `log` callback to avoid unbounded memory growth.
fn initial_scan(entries: Vec<InitialScanEntry>, log: impl Fn(&str)) {
    for entry in &entries {
        let max_depth = if entry.recursive {
            uninorm_core::MAX_WALK_DEPTH
        } else {
            1
        };
        let mut rename_count = 0usize;
        let mut content_count = 0usize;

        let walker = walkdir::WalkDir::new(&entry.path)
            .max_depth(max_depth)
            .follow_links(entry.follow_symlinks)
            .contents_first(true)
            .into_iter();

        for result in walker {
            let dir_entry = match result {
                Ok(de) => de,
                Err(e) => {
                    log(&format!("Walk error in {}: {e}", entry.path.display()));
                    continue;
                }
            };

            // Skip the root directory itself
            if dir_entry.depth() == 0 {
                continue;
            }

            let path = dir_entry.path().to_path_buf();

            if let Some(name) = dir_entry.file_name().to_str() {
                if is_temp_file(name) {
                    continue;
                }
            }

            if !entry.follow_symlinks {
                if let Ok(m) = std::fs::symlink_metadata(&path) {
                    if m.file_type().is_symlink() {
                        continue;
                    }
                }
            }

            // Rename filename if needed
            let ce_inline = InlineCompiledEntry {
                entry_path: &entry.path,
                globs: &entry.globs,
            };
            let (msg, renamed_path) = rename_if_needed_inline(&path, &ce_inline);
            if renamed_path.is_some() {
                rename_count += 1;
            }
            if let Some(msg) = msg {
                log(&msg);
            }

            // Convert content if enabled
            if entry.content {
                let target = renamed_path.as_deref().unwrap_or(&path);
                if !uninorm_core::is_excluded(target, &entry.path, &entry.globs) {
                    if let Some(result) = convert_content_if_needed(target, entry.max_content_bytes)
                    {
                        if result.is_converted() {
                            content_count += 1;
                        }
                        log(result.message());
                    }
                }
            }
        }

        log(&format!(
            "Initial scan complete for {}: {} renamed, {} content converted",
            entry.path.display(),
            rename_count,
            content_count,
        ));
    }
}

/// Clean up stale temp files left by a previous crash.
fn cleanup_stale_temps(wc: &WatchConfig) {
    for entry in wc.entries.iter().filter(|e| e.enabled) {
        let walker = walkdir::WalkDir::new(&entry.path)
            .max_depth(if entry.recursive {
                uninorm_core::MAX_WALK_DEPTH
            } else {
                1
            })
            .follow_links(entry.follow_symlinks);
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
/// Uses `Arc` so cloning for `spawn_blocking` closures is cheap.
struct CompiledEntry<'a> {
    entry: &'a config::WatchEntry,
    globs: Arc<globset::GlobSet>,
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
    let Some(file_name) = file_name_os.to_str() else {
        return (
            Some(format!("Skipped non-UTF-8 filename: {}", path.display())),
            None,
        );
    };

    if uninorm_core::is_excluded(path, ce.entry_path, ce.globs) {
        return (None, None);
    }

    if !uninorm_core::needs_filename_conversion(file_name) {
        return (None, None);
    }

    let nfc_name = uninorm_core::to_nfc_filename(file_name);
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

/// Result of a content conversion attempt.
enum ContentResult {
    /// Content was successfully converted to NFC.
    Converted(String),
    /// An error occurred during conversion.
    Error(String),
}

impl ContentResult {
    fn message(&self) -> &str {
        match self {
            ContentResult::Converted(msg) | ContentResult::Error(msg) => msg,
        }
    }

    fn is_converted(&self) -> bool {
        matches!(self, ContentResult::Converted(_))
    }
}

/// Convert file content from NFD to NFC if needed.
fn convert_content_if_needed(path: &Path, max_bytes: u64) -> Option<ContentResult> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            return Some(ContentResult::Error(format!(
                "Error: cannot read metadata for {}: {e}",
                path.display()
            )));
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
            return Some(ContentResult::Error(format!(
                "Error: cannot read {}: {e}",
                path.display()
            )));
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
            if let Err(e) = std::fs::set_permissions(&tmp_path, meta.permissions()) {
                append_log(&format!(
                    "Warning: could not preserve permissions for {}: {e}",
                    path.display()
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let _ = std::os::unix::fs::chown(&tmp_path, Some(meta.uid()), Some(meta.gid()));
            }
            match std::fs::rename(&tmp_path, path) {
                Ok(_) => Some(ContentResult::Converted(format!(
                    "Content converted: {}",
                    path.display()
                ))),
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp_path);
                    Some(ContentResult::Error(format!(
                        "Error: content write failed for {}: {e}",
                        path.display()
                    )))
                }
            }
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            Some(ContentResult::Error(format!(
                "Error: content write failed for {}: {e}",
                path.display()
            )))
        }
    }
}

/// Find the most specific CompiledEntry a path belongs to (longest matching prefix).
fn find_entry_for_path<'a>(
    path: &Path,
    entries: &'a [CompiledEntry<'a>],
) -> Option<&'a CompiledEntry<'a>> {
    entries
        .iter()
        .filter(|ce| path.starts_with(&ce.entry.path))
        .max_by_key(|ce| ce.entry.path.as_os_str().len())
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

        let raw_debounce = watch_config.debounce_ms.unwrap_or(300);
        let debounce_ms = raw_debounce.max(10);
        if raw_debounce < 10 {
            append_log(&format!(
                "Warning: debounce_ms {raw_debounce} clamped to minimum 10ms"
            ));
        }

        {
            let cfg_ref = watch_config.clone();
            tokio::task::spawn_blocking(move || cleanup_stale_temps(&cfg_ref)).await?;
        }

        // Load global ignore patterns and merge with per-entry excludes
        let (global_ignore, global_warn) = config::load_global_ignore();
        if let Some(warn) = global_warn {
            append_log(&warn);
        }
        if !global_ignore.is_empty() {
            append_log(&format!(
                "Global ignore: {} patterns loaded",
                global_ignore.len()
            ));
        }

        // Pre-compile glob sets for enabled entries only
        let compiled: Vec<CompiledEntry<'_>> = watch_config
            .entries
            .iter()
            .filter(|e| e.enabled)
            .map(|e| {
                let (set, invalid) =
                    compile_entry_excludes(&global_ignore, &e.exclude, e.use_global_ignore);
                for pat in &invalid {
                    append_log(&format!("Warning: invalid exclude pattern ignored: {pat}"));
                }
                CompiledEntry {
                    entry: e,
                    globs: Arc::new(set),
                }
            })
            .collect();

        for ce in &compiled {
            append_log(&format!("Watching: {}", ce.entry.path.display()));
        }

        // Set up watcher BEFORE initial scan so events during the scan are
        // captured in the channel. The scan's conversions are idempotent, so
        // duplicate events from the scan itself are harmless.
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

        // Initial scan: convert pre-existing NFD files. The watcher is already
        // active, so no events are lost. Scan respects signals for cancellation.
        {
            let scan_entries: Vec<InitialScanEntry> = compiled
                .iter()
                .map(|ce| InitialScanEntry {
                    path: ce.entry.path.clone(),
                    recursive: ce.entry.recursive,
                    content: ce.entry.content,
                    follow_symlinks: ce.entry.follow_symlinks,
                    globs: ce.globs.clone(),
                    max_content_bytes: ce
                        .entry
                        .max_content_bytes
                        .unwrap_or(uninorm_core::DEFAULT_MAX_CONTENT_BYTES),
                })
                .collect();
            let scan_handle =
                tokio::task::spawn_blocking(move || initial_scan(scan_entries, append_log));
            tokio::select! {
                result = scan_handle => {
                    if let Err(e) = result {
                        append_log(&format!("Initial scan task failed: {e}"));
                    }
                }
                _ = sigterm.recv() => {
                    append_log("Daemon stopped (SIGTERM during initial scan)");
                    return Ok(());
                }
                _ = tokio::signal::ctrl_c() => {
                    append_log("Daemon stopped (SIGINT during initial scan)");
                    return Ok(());
                }
                _ = sighup.recv() => {
                    append_log("Received SIGHUP during initial scan -- reloading config");
                    continue;
                }
            }
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
                                    .and_then(|n| n.to_str())
                                    .is_some_and(is_temp_file)
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
                        type EventTask = (PathBuf, bool, PathBuf, Arc<globset::GlobSet>, bool, bool, u64);
                        let mut tasks: Vec<EventTask> = Vec::new();
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

                        // One spawn_blocking per directory — sequential within, parallel across.
                        // Messages are collected into a Vec here (unlike initial_scan's streaming
                        // callback) because batches are bounded by MAX_PENDING (10,000 entries),
                        // keeping memory usage predictable.
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
                                        if let Some(result) = convert_content_if_needed(target, max_bytes) {
                                            msgs.push(result.message().to_owned());
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
