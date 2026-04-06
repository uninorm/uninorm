//! Core library for Unicode NFD→NFC normalization.
//!
//! `uninorm-core` provides cross-platform utilities for converting filenames and text
//! content from Unicode NFD (Normalization Form Decomposed) to NFC (Normalization Form
//! Composed). On macOS, it handles the non-standard HFS+/APFS NFD variant that
//! decomposes Korean Hangul, Japanese kana voiced marks, and Latin diacritics differently
//! from standard Unicode NFD.
//!
//! # Modules
//!
//! - [`normalize`] — low-level NFC conversion functions
//! - [`file_ops`] — directory walking, file renaming, content conversion, and scanning
//! - [`error`] — error types for conversion operations

pub mod error;
pub mod file_ops;
pub mod normalize;

pub use error::ConvertError;
pub use file_ops::{
    compile_excludes, convert_path, convert_text, is_excluded, same_inode, scan_path, temp_name,
    ConversionOptions, ConversionStats, ScanEntry, ScanResult, DEFAULT_MAX_CONTENT_BYTES,
    MAX_WALK_DEPTH,
};
pub use normalize::{is_nfc, needs_filename_conversion, to_nfc, to_nfc_filename};
