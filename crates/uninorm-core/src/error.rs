use std::path::PathBuf;

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
}

impl From<walkdir::Error> for ConvertError {
    fn from(e: walkdir::Error) -> Self {
        ConvertError::Walk(e.to_string())
    }
}
