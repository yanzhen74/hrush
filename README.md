# hrush

[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

> A high-performance terminal hex editor written in Rust, inspired by `bvi` but faster.

## Features

- **High Performance** — Written in Rust with memory-mapped file support (`mmap`) for large files.
- **Vi-style Key Bindings** — Familiar `hjkl` navigation and modal editing for power users.
- **Seven Modes** — Normal / Insert / Replace / Visual / Command / Search / Help.
- **Dual-panel Editing** — Hex and ASCII views side-by-side with `Ctrl+W` to switch panels.
- **Jump List** — Navigate through your jump history with `Ctrl+O` (back) and `Tab` (forward), just like Vim's jumplist.
- **Search & Replace** — Supports both hex (`x:AABB`) and ASCII patterns, with global or single replacement.
- **Multi-step Undo / Redo** — Grouped actions with automatic merge of adjacent edits.
- **Hex Text Import / Export** — Import hex text files and save as binary; export binary to formatted hex text.
- **Frame Mode** — Split files by fixed length (`:frame len=N`) or sync word (`:frame sync=XXYY`). Each frame is displayed on its own line with frame number, offset, and length. Full editing, undo/redo, and horizontal scrolling are supported.
- **Modified Byte Highlighting** — Changed bytes are highlighted in yellow for easy tracking.
- **Append Insert (`a`)** — Press `a` to insert after cursor, just like Vim's append mode.
- **Search Progress Bar** — Async search with real-time progress display (percentage + match count), cancelable with `Esc`.
- **Count Prefix** — Type a number before commands to repeat them (e.g., `3l`, `5h`, `2dd`, `3x`), just like Vim.
- **Visual Mode** — Press `v` to select bytes, `V` for whole-line selection, `Ctrl+V` (or `:block`) for rectangular block selection. Use movement keys to extend selection, `y` to yank (copy), `d` to cut, and `p` to paste.
- **Block (Rectangular) Editing** — Yank/cut/paste rectangular blocks; `p` pastes a block row-by-row at the cursor, `Ctrl+P` (or `:overpaste`) overwrites without growing the file. In block mode, `i`/`a` insert/append the typed bytes across every selected row, undone as a single change.
- **Checksums** — Compute `:sum8/:sum16/:sum32`, `:crc16` (configurable poly/init/refin/refout/xorout), `:crc32`, `:md5`, `:sha256` over the selection or the whole file.
- **Repeat Last Change** — `.` repeats the last edit (supports count prefix, e.g., `3.`).
- **Built-in Help System** — Press `?` or `F1` for full-screen help with all keybindings and commands. Use `:help [topic]` to jump to specific topics. Status bar displays the current mode with color-coded indicators.

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
 h/j/k/l move | : command | Ctrl+W switch panel
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
| `F2` | Toggle Raw / Frame mode |
| `Ctrl+Right` | Horizontal scroll right (Frame mode) |
| `Ctrl+Left` | Horizontal scroll left (Frame mode) |
| `i` | Enter Insert mode |
| `a` | Append after cursor (Insert mode) |
| `r` | Single byte replace (next keystroke) |
| `R` | Enter Replace mode |
| `x` | Delete byte at cursor |
| `dd` | Delete current line (16 bytes) |
| `u` | Undo |
| `Ctrl+R` | Redo |
| `Ctrl+O` | Jump back to previous location |
| `Tab` | Jump forward to next location |
| `Ctrl+W` | Switch between Hex and ASCII panel |
| `/` | Enter Search mode |
| `n` | Jump to next search match |
| `N` | Jump to previous search match |
| `v` | Enter Visual mode (select bytes) |
| `V` | Enter Visual Line mode (select whole rows/frames) |
| `Ctrl+V` | Enter Visual Block mode (rectangular selection; use `:block` if the terminal intercepts it) |
| `p` | Paste yanked bytes after cursor (row-by-row for block yanks) |
| `Ctrl+P` | Overwrite paste, no file growth, clamped at EOF (use `:overpaste` if the terminal intercepts it) |
| `.` | Repeat last change (with optional count prefix) |
| `?` / `F1` | Open help page |
| `1-9` | Count prefix for repeating commands |
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

### Visual Mode

| Key | Action |
|-----|--------|
| `h`, `←` | Extend selection left |
| `l`, `→` | Extend selection right |
| `k`, `↑` | Extend selection up |
| `j`, `↓` | Extend selection down |
| `0` | Extend selection to start of line |
| `$` | Extend selection to end of line |
| `G` | Extend selection to end of file |
| `v` / `V` / `Ctrl+V` | Switch between char / line / block selection (anchor unchanged) |
| `y` | Yank (copy) selection |
| `d`, `x` | Delete (cut) selection |
| `i` | Block insert at the left edge (block mode only; typed bytes are applied to every selected row) |
| `a` | Block append at the right edge (block mode only) |
| `:` | Enter Command mode; checksum / `:fill` / `:set` commands apply to the selection |
| `Esc` | Cancel selection, return to Normal mode |

### Help Mode

| Key | Action |
|-----|--------|
| `j`, `↓` | Scroll down |
| `k`, `↑` | Scroll up |
| `Ctrl+F`, `PageDown` | Page down |
| `Ctrl+B`, `PageUp` | Page up |
| `gg` | Jump to top |
| `G` | Jump to bottom |
| `q`, `Esc` | Close help |

### Command Mode

Type a command after `:` and press `Enter`.

| Command | Action |
|---------|--------|
| `:w [path]` | Save file (optionally to a new path) |
| `:q` | Quit (fails if unsaved) |
| `:q!` | Force quit without saving |
| `:wq` | Save and quit |
| `:w! [path]` | Force save (bypass fixed-length frame alignment warning) |
| `:goto <offset>` | Jump to offset (decimal or `0x` hex) |
| `:frame len=N` | Enable frame mode with fixed length N |
| `:frame sync=XXYY` | Enable frame mode with sync word (hex) |
| `:frame off` | Disable frame mode, return to raw view |
| `:import <path>` | Import a hex text file |
| `:export <path>` | Export current buffer as hex text |
| `:s/old/new` | Replace current match |
| `:%s/old/new/g` | Replace all matches globally |
| `:fill BYTE` | Fill selection with a byte value (hex `0xAA` or decimal `255`); requires selection |
| `:set HEXBYTES` | Overwrite selection with repeating hex bytes, e.g. `:set AABB`; requires selection |
| `:block` | Enter Visual Block mode (alternative to `Ctrl+V`) |
| `:overpaste` | Overwrite paste (alternative to `Ctrl+P`); alias `:op` |
| `:sum` / `:sum8` / `:sum16` / `:sum32` | Additive checksums; word order follows global endianness (selection or whole file) |
| `:crc16` / `:crc32` | CRC checksums; `:crc16` supports `poly=`/`init=`/`refin=`/`refout=`/`xorout=` options |
| `:md5` / `:sha256` | Hash the selection or whole file |
| `:help [topic]` | Open help (topics: overview, navigation, editing, visual, search, commands, frame) |

> In `:s` and `:%s` commands, both `old` and `new` support hex patterns with the `x:` prefix (e.g., `:%s/x:DEAD/x:BEEF/g`). Without the prefix, the pattern is treated as ASCII text.

### Search Mode

| Key | Action |
|-----|--------|
| Any text | Input search pattern |
| `Enter` | Execute search and return to Normal mode |
| `Esc` | Cancel search and return to Normal mode |

> Search patterns starting with `x:` are treated as hex (e.g., `x:DEADBEEF`). Otherwise the pattern is treated as ASCII.

> For large files, a progress bar is displayed in the status bar during search. Press `Esc` to cancel an ongoing search.

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

## Frame Mode

Frame mode splits the file into logical frames and displays one frame per line. This is useful for analyzing structured binary data such as telemetry packets or network frames.

### Enabling Frame Mode

- **Fixed length**: `:frame len=64` splits the file into 64-byte frames.
- **Sync word**: `:frame sync=AABB` splits at each occurrence of the hex sync word `AABB`.
- **Disable**: `:frame off` returns to the normal raw hex view.
- **Quick toggle**: Press `F2` to switch between raw and frame mode.

### Frame Mode Display

Each line shows:
- `#NNNN` — Frame number (1-based)
- `@XXXXXXXX` — File offset of the frame start
- `LXXXX` — Frame length in bytes
- Horizontal coordinate ruler (`00 01 02 ...`) for byte position within the frame

### Frame Mode Navigation

All normal-mode navigation keys work in frame mode:
- `h`/`l`/`j`/`k` — Move within/across frames
- `0` / `$` — Start / end of current frame
- `gg` / `G` — First / last frame
- `Ctrl+F` / `Ctrl+B` — Page down / up
- `Ctrl+Right` / `Ctrl+Left` — Horizontal scroll by one screen width

### Editing in Frame Mode

- Insert (`i`) and Replace (`R`) work exactly as in raw mode.
- Undo (`u`) / Redo (`Ctrl+R`) are fully supported.
- After each edit, frame offsets and lengths are adjusted incrementally in O(n), so rows stay aligned and the `LNN` length label reflects the edit immediately (e.g. inserting one byte per frame turns `L24` into `L25`).
- Block paste / block insert sessions are O(n) and remain responsive even on multi-MB files with tens of thousands of frames.
- **Caveat**: In fixed-length mode, if your edits change the total file size so it is no longer a multiple of the frame length, `:w` will warn you. Use `:w!` to force save anyway.

## License

[MIT](LICENSE)
