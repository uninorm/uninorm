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
