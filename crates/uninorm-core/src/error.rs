//! Error types for file conversion operations.
//!
//! [`ConvertError`] covers the failure modes that can occur during directory walking,
//! file renaming, and content conversion — including I/O errors, permission denials,
//! rename conflicts, and content size limits.

use std::path::{Path, PathBuf};

/// Errors that can occur during file conversion operations.
#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("path does not exist: {0}")]
    NotFound(PathBuf),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("walk error: {0}")]
    Walk(String),

    #[error("permission denied: {0}")]
    PermissionDenied(PathBuf),

    #[error("file too large ({size} bytes, max {max_size} bytes): {path}")]
    ContentTooLarge {
        path: PathBuf,
        size: u64,
        max_size: u64,
    },

    #[error("rename conflict: {source_path} -> {target} (target already exists)")]
    RenameConflict {
        source_path: PathBuf,
        target: PathBuf,
    },
}

impl From<walkdir::Error> for ConvertError {
    fn from(e: walkdir::Error) -> Self {
        ConvertError::Walk(e.to_string())
    }
}

impl ConvertError {
    /// Returns the path associated with this error, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            ConvertError::NotFound(p) => Some(p),
            ConvertError::Io { path, .. } => Some(path),
            ConvertError::Walk(_) => None,
            ConvertError::PermissionDenied(p) => Some(p),
            ConvertError::ContentTooLarge { path, .. } => Some(path),
            ConvertError::RenameConflict { source_path, .. } => Some(source_path),
        }
    }

    /// Returns true if this is a permission-related error.
    pub fn is_permission_error(&self) -> bool {
        matches!(self, ConvertError::PermissionDenied(_))
            || matches!(self, ConvertError::Io { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_found_display() {
        let err = ConvertError::NotFound(PathBuf::from("/tmp/missing"));
        assert_eq!(err.to_string(), "path does not exist: /tmp/missing");
        assert_eq!(err.path(), Some(Path::new("/tmp/missing")));
    }

    #[test]
    fn test_permission_denied_display() {
        let err = ConvertError::PermissionDenied(PathBuf::from("/root/secret"));
        assert_eq!(err.to_string(), "permission denied: /root/secret");
        assert!(err.is_permission_error());
    }

    #[test]
    fn test_content_too_large_display() {
        let err = ConvertError::ContentTooLarge {
            path: PathBuf::from("/tmp/huge.bin"),
            size: 200 * 1024 * 1024,
            max_size: 100 * 1024 * 1024,
        };
        let msg = err.to_string();
        assert!(msg.contains("file too large"));
        assert!(msg.contains("huge.bin"));
    }

    #[test]
    fn test_rename_conflict_display() {
        let err = ConvertError::RenameConflict {
            source_path: PathBuf::from("/tmp/café_nfd"),
            target: PathBuf::from("/tmp/café"),
        };
        let msg = err.to_string();
        assert!(msg.contains("rename conflict"));
        assert!(msg.contains("café_nfd"));
        assert!(msg.contains("target already exists"));
    }

    #[test]
    fn test_io_permission_detected() {
        let err = ConvertError::Io {
            path: PathBuf::from("/root/file"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied"),
        };
        assert!(err.is_permission_error());
    }

    #[test]
    fn test_io_non_permission_not_detected() {
        let err = ConvertError::Io {
            path: PathBuf::from("/tmp/file"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        assert!(!err.is_permission_error());
    }

    #[test]
    fn test_walk_has_no_path() {
        let err = ConvertError::Walk("some walk error".to_string());
        assert_eq!(err.path(), None);
    }
}
