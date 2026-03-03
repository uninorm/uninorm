pub mod error;
pub mod file_ops;
pub mod normalize;

pub use error::NfcError;
pub use file_ops::{convert_path, convert_text, ConversionOptions, ConversionStats};
pub use normalize::{is_nfc, needs_filename_conversion, to_nfc, to_nfc_filename};
