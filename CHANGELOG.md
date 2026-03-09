# Changelog

## [Unreleased]

### Added

#### uninorm-core
- `ConversionOptions.exclude_patterns`: skip files/directories whose name matches any pattern
- Atomic content write: content conversion now writes to a temp file then renames to prevent partial writes
- 100 MB guard: files larger than 100 MB are silently skipped during content conversion

#### uninorm-cli
- `--exclude <PATTERN>` flag for `uninorm files` (repeatable: `--exclude .git --exclude node_modules`)

---

## [0.1.0] — 2026-03-05

### Added

#### uninorm-core
- HFS+ NFD → NFC filename conversion (`needs_filename_conversion`, `convert_path`)
- Text content NFC normalization (`convert_text`)
- Recursive directory traversal with `contents_first` ordering
- Async file operations via `tokio`

#### uninorm-cli
- `uninorm files <path>` — rename NFD filenames to NFC (recursive by default)
- `uninorm files --dry-run` — preview changes without modifying files
- `uninorm files --content` — also convert text content inside files
- `uninorm clipboard` — convert clipboard text to NFC
- `uninorm check <text>` — exit 1 if text is not NFC

