# uninorm

Converts Unicode NFD filenames and text to NFC — works on macOS, Linux, and Windows.

macOS HFS+/APFS stores filenames in a non-standard NFD variant, causing Korean, Japanese kana, and accented Latin characters to appear broken on Linux and Windows.

English | [한국어](README.ko.md)

---

## Install

**Homebrew (recommended):**

```bash
brew tap uninorm/uninorm
brew install uninorm
```

**From source:**

```bash
cargo install --path crates/uninorm-cli
```

---

## Quick start

```bash
# Preview changes (nothing is modified)
uninorm files ~/Downloads --dry-run

# Rename all NFD filenames under a path
uninorm files ~/Downloads

# Also convert text content inside files
uninorm files ~/Downloads --content

# Convert clipboard text
uninorm clipboard

# Check if text is NFC (exits 1 if not)
uninorm check "東京"

# Convert text from NFD to NFC (reads stdin if no text given)
echo "NFD text" | uninorm convert
```

---

## `files` — one-time conversion

```bash
uninorm files <path> [options]
```

| Flag | Default | Description |
|---|---|---|
| `--dry-run` | false | Preview only, no writes |
| `--no-recursive` | false | Do not recurse into subdirectories |
| `--content` | false | Convert text inside files too |
| `--follow-symlinks` | false | Follow symbolic links |
| `--exclude <PATTERN>` | — | Skip entries matching name or glob pattern (repeatable) |
| `--max-size <SIZE>` | 100MB | Maximum file size for content conversion (e.g. `50MB`, `1GB`) |
| `-y / --yes` | false | Skip confirmation prompt |
| `-v / --verbose` | false | Show individual file changes |

---

## `watch` — manage watch entries

Manage watch entries for the background daemon. Files are auto-converted as they are created or modified.

```bash
# Add a path to watch (starts daemon automatically)
uninorm watch add ~/Downloads
uninorm watch add ~/Documents --content --exclude .git --exclude "*.log" --max-size 200MB

# List all entries (numbered)
uninorm watch list
#  1. /Users/you/Downloads   [enabled]
#  2. /Users/you/Documents   [disabled]  (content, excludes: .git, *.log)

# Enable/disable by number (comma-separated)
uninorm watch enable 1,2
uninorm watch disable 2

# Remove by number
uninorm watch remove 1

# Remove all entries and stop daemon (autostart is preserved)
uninorm watch reset
```

### Watch entry options

| Flag | Default | Description |
|---|---|---|
| `--no-recursive` | false | Do not recurse into subdirectories |
| `--content` | false | Convert text content inside files |
| `--follow-symlinks` | false | Follow symbolic links |
| `--exclude <PATTERN>` | — | Skip entries matching name or glob pattern (repeatable) |
| `--max-size <SIZE>` | 100MB | Maximum file size for content conversion |
| `--debounce <MS>` | 300 | Event debounce interval in milliseconds |

---

## `daemon` — manage the background daemon

```bash
uninorm daemon start       # Start the daemon
uninorm daemon stop        # Stop the daemon
uninorm daemon restart     # Restart the daemon
```

---

## `autostart` — login-time auto-start

Register or unregister the daemon to start automatically on login (LaunchAgent on macOS, systemd on Linux).

```bash
uninorm autostart on       # Enable autostart
uninorm autostart off      # Disable autostart
```

Autostart is automatically registered on first run of any `uninorm` command. `watch reset` does not remove autostart — use `uninorm autostart off` to disable it explicitly.

---

## Other commands

```bash
uninorm convert "text"     # Convert text from NFD to NFC
echo "text" | uninorm convert  # Read from stdin
uninorm convert -c "text"  # Convert and copy to clipboard
uninorm clipboard          # Convert clipboard text from NFD to NFC
uninorm check "text"       # Check if text is already NFC-normalized
uninorm status             # Show daemon status, autostart, and entry summary
uninorm log -n 50          # Show recent conversion log (last 50 entries)
```

---

## How it works

macOS decomposes characters like `강` (U+AC15) into separate code points (`ᄀ` + `ᅡ` + `ᆼ`) when writing to the filesystem. The same applies to Japanese voiced kana (e.g. `が` → `か` + `゛`) and Latin characters with diacritics (e.g. `é` → `e` + `´`).

`uninorm` composes them back into precomposed NFC form, which other systems expect.

> **Note:** macOS uses a non-standard HFS+ NFD for filenames that differs from Unicode Standard Annex #15 NFD. `uninorm` handles both variants correctly using the [`hfs_nfd`](https://crates.io/crates/hfs_nfd) crate. On Linux and Windows, standard Unicode NFC normalization is used.

---

## Workspace

| Crate | Description |
|---|---|
| `uninorm-core` | Core library — normalization, file operations, scanning |
| `uninorm-cli` | CLI binary — `files`, `watch`, `daemon`, `autostart`, `convert`, `clipboard`, `check` |
| `uninorm-daemon` | Daemon library — config, controller, autostart, background watcher |

---

## License

MIT
