# uninorm CLI Reference

English | [한국어](cli.ko.md)

## Subcommands

- [`files`](#files) — Batch rename files/folders (and optionally convert content)
- [`watch`](#watch) — Manage watch entries for the background daemon
- [`daemon`](#daemon) — Manage the background daemon (start/stop/restart)
- [`autostart`](#autostart) — Register daemon to start on login (on/off)
- [`convert`](#convert) — Convert text from NFD to NFC
- [`clipboard`](#clipboard) — Convert clipboard text
- [`check`](#check) — Check if text is NFC-normalized
- [`log`](#log) — View recent conversion log
- [`status`](#status) — Show daemon status, autostart, and watch entry summary

---

## `files`

Recursively scan a directory (or a single file) and rename any NFD filenames to NFC. Optionally convert text content inside files.

```
uninorm files <PATH> [OPTIONS]
```

**Arguments**

| Argument | Description |
|---|---|
| `PATH` | File or directory to process (required) |

**Options**

| Flag | Default | Description |
|---|---|---|
| `--dry-run` | false | Preview changes without renaming or writing anything |
| `--no-recursive` | false | Do not recurse into subdirectories |
| `--content` | false | Also convert text content inside files |
| `--follow-symlinks` | false | Follow symbolic links |
| `--exclude <PATTERN>` | — | Skip entries matching name or glob pattern (repeatable) |
| `--max-size <SIZE>` | 100MB | Maximum file size for content conversion (e.g. `50MB`, `1GB`) |
| `--no-global-ignore` | false | Do not apply global ignore patterns |
| `-y / --yes` | false | Skip confirmation prompt |
| `-v / --verbose` | false | Show individual file changes |
| `--json` | false | Output results as JSON (for scripting/CI) |

**Examples**

```bash
# Preview what would change
uninorm files ~/Downloads --dry-run

# Rename all NFD filenames under ~/Downloads
uninorm files ~/Downloads

# Also fix text inside files (e.g. source code with NFD string literals)
uninorm files ~/Downloads --content

# Skip .git and node_modules
uninorm files ~/project --exclude .git --exclude node_modules

# Single file
uninorm files ~/Downloads/한글파일.txt

# JSON output for scripting/CI
uninorm files ~/Downloads --json --yes
```

**Output**

```
Scanned:  1024
Renamed:  17
Content:  3
```

Exit code is `1` if any rename or write error occurred.

**Notes**

- Content writes are atomic: written to a temp file first, then renamed into place.
- `--exclude` matches against the entry's name only (not the full path).

---

## `watch`

Manage watch entries for the background daemon. Files are auto-converted as they are created or modified.

```
uninorm watch <SUBCOMMAND>
```

### `watch add`

Add or update a watch entry. Starts the daemon automatically if not running.

```bash
uninorm watch add <PATH> [OPTIONS]
```

| Flag | Default | Description |
|---|---|---|
| `--no-recursive` | false | Do not recurse into subdirectories |
| `--content` | false | Convert text content inside files |
| `--follow-symlinks` | false | Follow symbolic links |
| `--exclude <PATTERN>` | — | Skip entries matching name or glob pattern (repeatable) |
| `--max-size <SIZE>` | 100MB | Maximum file size for content conversion |
| `--debounce <MS>` | 300 | Event debounce interval in milliseconds |

### `watch list`

Show all watch entries (numbered).

```bash
uninorm watch list
#  1. /Users/you/Downloads   [enabled]
#  2. /Users/you/Documents   [disabled]  (content, excludes: .git, *.log)
```

### `watch enable` / `watch disable`

Enable or disable entries by number (comma-separated).

```bash
uninorm watch enable 1,2
uninorm watch disable 2
```

### `watch remove`

Remove entries by number (comma-separated).

```bash
uninorm watch remove 1
```

### `watch reset`

Remove all watch entries and stop daemon. Autostart is preserved.

```bash
uninorm watch reset
uninorm watch reset -y   # skip confirmation
```

---

## `daemon`

Manage the background daemon process. Similar to `systemctl start/stop`.

```bash
uninorm daemon start       # Start the daemon
uninorm daemon stop        # Stop the daemon
uninorm daemon restart     # Restart the daemon
```

The daemon watches paths configured via `uninorm watch add` and auto-converts NFD filenames (and optionally content) as filesystem events arrive.

---

## `autostart`

Register or unregister the daemon to start automatically on login. Similar to `systemctl enable/disable`.

- **macOS:** installs a LaunchAgent plist
- **Linux:** installs a systemd user service

```bash
uninorm autostart on       # Enable autostart
uninorm autostart off      # Disable autostart
```

Autostart is automatically registered on first run of any `uninorm` command. `watch reset` does not remove autostart — use `uninorm autostart off` to disable it explicitly.

---

## `convert`

Convert text from NFD to NFC and print the result. Reads from stdin if no text is given.

```
uninorm convert [TEXT] [OPTIONS]
```

| Flag | Description |
|---|---|
| `-c / --clipboard` | Copy result to clipboard |

**Examples**

```bash
uninorm convert "NFD text"
echo "NFD text" | uninorm convert
uninorm convert -c "text"   # convert and copy to clipboard
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

---

## `check`

Check whether a string is already NFC-normalized. Exits with code `1` if it is not.

```
uninorm check <TEXT>
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

## `log`

Show recent entries from the conversion log.

```
uninorm log [-n N]
```

| Flag | Default | Description |
|---|---|---|
| `-n / --lines N` | 50 | Number of recent lines to show |

**Log location:** `~/.config/uninorm/uninorm.log`

---

## `status`

Show daemon status, autostart state, watch entry summary, and recent log activity.

```
uninorm status
```

**Sample output**

```
Daemon running (PID 12345)
Autostart: on
Watch entries: 2/3 enabled
Use `uninorm watch list` for details.

Recent activity:
  [2024-03-09 14:23:15] Renamed: 한글파일.txt → 한글파일.txt
  [2024-03-09 14:30:02] Renamed: café.txt → café.txt
```

---

## Global ignore

Create `~/.config/uninorm/ignore` to define patterns that are always excluded — applied to both the `watch` daemon and `files` by default.

```
# ~/.config/uninorm/ignore
.git
node_modules
target
__pycache__
.DS_Store
*.pyc
```

Format: one glob pattern per line, `#` for comments, blank lines ignored.

The `files` command can opt out with `--no-global-ignore`. The daemon always applies global ignore patterns; use per-entry `--exclude` for entry-specific overrides.

---

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success (or already NFC for `check`) |
| `1` | One or more errors during `files`; text is not NFC for `check` |
