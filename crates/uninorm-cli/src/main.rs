use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;

static WATCH_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use uninorm_core::{ConversionOptions, ConversionStats};

#[derive(Parser)]
#[command(
    name = "uninorm",
    about = "Convert Unicode NFD → NFC for filenames and text content",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert file/folder names (and optionally content) under a path
    Files {
        /// Path to convert (file or directory)
        path: PathBuf,

        /// Preview changes without renaming or writing anything
        #[arg(long)]
        dry_run: bool,

        /// Recurse into subdirectories (default: true)
        #[arg(short = 'r', long, default_value = "true")]
        recursive: bool,

        /// Also convert text content inside files
        #[arg(long)]
        content: bool,

        /// Follow symbolic links
        #[arg(long)]
        follow_symlinks: bool,

        /// Exclude entries whose name matches this pattern (repeatable: --exclude .git --exclude node_modules)
        #[arg(long, value_name = "PATTERN")]
        exclude: Vec<String>,
    },

    /// Watch paths and automatically convert NFD filenames as files appear or are renamed
    Watch {
        /// One or more paths to watch
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Exclude entries whose name matches this pattern (repeatable)
        #[arg(long, value_name = "PATTERN")]
        exclude: Vec<String>,
    },

    /// Show recent conversion log
    Log {
        /// Number of recent lines to show
        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,
    },

    /// Show watcher status (running paths, PID, recent activity)
    Status,

    /// Convert clipboard text from NFD to NFC and write it back
    Clipboard,

    /// Check whether TEXT is already NFC-normalized
    Check {
        /// Text to check
        text: String,
    },
}

// ── Log helpers ───────────────────────────────────────────────────────────────

fn log_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("uninorm").join("uninorm.log"))
}

// ── Watch state helpers ────────────────────────────────────────────────────────

fn watch_state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("uninorm").join("watch.state"))
}

fn write_watch_state(paths: &[PathBuf]) {
    let Some(state_path) = watch_state_path() else {
        return;
    };
    if let Some(parent) = state_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let pid = std::process::id();
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let mut content = format!("{pid}\n{ts}\n");
    for p in paths {
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
        content.push_str(&format!("{}\n", canonical.display()));
    }
    let _ = std::fs::write(&state_path, content);
}

fn clear_watch_state() {
    if let Some(state_path) = watch_state_path() {
        let _ = std::fs::remove_file(state_path);
    }
}

fn append_log(message: &str) {
    let Some(path) = log_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
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

// ── Watch helper: rename a single path if it needs NFD→NFC conversion ────────

fn rename_if_needed(path: &Path, exclude: &[String], watch_roots: &[PathBuf]) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();

    // Check exclude patterns against the path relative to the watch root only,
    // so that patterns like "node_modules" don't accidentally match a parent
    // directory that happens to share the name (e.g. /home/node_modules/project/).
    if !exclude.is_empty() {
        let relative = watch_roots
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())
            .unwrap_or(path);
        if relative.components().any(|c| {
            if let std::path::Component::Normal(n) = c {
                let s = n.to_string_lossy();
                exclude.iter().any(|pat| s.as_ref() == pat.as_str())
            } else {
                false
            }
        }) {
            return None;
        }
    }

    if !uninorm_core::needs_filename_conversion(&file_name) {
        return None;
    }

    let nfc_name = uninorm_core::to_nfc_filename(&file_name);
    let new_path = path.with_file_name(&nfc_name);
    let parent = path.parent()?;

    // Check for a genuine conflict: NFC target exists AND is a different file.
    // On normalization-insensitive filesystems (APFS) the NFC path resolves to
    // the same inode as the NFD path, so new_path.exists() can be true even
    // when no separate file is there.
    let is_conflict = new_path.exists() && !uninorm_core::same_inode(path, &new_path);
    if is_conflict {
        return Some(format!(
            "Conflict: skipping {file_name} (NFC target already exists)"
        ));
    }

    // Use a monotonic counter for the temp name — never embed the original
    // (potentially NFD) filename, which would cause the watcher to pick up
    // the temp file and attempt another rename, triggering an infinite loop.
    let count = WATCH_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".uninorm_tmp_{count}"));

    match std::fs::rename(path, &tmp) {
        Ok(_) => match std::fs::rename(&tmp, &new_path) {
            Ok(_) => Some(format!("Renamed: {} → {}", file_name, nfc_name)),
            Err(e) => {
                let _ = std::fs::rename(&tmp, path);
                Some(format!("Error: rename failed for {file_name}: {e}"))
            }
        },
        Err(e) => Some(format!("Error: rename failed for {file_name}: {e}")),
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Files {
            path,
            dry_run,
            recursive,
            content,
            follow_symlinks,
            exclude,
        } => {
            if !path.exists() {
                anyhow::bail!("Path does not exist: {}", path.display());
            }

            if dry_run {
                println!("[dry-run] No files will be modified.");
            }
            if !exclude.is_empty() {
                println!("Excluding: {}", exclude.join(", "));
            }

            let opts = ConversionOptions {
                convert_filenames: true,
                convert_content: content,
                dry_run,
                recursive,
                follow_symlinks,
                exclude_patterns: exclude,
            };

            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::with_template("{spinner:.cyan} {msg}")
                    .expect("hardcoded progress template must parse")
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(80));

            let stats = uninorm_core::convert_path(&path, &opts, |s: &ConversionStats| {
                pb.set_message(format!(
                    "Scanned: {}  Renamed: {}  Content: {}",
                    s.files_scanned, s.files_renamed, s.files_content_converted
                ));
            })
            .await?;

            pb.finish_and_clear();
            print_stats(&stats, dry_run);
            if !stats.errors.is_empty() {
                std::process::exit(1);
            }
        }

        Commands::Watch { paths, exclude } => {
            use notify::Watcher;
            use tokio::sync::mpsc;

            let (tx, mut rx) = mpsc::unbounded_channel::<notify::Result<notify::Event>>();

            let mut watcher = notify::RecommendedWatcher::new(
                move |res| {
                    let _ = tx.send(res);
                },
                notify::Config::default(),
            )?;

            for path in &paths {
                if !path.exists() {
                    anyhow::bail!("Path does not exist: {}", path.display());
                }
                watcher.watch(path.as_path(), notify::RecursiveMode::Recursive)?;
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                let msg = format!("Watching: {}", canonical.display());
                println!("{msg}");
                append_log(&msg);
            }

            write_watch_state(&paths);

            if !exclude.is_empty() {
                println!("Excluding: {}", exclude.join(", "));
            }
            println!("Press Ctrl+C to stop.\n");

            loop {
                tokio::select! {
                    Some(result) = rx.recv() => {
                        match result {
                            Ok(event) => {
                                use notify::EventKind;
                                match event.kind {
                                    EventKind::Create(_)
                                    | EventKind::Modify(notify::event::ModifyKind::Name(_))
                                    | EventKind::Any => {
                                        for path in &event.paths {
                                            // Skip our own temp files to prevent
                                            // the two-step rename from re-triggering
                                            // the watcher and causing an infinite loop.
                                            if path.file_name()
                                                .is_some_and(|n| n.to_string_lossy().starts_with(".uninorm_tmp_"))
                                            {
                                                continue;
                                            }
                                            if path.exists() {
                                                if let Some(msg) = rename_if_needed(path, &exclude, &paths) {
                                                    println!("{msg}");
                                                    append_log(&msg);
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Err(e) => eprintln!("Watch error: {e}"),
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        let msg = "Watch stopped.";
                        println!("\n{msg}");
                        append_log(msg);
                        clear_watch_state();
                        break;
                    }
                }
            }
        }

        Commands::Log { lines } => {
            let path = log_path()
                .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

            if !path.exists() {
                println!("No log file yet. Run `uninorm watch` to start logging.");
                println!("Log location: {}", path.display());
                return Ok(());
            }

            let content = std::fs::read_to_string(&path)?;
            let all: Vec<&str> = content.lines().collect();
            let start = all.len().saturating_sub(lines);
            for line in &all[start..] {
                println!("{line}");
            }
            if all.is_empty() {
                println!("Log is empty.");
            } else {
                println!(
                    "\n({} total entries, showing last {})",
                    all.len(),
                    lines.min(all.len())
                );
            }
        }

        Commands::Status => {
            let state_path = watch_state_path()
                .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

            if !state_path.exists() {
                println!("No watcher is running.");
            } else {
                let raw = std::fs::read_to_string(&state_path)?;
                let mut lines = raw.lines();

                let pid: u32 = lines.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let started = lines.next().unwrap_or("unknown");
                let watch_paths: Vec<&str> = lines.collect();

                // Check whether the PID is still alive.
                let alive = pid > 0 && {
                    #[cfg(unix)]
                    {
                        // kill(pid, 0) returns Ok if process exists.
                        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
                    }
                    #[cfg(not(unix))]
                    {
                        false
                    }
                };

                if alive {
                    println!("Watcher running  (PID {pid}, started {started})");
                } else {
                    println!("Watcher not running  (last PID {pid}, started {started})");
                    // Clean up stale state file.
                    let _ = std::fs::remove_file(&state_path);
                }

                if !watch_paths.is_empty() {
                    println!("\nWatched paths:");
                    for p in &watch_paths {
                        println!("  {p}");
                    }
                }

                // Show last 5 log lines.
                if let Some(log) = log_path() {
                    if log.exists() {
                        if let Ok(content) = std::fs::read_to_string(&log) {
                            let all: Vec<&str> = content.lines().collect();
                            let recent = &all[all.len().saturating_sub(5)..];
                            if !recent.is_empty() {
                                println!("\nRecent activity:");
                                for l in recent {
                                    println!("  {l}");
                                }
                            }
                        }
                    }
                }
            }
        }

        Commands::Clipboard => {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|e| anyhow::anyhow!("Failed to open clipboard: {e}"))?;

            let text = clipboard
                .get_text()
                .map_err(|e| anyhow::anyhow!("Failed to read clipboard: {e}"))?;

            let nfc = uninorm_core::convert_text(&text);

            if nfc == text {
                println!("Clipboard is already NFC — no changes made.");
            } else {
                clipboard
                    .set_text(nfc)
                    .map_err(|e| anyhow::anyhow!("Failed to write clipboard: {e}"))?;
                println!("Clipboard converted to NFC.");
            }
        }

        Commands::Check { text } => {
            if uninorm_core::is_nfc(&text) {
                println!("✓ Already NFC");
            } else {
                let nfc = uninorm_core::convert_text(&text);
                println!("✗ NOT NFC — converted form: {nfc}");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

fn print_stats(stats: &ConversionStats, dry_run: bool) {
    let prefix = if dry_run { "[dry-run] " } else { "" };
    println!("{prefix}Scanned:  {}", stats.files_scanned);
    println!("{prefix}Renamed:  {}", stats.files_renamed);
    println!("{prefix}Content:  {}", stats.files_content_converted);

    if !stats.errors.is_empty() {
        eprintln!("\nErrors ({}):", stats.errors.len());
        for e in &stats.errors {
            eprintln!("  - {e}");
        }
    }
}
