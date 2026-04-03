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
- Configurable max content size guard (default 100 MB)
- Cross-platform support: macOS uses `hfs_nfd` for HFS+/APFS NFD variant; Linux/Windows use standard `unicode-normalization`
- Pre-scan with confirmation prompt before applying changes

#### uninorm-cli
- `uninorm files <PATH>` — rename NFD filenames to NFC, recursive by default
- `uninorm files --dry-run` — preview changes without modifying files
- `uninorm files --content` — also convert text content inside files
- `uninorm files --exclude <PATTERN>` — skip matching names (repeatable)
- `uninorm files --max-size <SIZE>` — maximum file size for content conversion (default 100MB)
- `uninorm files -y/--yes` — skip confirmation prompt
- `uninorm files -v/--verbose` — show individual file changes
- `uninorm files --no-global-ignore` — opt out of global ignore patterns
- `uninorm watch add <PATH>` — add a watch entry (starts daemon automatically)
- `uninorm watch remove/list/enable/disable/reset` — manage watch entries
- `uninorm daemon start/stop/restart` — manage the background daemon process
- `uninorm autostart on/off` — register/unregister daemon to start on login (LaunchAgent on macOS, systemd on Linux)
- `uninorm convert [TEXT]` — convert text from NFD to NFC (reads stdin if no text given)
- `uninorm convert -c` — convert and copy result to clipboard
- `uninorm clipboard` — convert clipboard text to NFC
- `uninorm check <TEXT>` — exit 1 if text is not NFC
- `uninorm log [-n N]` — show recent conversion log (default: last 50 entries)
- `uninorm status` — show daemon status, autostart state, watch entry summary, and recent activity
- `uninorm files --json` — output results as JSON for scripting/CI
- `uninorm check --json` — output NFC check result as JSON
- `uninorm convert --json` — output conversion result as JSON
- `uninorm status --json` — output daemon status as JSON
- CLI helper functions (`parse_size`, `format_size`, `parse_indices`) extracted to lib with unit tests

#### uninorm-core
- `ConvertError::PermissionDenied`, `ContentTooLarge`, `RenameConflict` error variants
- `ConvertError::path()` and `is_permission_error()` helper methods
- `ConversionStats.files_skipped` and `directories_scanned` fields
- `Display` trait implementation for `ConversionStats`
- `MAX_WALK_DEPTH = 256` constant shared between CLI and daemon
- Doc-tests for `to_nfc`, `to_nfc_filename`, `is_nfc`, `needs_filename_conversion`, `convert_text`, `compile_excludes`, `is_excluded`
- Criterion benchmarks for normalize, convert_text, and compile_excludes

#### uninorm-daemon
- Background daemon with filesystem watching (FSEvents on macOS, inotify on Linux)
- Watch entry config with per-entry options (recursive, content, follow-symlinks, exclude, max-size)
- **Initial scan on daemon start**: pre-existing NFD files are converted when the daemon starts or reloads config, not just new filesystem events
- Daemon controller: start/stop/restart/reload
- Autostart: LaunchAgent (macOS) with immediate `launchctl load`, systemd user service (Linux)
- Debounce support for filesystem events
- PID file management with stale PID cleanup
- **Global ignore file** (`~/.config/uninorm/ignore`): define patterns excluded from all watch entries and `files` commands by default
- UID/GID preservation during content conversion on Unix
- Non-UTF-8 filenames safely skipped with log message

### Changed

- Removed silent auto-install of autostart on every CLI command (users must explicitly opt-in via `uninorm autostart on`)
- Reduced scan_path concurrent file reads from 32 to 8 to limit peak memory usage
- CI clippy now checks benchmark targets (`--all-targets`)

### Fixed

#### uninorm-cli
- `uninorm files` now exits with code 1 when any rename or content-write error occurs (previously always exited 0)

#### uninorm-daemon
- `DaemonError::UnsupportedPlatform` message now correctly says "macOS and Linux" (was "macOS" only)
- `DaemonController::stop` returns error when daemon fails to exit after SIGTERM (was silent success)
- Daemon chown failure now logs a warning instead of being silently ignored
- Filter out `Name(From)` rename events to reduce unnecessary syscalls

### Security

- PID file: use `O_CREAT|O_EXCL` exclusive create to prevent concurrent daemon start race condition
- PID file: reject symlinks at PID path to prevent symlink-following attacks
- Temp files: create with mode `0o600` on Unix to prevent content exposure during write-rename window
- Fix temp file leak on write failure in `convert_single_content`
- LaunchAgent plist: XML-escape executable and log paths to prevent injection
- Systemd unit: escape spaces in `ExecStart` path per systemd.service(5) spec
