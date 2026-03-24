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

# Watch a directory for new files (starts daemon automatically)
uninorm watch add ~/Downloads

# Set up global ignore patterns (optional)
echo -e ".git\nnode_modules\n.DS_Store" > ~/.config/uninorm/ignore

# Convert clipboard text
uninorm clipboard

# Convert text from NFD to NFC (reads stdin if no text given)
echo "NFD text" | uninorm convert
```

For the full CLI reference, see [docs/cli.md](docs/cli.md).

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
