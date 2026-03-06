# uninorm CLI Reference

> 한국어: [cli.ko.md](cli.ko.md)

## Subcommands

- [`files`](#files) — Batch rename files/folders (and optionally convert content)
- [`watch`](#watch) — Real-time watcher: auto-rename files as they appear
- [`log`](#log) — View recent conversion log
- [`clipboard`](#clipboard) — Convert clipboard text
- [`check`](#check) — Check if text is NFC-normalized

---

## `files`

Recursively scan a directory (or a single file) and rename any NFD filenames to NFC. Optionally convert text content inside files.

```
uninorm files [PATH] [OPTIONS]
```

**Arguments**

| Argument | Default | Description |
|---|---|---|
| `PATH` | `.` (current directory) | File or directory to process |

**Options**

| Flag | Default | Description |
|---|---|---|
| `--dry-run` | false | Preview changes without renaming or writing anything |
| `-r / --recursive` | true | Recurse into subdirectories |
| `--content` | false | Also convert text content inside files |
| `--follow-symlinks` | false | Follow symbolic links |
| `--exclude <PATTERN>` | — | Skip entries whose name matches PATTERN (repeatable) |

**Examples**

```bash
# Preview what would change in the current directory
uninorm files --dry-run

# Rename all NFD filenames under ~/Downloads
uninorm files ~/Downloads

# Also fix text inside files (e.g. source code with NFD string literals)
uninorm files ~/Downloads --content

# Skip .git and node_modules
uninorm files ~/project --exclude .git --exclude node_modules

# Single file
uninorm files ~/Downloads/한글파일.txt
```

**Output**

```
Scanned:  1024
Renamed:  17
Content:  3
```

Exit code is `1` if any rename or write error occurred.

**Notes**

- Files larger than 100 MB are skipped for content conversion.
- Content writes are atomic: written to a temp file first, then renamed into place.
- `--exclude` matches against the entry's name only (not the full path).

---

## `watch`

Watch one or more directories and automatically rename files from NFD to NFC as they are created or renamed. Uses the native filesystem event API (FSEvents on macOS, inotify on Linux, ReadDirectoryChanges on Windows).

```
uninorm watch [PATH...] [OPTIONS]
```

**Arguments**

| Argument | Default | Description |
|---|---|---|
| `PATH` | `.` (current directory) | One or more directories to watch (space-separated) |

**Options**

| Flag | Default | Description |
|---|---|---|
| `--exclude <PATTERN>` | — | Skip entries whose name matches PATTERN (repeatable) |

**Examples**

```bash
# Watch the current directory
uninorm watch

# Watch multiple paths
uninorm watch ~/Downloads ~/Desktop

# Watch but skip .git directories
uninorm watch ~/project --exclude .git

# Run in the background (shell job control)
uninorm watch ~/Downloads &
```

**Output**

```
Watching: /Users/you/Downloads
Press Ctrl+C to stop.

Renamed: 한글파일.txt → 한글파일.txt
```

Each conversion is printed to stdout and appended to the log file at `~/.config/uninorm/uninorm.log`.

Press **Ctrl+C** to stop the watcher gracefully.

**Notes**

- APFS on modern macOS normalizes new filenames to NFC, so `watch` will mostly trigger on files copied from external sources (USB drives, network shares, older HFS+ volumes).
- `watch` does not do a full scan on startup — it only processes new events. Use `uninorm files` first to convert existing NFD filenames.

---

## `log`

Show recent entries from the conversion log written by `watch`.

```
uninorm log [-n N]
```

**Options**

| Flag | Default | Description |
|---|---|---|
| `-n / --lines N` | 50 | Number of recent lines to show |

**Log location:** `~/.config/uninorm/uninorm.log`

**Examples**

```bash
# Show last 50 log entries (default)
uninorm log

# Show last 100 entries
uninorm log -n 100

# Show all entries (pipe through a pager)
uninorm log -n 99999 | less
```

**Sample output**

```
[2024-03-09 14:22:01] Watching: /Users/you/Downloads
[2024-03-09 14:23:15] Renamed: 한글파일.txt → 한글파일.txt
[2024-03-09 14:30:02] Watch stopped.

(3 total entries, showing last 3)
```

---

## `clipboard`

Read the clipboard, convert any NFD text to NFC, and write the result back.

```
uninorm clipboard
```

**Examples**

```bash
uninorm clipboard
# → "Clipboard converted to NFC."
# → "Clipboard is already NFC — no changes made."
```

Useful as a post-paste step or bound to a keyboard shortcut.

---

## `check`

Check whether a string is already NFC-normalized. Exits with code `1` if it is not.

```
uninorm check TEXT
```

**Examples**

```bash
uninorm check "東京"
# ✓ Already NFC

uninorm check $'か\u3099'   # か + combining dakuten (NFD)
# ✗ NOT NFC — converted form: が

# Use in scripts
if ! uninorm check "$filename"; then
  echo "Filename needs normalization"
fi
```

---

## Log file

`watch` appends timestamped entries to:

```
~/.config/uninorm/uninorm.log
```

The directory is created automatically on first run.

---

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success (or already NFC for `check`) |
| `1` | One or more errors during `files`; text is not NFC for `check` |
