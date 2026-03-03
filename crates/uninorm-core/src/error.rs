use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NfcError {
    #[error("IO error on path {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to walk directory: {0}")]
    Walk(#[from] walkdir::Error),

    #[error("Rename conflict: {0} already exists")]
    RenameConflict(PathBuf),
}
