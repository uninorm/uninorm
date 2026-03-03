# mac-uninorm

Converts Unicode NFD filenames and text to NFC on macOS.

macOS HFS+/APFS stores filenames in a non-standard NFD variant, causing Korean, Japanese kana, and accented Latin characters to appear broken on Linux and Windows.

> 한국어: [README.ko.md](README.ko.md)

## Install

```bash
cargo install --path crates/uninorm-cli
```

## Usage

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

## How it works

macOS decomposes characters like `강` (U+AC15) into separate code points (`ᄀ` + `ᅡ` + `ᆼ`) when writing to the filesystem. The same applies to Japanese voiced kana (e.g. `が` → `か` + `゛`) and Latin characters with diacritics (e.g. `é` → `e` + `´`).

`uninorm` composes them back into precomposed NFC form, which other systems expect.

> **Note:** macOS uses a non-standard HFS+ NFD for filenames that differs from Unicode Standard Annex #15 NFD. `uninorm` handles both variants correctly.

## Workspace

| Crate | Description |
|---|---|
| `uninorm-core` | Core library (cross-platform) |
| `uninorm-cli` | CLI binary |

## License

MIT
