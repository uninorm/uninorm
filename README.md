# mac-uninorm

Converts Unicode NFD filenames and text to NFC on macOS.

macOS HFS+/APFS stores filenames in a non-standard NFD variant, causing Korean, Japanese kana, and accented Latin characters to appear broken on Linux and Windows.

> 한국어: [README.ko.md](README.ko.md)

---

## CLI

### Install

**Homebrew (recommended):**

```bash
brew tap sts07142/uninorm
brew install uninorm
```

**From source:**

```bash
cargo install --path crates/uninorm-cli
```

### Usage

```bash
# Preview changes (no files modified)
uninorm files ~/Downloads --dry-run

# Rename files and folders recursively
uninorm files ~/Downloads

# Also convert text inside files
uninorm files ~/Downloads --content

# Convert clipboard text
uninorm clipboard

# Check if text is NFC (exits 1 if not)
uninorm check "東京"
```

### Options for `files`

| Flag | Default | Description |
|---|---|---|
| `--dry-run` | false | Preview only, no writes |
| `-r / --recursive` | true | Recurse into subdirectories |
| `--content` | false | Convert text inside files too |
| `--follow-symlinks` | false | Follow symbolic links |

---

## GUI (macOS menu bar app)

A macOS menu bar app that watches folders and automatically converts NFD filenames to NFC as files are created or renamed.

### Install

**From source (requires Rust + macOS):**

```bash
# Run directly

# Build a distributable .app bundle (requires cargo-bundle)
cargo install cargo-bundle
make bundle
# → target/release/bundle/osx/uninorm.app
```

### Features

| Feature | Description |
|---|---|
| **Menu bar** | Runs as a menu bar icon (hidden from Dock) |
| **File browser** | Browse files in Hierarchy / List / Icon / Gallery view |
| **Watched paths** | Register folders to monitor for NFD filenames |
| **Auto-convert** | Automatically converts filenames on Create/Rename events |
| **Scan All** | Manually scan all watched paths for existing NFD filenames |
| **Bookmarks** | Save frequently-used paths for quick navigation and one-click add |
| **Activity log** | In-app log + persistent log file at `~/.config/uninorm/uninorm.log` |
| **Login auto-start** | Optional LaunchAgent to start at login |
| **Language** | English / Korean UI (persisted in config) |
| **Drag & drop** | Drag folders onto the window to add them to watched paths |

### Configuration

Config is stored at `~/.config/uninorm/config.json`:

```json
{
  "watched_paths": ["/Users/you/Downloads"],
  "inactive_paths": [],
  "bookmarks": [],
  "lang": "English"
}
```

### Why doesn't auto-convert always trigger?

APFS normalizes filenames to NFC when written, so new files created on a modern Mac are already NFC. Auto-convert triggers when NFD filenames arrive from external sources (USB drives, network shares, old HFS+ volumes). Use **Scan All** to convert any existing NFD files.

---

## How it works

macOS decomposes characters like `강` (U+AC15) into separate code points (`ᄀ` + `ᅡ` + `ᆼ`) when writing to the filesystem. The same applies to Japanese voiced kana (e.g. `が` → `か` + `゛`) and Latin characters with diacritics (e.g. `é` → `e` + `´`).

`uninorm` composes them back into precomposed NFC form, which other systems expect.

> **Note:** macOS uses a non-standard HFS+ NFD for filenames that differs from Unicode Standard Annex #15 NFD. `uninorm` handles both variants correctly.

---

## Workspace

| Crate | Description | Status |
|---|---|---|
| `uninorm-core` | Core library (cross-platform) | Done |
| `uninorm-cli` | CLI binary | Done |

---

## License

MIT
