# hrush

[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

> A high-performance terminal hex editor written in Rust, inspired by `bvi` but faster.

## Features

- **High Performance** — Written in Rust with memory-mapped file support (`mmap`) for large files.
- **Vi-style Key Bindings** — Familiar `hjkl` navigation and modal editing for power users.
- **Five Modes** — Normal / Insert / Replace / Command / Search.
- **Dual-panel Editing** — Hex and ASCII views side-by-side with `Tab` to switch panels.
- **Search & Replace** — Supports both hex (`x:AABB`) and ASCII patterns, with global or single replacement.
- **Multi-step Undo / Redo** — Grouped actions with automatic merge of adjacent edits.
- **Hex Text Import / Export** — Import hex text files and save as binary; export binary to formatted hex text.
- **Modified Byte Highlighting** — Changed bytes are highlighted in yellow for easy tracking.

## Screenshot

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Hex View                                                                 │
│  Offset  │ Hex                                      │ ASCII              │
│ 00000000│ 48 65 6C 6C 6F 20 57 6F  72 6C 64 21 0A 00 01 02 │Hello World!....│
│ 00000010│ A0 B1 C2 D3 E4 F5 67 89  0A 0C 15 37 AA BB CC DD │......g....7....│
│ 0000001A│ CC EE AA BB 01 02 03 04  -- -- -- -- -- -- -- -- │................│
└──────────────────────────────────────────────────────────────────────────┘
 NORMAL  example.bin | 256 B | 0x00000010 (16) [+]
 h/j/k/l move | : command | Tab switch panel
```

## Installation

### From Source

```bash
git clone https://github.com/yanzhen74/hrush.git
cd hrush
cargo install --path .
```

### From Release

Download the pre-built binary from the [Releases](https://github.com/yanzhen74/hrush/releases) page.

## Usage

```bash
# Open a binary file
hrush <file>

# Import a hex text file
hrush --import hex.txt
```

## Key Bindings

### Normal Mode

| Key | Action |
|-----|--------|
| `h`, `←` | Move cursor left |
| `l`, `→` | Move cursor right |
| `k`, `↑` | Move cursor up |
| `j`, `↓` | Move cursor down |
| `gg` | Go to start of file |
| `G` | Go to end of file |
| `0` | Go to start of line |
| `$` | Go to end of line |
| `Ctrl+F` | Page down |
| `Ctrl+B` | Page up |
| `i` | Enter Insert mode |
| `r` | Single byte replace (next keystroke) |
| `R` | Enter Replace mode |
| `x` | Delete byte at cursor |
| `dd` | Delete current line (16 bytes) |
| `u` | Undo |
| `Ctrl+R` | Redo |
| `Tab` | Switch between Hex / ASCII panel |
| `/` | Enter Search mode |
| `n` | Jump to next search match |
| `N` | Jump to previous search match |
| `:` | Enter Command mode |

### Insert Mode

| Key | Action |
|-----|--------|
| `0-9`, `a-f` | Enter hex digits (two characters = one byte) in Hex panel |
| Any printable ASCII | Insert ASCII character in ASCII panel |
| `Esc` | Return to Normal mode |

### Replace Mode

| Key | Action |
|-----|--------|
| `0-9`, `a-f` | Overwrite hex digits in Hex panel |
| Any printable ASCII | Overwrite ASCII character in ASCII panel |
| `Esc` | Return to Normal mode |

### Command Mode

Type a command after `:` and press `Enter`.

| Command | Action |
|---------|--------|
| `:w [path]` | Save file (optionally to a new path) |
| `:q` | Quit (fails if unsaved) |
| `:q!` | Force quit without saving |
| `:wq` | Save and quit |
| `:goto <offset>` | Jump to offset (decimal or `0x` hex) |
| `:import <path>` | Import a hex text file |
| `:export <path>` | Export current buffer as hex text |
| `:s/old/new` | Replace current match |
| `:%s/old/new/g` | Replace all matches globally |

> In `:s` and `:%s` commands, both `old` and `new` support hex patterns with the `x:` prefix (e.g., `:%s/x:DEAD/x:BEEF/g`). Without the prefix, the pattern is treated as ASCII text.

### Search Mode

| Key | Action |
|-----|--------|
| Any text | Input search pattern |
| `Enter` | Execute search and return to Normal mode |
| `Esc` | Cancel search and return to Normal mode |

> Search patterns starting with `x:` are treated as hex (e.g., `x:DEADBEEF`). Otherwise the pattern is treated as ASCII.

## Command Reference

| Command | Description |
|---------|-------------|
| `hrush <file>` | Open a binary file for editing |
| `hrush --import <file>` | Import a hex text file and convert to binary |

## Hex Text Import Format

Hex text files accepted by `--import` and `:import` follow these rules:

- Each line may contain one or more space-separated hex byte sequences.
- Empty lines are ignored.
- Lines starting with `#` are treated as comments and ignored.
- Each hex chunk must have an even number of characters.
- Hex digits may be uppercase or lowercase.

### Example

```text
# Boot sector header
EB 3C 90 4D 53 44 4F 53  35 2E 30

# Volume label
00 02 40 00 02 00
```

> When imported, the file is saved as a `.bin` file with the same base name.

## License

[MIT](LICENSE)
