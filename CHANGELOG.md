# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

#### uninorm-core
- HFS+ NFD → NFC filename conversion (`needs_filename_conversion`, `to_nfc_filename`)
- Text content NFC normalization (`to_nfc`)
- Recursive directory traversal with `contents_first` ordering (children renamed before parents)
- Async file operations via `tokio`
- `ConversionOptions.exclude_patterns`: skip files/directories whose name matches any pattern
- Atomic content write: writes to a temp file then renames to prevent partial writes on failure
- 100 MB guard: files larger than 100 MB are silently skipped during content conversion
- Cross-platform support: macOS uses `hfs_nfd` for HFS+/APFS NFD variant; Linux/Windows use standard `unicode-normalization`

#### uninorm-cli
- `uninorm files [PATH]` — rename NFD filenames to NFC, recursive by default (PATH defaults to `.`)
- `uninorm files --dry-run` — preview changes without modifying files
- `uninorm files --content` — also convert text content inside files
- `uninorm files --exclude <PATTERN>` — skip matching names (repeatable: `--exclude .git --exclude node_modules`)
- `uninorm watch [PATH...]` — watch paths and automatically convert NFD filenames as files appear or are renamed
- `uninorm watch --exclude <PATTERN>` — exclude patterns from watch mode
- `uninorm log [-n N]` — show recent conversion log (default: last 50 entries); log stored at `~/.config/uninorm/uninorm.log`
- `uninorm clipboard` — convert clipboard text to NFC
- `uninorm check <TEXT>` — exit 1 if text is not NFC

### Fixed

#### uninorm-cli
- `uninorm files` now exits with code 1 when any rename or content-write error occurs (previously always exited 0)
