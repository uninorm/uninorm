use std::path::Path;
use walkdir::WalkDir;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use crate::normalize::{to_nfc, to_nfc_filename, needs_filename_conversion};

/// Check if two paths refer to the same filesystem inode.
/// Used to detect the APFS case where NFD and NFC forms of the same name
/// both resolve to the same file (normalization-insensitive lookup).
#[cfg(unix)]
fn same_inode(p1: &Path, p2: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(p1), std::fs::metadata(p2)) {
        (Ok(m1), Ok(m2)) => m1.ino() == m2.ino() && m1.dev() == m2.dev(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_inode(_p1: &Path, _p2: &Path) -> bool { false }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionOptions {
    pub convert_filenames: bool,
    pub convert_content: bool,
    pub dry_run: bool,
    pub recursive: bool,
    pub follow_symlinks: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            convert_filenames: true,
            convert_content: false,
            dry_run: false,
            recursive: true,
            follow_symlinks: false,
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

        stats.files_scanned += 1;

        let file_name = entry.file_name().to_string_lossy();

        // Rename file/folder if needed
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
                        Ok(_) => stats.files_renamed += 1,
                        Err(e) => {
                            let _ = tokio::fs::rename(&tmp, entry.path()).await; // restore
                            stats.errors.push(format!(
                                "Rename failed {}: {e}",
                                entry.path().display()
                            ));
                        }
                    },
                    Err(e) => stats.errors.push(format!(
                        "Rename failed {}: {e}",
                        entry.path().display()
                    )),
                }
            } else {
                stats.files_renamed += 1; // count in dry-run too
            }
        }

        // Convert file content (text files only)
        if opts.convert_content && entry.file_type().is_file() {
            // Determine the current path (may have been renamed above)
            let file_name_nfc = to_nfc_filename(&file_name);
            let current_path = if opts.convert_filenames && needs_filename_conversion(&file_name) && !opts.dry_run {
                entry.path().with_file_name(&file_name_nfc)
            } else {
                entry.path().to_path_buf()
            };

            match tokio::fs::read_to_string(&current_path).await {
                Ok(content) => {
                    let nfc_content = to_nfc(&content);
                    if nfc_content != content {
                        if !opts.dry_run {
                            if let Err(e) = tokio::fs::write(&current_path, nfc_content.as_bytes()).await {
                                stats.errors.push(format!(
                                    "Content write failed {}: {e}",
                                    current_path.display()
                                ));
                            } else {
                                stats.files_content_converted += 1;
                            }
                        } else {
                            stats.files_content_converted += 1;
                        }
                    }
                }
                Err(_) => {
                    // Binary file or unreadable — skip silently
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

    #[tokio::test]
    async fn test_convert_filename_dry_run() {
        let dir = TempDir::new().unwrap();
        // Create a file with NFC name (we can't easily create real HFS+ NFD on Linux)
        let nfc_path = dir.path().join("강남구.txt");
        fs::write(&nfc_path, "test content").unwrap();

        let opts = ConversionOptions {
            convert_filenames: true,
            convert_content: false,
            dry_run: true,
            recursive: false,
            follow_symlinks: false,
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_scanned, 2); // dir + file
        assert_eq!(stats.errors, Vec::<String>::new());
    }

    #[tokio::test]
    async fn test_convert_content() {
        let dir = TempDir::new().unwrap();
        // Content with NFD text
        let nfd_content = "e\u{0301}"; // é in NFD
        let path = dir.path().join("test.txt");
        fs::write(&path, nfd_content).unwrap();

        let opts = ConversionOptions {
            convert_filenames: false,
            convert_content: true,
            dry_run: false,
            recursive: false,
            follow_symlinks: false,
        };

        let stats = convert_path(dir.path(), &opts, |_| {}).await.unwrap();
        assert_eq!(stats.files_content_converted, 1);

        let result = fs::read_to_string(&path).unwrap();
        assert_eq!(result, "\u{00E9}"); // é in NFC
    }
}
