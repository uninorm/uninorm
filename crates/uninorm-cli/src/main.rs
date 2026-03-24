use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use uninorm_core::{ConversionOptions, ConversionStats, DEFAULT_MAX_CONTENT_BYTES};
use uninorm_daemon::{config, DaemonController, DaemonError};

#[derive(Parser)]
#[command(
    name = "uninorm",
    about = "Convert Unicode NFD -> NFC for filenames and text content",
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

        /// Do not recurse into subdirectories
        #[arg(long)]
        no_recursive: bool,

        /// Also convert text content inside files
        #[arg(long)]
        content: bool,

        /// Follow symbolic links
        #[arg(long)]
        follow_symlinks: bool,

        /// Exclude entries matching NAME or glob pattern (repeatable: --exclude .git --exclude "*.log")
        #[arg(long, value_name = "PATTERN")]
        exclude: Vec<String>,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show individual file changes
        #[arg(short = 'v', long)]
        verbose: bool,

        /// Maximum file size for content conversion (e.g. 50MB, 1GB). Default: 100MB
        #[arg(long, value_name = "SIZE", value_parser = parse_size)]
        max_size: Option<u64>,

        /// Do not apply global ignore patterns (~/.config/uninorm/ignore)
        #[arg(long)]
        no_global_ignore: bool,
    },

    /// Manage background file watching (add/remove/start/stop watch entries)
    Watch {
        #[command(subcommand)]
        action: WatchAction,
    },

    /// Show recent conversion log
    Log {
        /// Number of recent lines to show
        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,
    },

    /// Show watcher status (daemon PID, watched paths, recent activity)
    Status,

    /// Manage autostart (on/off) — register daemon to start on login
    Autostart {
        #[command(subcommand)]
        action: AutostartAction,
    },

    /// Manage the background daemon (start/stop/restart)
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },

    /// Convert clipboard text from NFD to NFC and write it back
    Clipboard,

    /// Check whether TEXT is already NFC-normalized
    Check {
        /// Text to check
        text: String,
    },

    /// Convert TEXT from NFD to NFC and print the result (reads stdin if no text given)
    Convert {
        /// Text to convert (omit to read from stdin)
        text: Option<String>,

        /// Copy result to clipboard
        #[arg(short = 'c', long)]
        clipboard: bool,
    },

    /// Internal: run as background daemon process
    #[command(hide = true, name = "daemon-run")]
    DaemonRun,
}

#[derive(Subcommand)]
enum WatchAction {
    /// Add or update a watch entry
    Add {
        /// Path to watch
        path: PathBuf,

        /// Do not recurse into subdirectories
        #[arg(long)]
        no_recursive: bool,

        /// Also convert text content inside files on change
        #[arg(long)]
        content: bool,

        /// Follow symbolic links
        #[arg(long)]
        follow_symlinks: bool,

        /// Exclude entries matching NAME or glob pattern (repeatable)
        #[arg(long, value_name = "PATTERN")]
        exclude: Vec<String>,

        /// Maximum file size for content conversion (e.g. 50MB, 1GB). Default: 100MB
        #[arg(long, value_name = "SIZE", value_parser = parse_size)]
        max_size: Option<u64>,

        /// Event debounce interval in milliseconds (default: 300)
        #[arg(long, value_name = "MS")]
        debounce: Option<u64>,
    },

    /// Remove watch entries by number (comma-separated, e.g. 1,3,5)
    Remove {
        /// Entry numbers to remove (comma-separated)
        indices: String,
    },

    /// Show all watch entries
    List,

    /// Enable watch entries by number (comma-separated, e.g. 1,3,5)
    Enable {
        /// Entry numbers to enable (comma-separated)
        indices: String,
    },

    /// Disable watch entries (comma-separated, e.g. 1,3,5)
    Disable {
        /// Entry numbers to disable (comma-separated)
        indices: String,
    },

    /// Remove all watch entries and stop daemon
    Reset {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum AutostartAction {
    /// Enable autostart (daemon starts on login)
    On,
    /// Disable autostart
    Off,
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Restart the daemon (stop + start)
    Restart,
}

// -- Helpers --

/// Parse comma-separated 1-based indices (e.g. "1,3,5") and validate against entry count.
/// Returns sorted, deduplicated 0-based indices.
fn parse_indices(s: &str, count: usize) -> Result<Vec<usize>> {
    let mut indices = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let n: usize = part
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid number: {part}"))?;
        if n == 0 || n > count {
            anyhow::bail!(
                "Entry #{n} does not exist. Use `uninorm watch list` to see entries (1-{count})."
            );
        }
        indices.push(n - 1);
    }
    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() {
        anyhow::bail!("No entry numbers provided.");
    }
    Ok(indices)
}

/// Parse human-readable size strings like "100MB", "1GB", "500KB", or raw bytes.
fn parse_size(s: &str) -> std::result::Result<u64, String> {
    let s = s.trim().to_uppercase();
    let (num_str, multiplier) = if let Some(n) = s.strip_suffix("GB") {
        (n, 1024 * 1024 * 1024u64)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix('B') {
        (n, 1)
    } else {
        (s.as_str(), 1)
    };
    let num: f64 = num_str
        .trim()
        .parse()
        .map_err(|_| format!("Invalid size: {s}"))?;
    if !num.is_finite() || num <= 0.0 {
        return Err(format!("Invalid size: {s}"));
    }
    let result = num * multiplier as f64;
    if result > u64::MAX as f64 {
        return Err(format!("Size too large: {s}"));
    }
    Ok(result as u64)
}

fn make_spinner() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .expect("hardcoded progress template must parse")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
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

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{}MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{bytes}B")
    }
}

/// Read the last N lines from a file without loading the entire file.
fn read_tail_lines(path: &std::path::Path, n: usize) -> std::io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();

    // Start with 64KB, double up to 1MB if we don't get enough lines
    let mut chunk_size = 64 * 1024u64;
    let max_chunk = 1024 * 1024u64;

    loop {
        let start_pos = file_len.saturating_sub(chunk_size);
        file.seek(SeekFrom::Start(start_pos))?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;
        let buf = String::from_utf8_lossy(&raw);

        let all_lines: Vec<&str> = buf.lines().collect();
        // If we seeked into the middle, the first line may be partial
        let tail = if start_pos > 0 && all_lines.len() > 1 {
            &all_lines[1..]
        } else {
            &all_lines[..]
        };

        // If we have enough lines or already reading from the start, return
        if tail.len() >= n || start_pos == 0 || chunk_size >= max_chunk {
            let start = tail.len().saturating_sub(n);
            return Ok(tail[start..].iter().map(|s| s.to_string()).collect());
        }

        chunk_size = (chunk_size * 2).min(max_chunk);
    }
}

// -- Entry point --

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Auto-register autostart on first run of any command
    if !uninorm_daemon::autostart::is_installed() {
        if let Err(e) = uninorm_daemon::autostart::install() {
            match e {
                DaemonError::UnsupportedPlatform => {}
                e => eprintln!("Warning: could not install autostart: {e}"),
            }
        }
    }

    match cli.command {
        // -- files: batch convert --
        Commands::Files {
            path,
            dry_run,
            no_recursive,
            content,
            follow_symlinks,
            exclude,
            yes,
            verbose,
            max_size,
            no_global_ignore,
        } => {
            if !path.exists() {
                anyhow::bail!("Path does not exist: {}", path.display());
            }

            let mut exclude_patterns = if no_global_ignore {
                Vec::new()
            } else {
                let (patterns, warn) = config::load_global_ignore();
                if let Some(warn) = warn {
                    eprintln!("{warn}");
                }
                patterns
            };
            exclude_patterns.extend(exclude);

            let opts = ConversionOptions {
                convert_filenames: true,
                convert_content: content,
                dry_run,
                recursive: !no_recursive,
                follow_symlinks,
                exclude_patterns,
                max_content_bytes: max_size.unwrap_or(DEFAULT_MAX_CONTENT_BYTES),
            };

            // Pre-scan
            let scan_pb = make_spinner();
            scan_pb.set_message("Scanning...");
            let scan = uninorm_core::scan_path(&path, &opts).await;
            scan_pb.finish_and_clear();

            println!(
                "Scanned {} entries under {}",
                scan.total_scanned,
                path.display()
            );

            if !scan.errors.is_empty() {
                eprintln!("Scan errors ({}):", scan.errors.len());
                for e in &scan.errors {
                    eprintln!("  - {e}");
                }
            }

            if scan.affected_count() == 0 {
                println!("No NFD entries found -- nothing to do.");
                return Ok(());
            }

            let rename_count = scan.rename_count();
            let content_count = scan.content_count();

            if rename_count > 0 {
                println!("  Filenames to rename:  {rename_count}");
            }
            if content_count > 0 {
                println!("  Files with NFD content: {content_count}");
            }

            if verbose {
                println!();
                for entry in &scan.entries {
                    if entry.needs_rename {
                        let old = entry.path.file_name().unwrap_or_default().to_string_lossy();
                        let new = entry.new_name.as_deref().unwrap_or("?");
                        println!("  rename: {} -> {}", old, new);
                    }
                    if entry.needs_content_conversion {
                        println!("  content: {}", entry.path.display());
                    }
                }
            }

            if !dry_run && !yes {
                println!();
                if !confirm("Proceed with conversion?") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            if dry_run {
                println!("\n[dry-run] No files will be modified.");
            }

            let pb = make_spinner();
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

        // -- watch: manage watch entries and daemon --
        Commands::Watch { action } => match action {
            WatchAction::Add {
                path,
                no_recursive,
                content,
                follow_symlinks,
                exclude,
                max_size,
                debounce,
            } => {
                if !path.exists() {
                    anyhow::bail!("Path does not exist: {}", path.display());
                }
                let canonical = path.canonicalize()?;

                let mut cfg = config::WatchConfig::load()?;

                if let Some(ms) = debounce {
                    cfg.debounce_ms = Some(ms);
                }

                let is_new = cfg.add_entry(config::WatchEntry {
                    path: canonical.clone(),
                    recursive: !no_recursive,
                    content,
                    follow_symlinks,
                    exclude,
                    max_content_bytes: max_size,
                    enabled: true,
                    use_global_ignore: true,
                });
                cfg.save()?;

                if is_new {
                    println!("Added: {}", canonical.display());
                } else {
                    println!("Updated: {}", canonical.display());
                }

                if DaemonController::status().is_some() {
                    let _ = DaemonController::reload();
                    println!("Daemon reloaded.");
                } else {
                    match DaemonController::start() {
                        Ok(pid) => println!("Daemon started (PID {pid})."),
                        Err(DaemonError::UnsupportedPlatform) => {}
                        Err(e) => eprintln!("Warning: could not start daemon: {e}"),
                    }
                }
            }

            WatchAction::Remove { indices } => {
                let mut cfg = config::WatchConfig::load()?;
                let to_remove = parse_indices(&indices, cfg.entries.len())?;

                for &idx in to_remove.iter().rev() {
                    let removed = cfg.entries.remove(idx);
                    println!("Removed #{}: {}", idx + 1, removed.path.display());
                }
                cfg.save()?;

                let _ = DaemonController::reload_or_stop();
            }

            WatchAction::List => {
                let cfg = config::WatchConfig::load()?;

                if cfg.entries.is_empty() {
                    println!("No watch entries. Add one with: uninorm watch add <path>");
                    return Ok(());
                }

                if let Some(ms) = cfg.debounce_ms {
                    println!("Debounce: {ms}ms");
                }

                println!();
                for (i, entry) in cfg.entries.iter().enumerate() {
                    let status = if entry.enabled { "enabled" } else { "disabled" };
                    let mut flags = Vec::new();
                    if !entry.recursive {
                        flags.push("non-recursive".to_string());
                    }
                    if entry.content {
                        flags.push("content".to_string());
                    }
                    if entry.follow_symlinks {
                        flags.push("follow-symlinks".to_string());
                    }
                    if !entry.exclude.is_empty() {
                        flags.push(format!("excludes: {}", entry.exclude.join(", ")));
                    }
                    if let Some(max) = entry.max_content_bytes {
                        flags.push(format!("max-size: {}", format_size(max)));
                    }
                    let opts = if flags.is_empty() {
                        String::new()
                    } else {
                        format!("  ({})", flags.join(", "))
                    };
                    println!("  {}. {}  [{status}]{opts}", i + 1, entry.path.display());
                }
            }

            WatchAction::Enable { indices } => {
                let mut cfg = config::WatchConfig::load()?;
                let to_enable = parse_indices(&indices, cfg.entries.len())?;

                for &idx in &to_enable {
                    cfg.entries[idx].enabled = true;
                    println!("Enabled #{}: {}", idx + 1, cfg.entries[idx].path.display());
                }
                cfg.save()?;

                let _ = DaemonController::reload();
            }

            WatchAction::Disable { indices } => {
                let mut cfg = config::WatchConfig::load()?;
                let to_disable = parse_indices(&indices, cfg.entries.len())?;

                for &idx in &to_disable {
                    cfg.entries[idx].enabled = false;
                    println!("Disabled #{}: {}", idx + 1, cfg.entries[idx].path.display());
                }
                cfg.save()?;

                let _ = DaemonController::reload();
            }

            WatchAction::Reset { yes } => {
                let cfg = config::WatchConfig::load()?;
                if cfg.entries.is_empty() {
                    println!("No watch entries to remove.");
                    return Ok(());
                }

                if !yes && !confirm(&format!("Remove all {} watch entries?", cfg.entries.len())) {
                    println!("Cancelled.");
                    return Ok(());
                }

                DaemonController::reset()?;
                println!("All watch entries removed.");
            }
        },

        // -- log: show recent entries --
        Commands::Log { lines } => {
            let path = config::log_path()?;

            if !path.exists() {
                println!("No log file yet. Run `uninorm watch add <path>` to start.");
                println!("Log location: {}", path.display());
                return Ok(());
            }

            let tail = read_tail_lines(&path, lines)?;
            if tail.is_empty() {
                println!("Log is empty.");
            } else {
                for line in &tail {
                    println!("{line}");
                }
                println!("\n(showing last {})", tail.len());
            }
        }

        // -- status: show daemon status + summary --
        Commands::Status => {
            match DaemonController::status() {
                Some(pid) => println!("Daemon running (PID {pid})"),
                None => {
                    if let Some(pid) = config::read_pid() {
                        println!("Daemon not running (stale PID {pid})");
                        config::remove_pid();
                    } else {
                        println!("Daemon not running.");
                    }
                }
            }

            if uninorm_daemon::autostart::is_installed() {
                println!("Autostart: on");
            } else {
                println!("Autostart: off (run `uninorm autostart on` to enable)");
            }

            let cfg = config::WatchConfig::load()?;
            let total = cfg.entries.len();
            let enabled = cfg.enabled_count();
            if total > 0 {
                println!("Watch entries: {enabled}/{total} enabled");
                println!("Use `uninorm watch list` for details.");
            } else {
                println!("No watch entries configured.");
            }

            // Show last 5 log lines
            if let Ok(log) = config::log_path() {
                if log.exists() {
                    if let Ok(recent) = read_tail_lines(&log, 5) {
                        if !recent.is_empty() {
                            println!("\nRecent activity:");
                            for l in &recent {
                                println!("  {l}");
                            }
                        }
                    }
                }
            }
        }

        // -- clipboard --
        Commands::Clipboard => {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|e| anyhow::anyhow!("Failed to open clipboard: {e}"))?;

            let text = clipboard
                .get_text()
                .map_err(|e| anyhow::anyhow!("Failed to read clipboard: {e}"))?;

            let nfc = uninorm_core::convert_text(&text);

            if nfc == text {
                println!("Clipboard is already NFC -- no changes made.");
            } else {
                clipboard
                    .set_text(nfc)
                    .map_err(|e| anyhow::anyhow!("Failed to write clipboard: {e}"))?;
                println!("Clipboard converted to NFC.");
            }
        }

        // -- check --
        Commands::Check { text } => {
            if uninorm_core::is_nfc(&text) {
                println!("Already NFC");
            } else {
                let nfc = uninorm_core::convert_text(&text);
                println!("NOT NFC -- converted form: {nfc}");
                std::process::exit(1);
            }
        }

        // -- convert --
        Commands::Convert { text, clipboard } => {
            let input = match text {
                Some(t) => t,
                None => {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buf)
                        .map_err(|e| anyhow::anyhow!("Failed to read stdin: {e}"))?;
                    buf
                }
            };

            let nfc = uninorm_core::convert_text(&input);

            if nfc == input {
                print!("{nfc}");
                eprintln!("(already NFC)");
            } else {
                print!("{nfc}");
            }

            if clipboard {
                let mut cb = arboard::Clipboard::new()
                    .map_err(|e| anyhow::anyhow!("Failed to open clipboard: {e}"))?;
                cb.set_text(&nfc)
                    .map_err(|e| anyhow::anyhow!("Failed to write clipboard: {e}"))?;
                eprintln!("(copied to clipboard)");
            }
        }

        // -- daemon start/stop/restart --
        Commands::Daemon { action } => match action {
            DaemonAction::Start => match DaemonController::start() {
                Ok(pid) => {
                    println!("Daemon started (PID {pid}).");
                    let cfg = config::WatchConfig::load()?;
                    let enabled_count = cfg.enabled_count();
                    println!("Watching {enabled_count} entries.");
                }
                Err(DaemonError::AlreadyRunning { pid }) => {
                    println!("Daemon already running (PID {pid}).");
                }
                Err(DaemonError::NoEnabledEntries) => {
                    println!("No enabled watch entries.");
                    println!("Add one with: uninorm watch add <path>");
                }
                Err(DaemonError::UnsupportedPlatform) => {
                    eprintln!("Daemon is not available on this platform.");
                    std::process::exit(1);
                }
                Err(e) => return Err(e.into()),
            },
            DaemonAction::Stop => match DaemonController::stop() {
                Ok(()) => println!("Daemon stopped."),
                Err(DaemonError::NotRunning) => println!("Daemon is not running."),
                Err(DaemonError::UnsupportedPlatform) => {
                    eprintln!("Daemon is not available on this platform.");
                }
                Err(e) => return Err(e.into()),
            },
            DaemonAction::Restart => {
                if DaemonController::status().is_some() {
                    let _ = DaemonController::stop();
                }
                match DaemonController::start() {
                    Ok(pid) => println!("Daemon restarted (PID {pid})."),
                    Err(DaemonError::NoEnabledEntries) => {
                        println!("No enabled watch entries.");
                        println!("Add one with: uninorm watch add <path>");
                    }
                    Err(DaemonError::UnsupportedPlatform) => {
                        eprintln!("Daemon is not available on this platform.");
                        std::process::exit(1);
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        },

        // -- autostart on/off --
        Commands::Autostart { action } => match action {
            AutostartAction::On => match uninorm_daemon::autostart::install() {
                Ok(()) => {
                    println!("Autostart enabled.");
                    if let Some(path) = uninorm_daemon::autostart::autostart_path() {
                        println!("  {}", path.display());
                    }
                }
                Err(DaemonError::UnsupportedPlatform) => {
                    eprintln!("Autostart is only available on macOS and Linux.");
                    std::process::exit(1);
                }
                Err(e) => return Err(e.into()),
            },
            AutostartAction::Off => match uninorm_daemon::autostart::uninstall() {
                Ok(()) => println!("Autostart disabled."),
                Err(DaemonError::UnsupportedPlatform) => {
                    eprintln!("Autostart is only available on macOS and Linux.");
                }
                Err(e) => return Err(e.into()),
            },
        },

        // -- daemon-run (hidden, internal) --
        Commands::DaemonRun => {
            uninorm_daemon::daemon::run_daemon().await?;
        }
    }

    Ok(())
}
