use std::path::PathBuf;

use anyhow::Result;
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

    /// Convert clipboard text from NFD to NFC and write it back
    Clipboard,

    /// Check whether TEXT is already NFC-normalized
    Check {
        /// Text to check
        text: String,
    },
}

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
                    .unwrap()
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
