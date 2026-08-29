pub struct HelpSection {
    pub id: &'static str,
    pub title: &'static str,
    pub entries: &'static [HelpEntry],
}

pub struct HelpEntry {
    pub key: &'static str,
    pub description: &'static str,
}

pub static HELP_SECTIONS: &[HelpSection] = &[
    HelpSection {
        id: "overview",
        title: "Overview",
        entries: &[
            HelpEntry { key: "hrush", description: "A hex editor TUI inspired by vim, written in Rust" },
            HelpEntry { key: "", description: "Seven modes: Normal, Insert, Replace, Command, Search, Visual, Help" },
            HelpEntry { key: "", description: "Supports hex editing, ASCII editing, frame analysis, search/replace" },
            HelpEntry { key: "", description: "Type inspector decodes bytes at cursor as integers, floats, string, hex" },
            HelpEntry { key: "", description: "Use :help <topic> to jump to a specific topic" },
        ],
    },
    HelpSection {
        id: "navigation",
        title: "Navigation",
        entries: &[
            HelpEntry { key: "h / Left", description: "Move cursor left" },
            HelpEntry { key: "j / Down", description: "Move cursor down (next row)" },
            HelpEntry { key: "k / Up", description: "Move cursor up (previous row)" },
            HelpEntry { key: "l / Right", description: "Move cursor right" },
            HelpEntry { key: "0", description: "Jump to start of current row" },
            HelpEntry { key: "$", description: "Jump to end of current row" },
            HelpEntry { key: "gg", description: "Jump to start of file" },
            HelpEntry { key: "G", description: "Jump to end of file" },
            HelpEntry { key: "Ctrl+F / PageDown", description: "Page down (jump forward one screen)" },
            HelpEntry { key: "Ctrl+B / PageUp", description: "Page up (jump backward one screen)" },
            HelpEntry { key: "Ctrl+O", description: "Jump back to previous location (jumplist)" },
            HelpEntry { key: "Tab", description: "Jump forward to next location (jumplist)" },
            HelpEntry { key: "Ctrl+W", description: "Switch between Hex and ASCII panel" },
            HelpEntry { key: "{N}<motion>", description: "Repeat motion N times (e.g. 3l, 5h, 10j)" },
        ],
    },
    HelpSection {
        id: "editing",
        title: "Editing",
        entries: &[
            HelpEntry { key: "i", description: "Enter Insert mode (insert before cursor)" },
            HelpEntry { key: "a", description: "Enter Insert mode (insert after cursor)" },
            HelpEntry { key: "r", description: "Replace single byte (hex nibble-by-nibble or ASCII)" },
            HelpEntry { key: "R", description: "Enter Replace mode (continuous overwrite)" },
            HelpEntry { key: "x", description: "Delete byte at cursor" },
            HelpEntry { key: "dd", description: "Delete entire row (16 bytes width)" },
            HelpEntry { key: "p", description: "Paste yanked/deleted bytes after cursor" },
            HelpEntry { key: ".", description: "Repeat last change (with count)" },
            HelpEntry { key: "u", description: "Undo last change" },
            HelpEntry { key: "Ctrl+R", description: "Redo last undone change" },
            HelpEntry { key: "Ctrl+W", description: "Toggle between Hex and ASCII panel" },
        ],
    },
    HelpSection {
        id: "visual",
        title: "Visual Mode",
        entries: &[
            HelpEntry { key: "v", description: "Enter Visual mode (start selection at cursor)" },
            HelpEntry { key: "h/j/k/l", description: "Extend selection by moving cursor" },
            HelpEntry { key: "0 / $", description: "Extend selection to row start/end" },
            HelpEntry { key: "G", description: "Extend selection to end of file" },
            HelpEntry { key: "y", description: "Yank (copy) selected bytes" },
            HelpEntry { key: "d / x", description: "Cut (delete) selected bytes into yank buffer" },
            HelpEntry { key: "Esc", description: "Cancel selection, return to Normal mode" },
        ],
    },
    HelpSection {
        id: "search",
        title: "Search",
        entries: &[
            HelpEntry { key: "/", description: "Enter Search mode (type pattern, Enter to start)" },
            HelpEntry { key: "Up / Down", description: "Browse search history" },
            HelpEntry { key: "n", description: "Jump to next match" },
            HelpEntry { key: "N", description: "Jump to previous match" },
            HelpEntry { key: "x:AABB", description: "Search by hex pattern (e.g. x:FF00AABB)" },
            HelpEntry { key: "x:AA??BB", description: "Wildcard bytes (?? matches any byte)" },
            HelpEntry { key: "plain text", description: "Search by ASCII text (default)" },
            HelpEntry { key: "Esc", description: "Cancel running async search" },
            HelpEntry { key: "", description: "Progress bar shown during async search" },
        ],
    },
    HelpSection {
        id: "commands",
        title: "Commands",
        entries: &[
            HelpEntry { key: ":w", description: "Save current file" },
            HelpEntry { key: ":w <path>", description: "Save to a new file path" },
            HelpEntry { key: ":w!", description: "Force save (ignore warnings)" },
            HelpEntry { key: ":q", description: "Quit (fails if unsaved changes)" },
            HelpEntry { key: ":q!", description: "Force quit without saving" },
            HelpEntry { key: ":wq", description: "Save and quit" },
            HelpEntry { key: ":goto OFFSET", description: "Jump to byte offset (hex: 0xFF, decimal: 255)" },
            HelpEntry { key: ":s/pattern/replace", description: "Replace next match of pattern" },
            HelpEntry { key: ":%s/old/new/g", description: "Replace all occurrences globally" },
            HelpEntry { key: ":frame len=N", description: "Set fixed-length frame mode" },
            HelpEntry { key: ":frame sync=PATTERN", description: "Set sync-word frame mode" },
            HelpEntry { key: ":frame off", description: "Disable frame mode" },
            HelpEntry { key: ":import FILE", description: "Import hex text file" },
            HelpEntry { key: ":export FILE", description: "Export current buffer as hex text file" },
            HelpEntry { key: ":help [topic]", description: "Open help (optional topic: overview, navigation, etc.)" },
            HelpEntry { key: "Up / Down", description: "Browse command history" },
        ],
    },
    HelpSection {
        id: "types",
        title: "Type Inspector",
        entries: &[
            HelpEntry { key: "t", description: "Open type inspector panel (decode bytes at cursor)" },
            HelpEntry { key: "h/j/k/l", description: "Move cursor while panel stays open (live update)" },
            HelpEntry { key: "e", description: "Toggle endianness (little <-> big)" },
            HelpEntry { key: "q / Esc", description: "Close type inspector panel" },
            HelpEntry { key: "", description: "Shows u8/i8 .. u64/i64, f32/f64, string and hex at cursor" },
        ],
    },
    HelpSection {
        id: "frame",
        title: "Frame Mode",
        entries: &[
            HelpEntry { key: "j / Down", description: "Navigate to next frame" },
            HelpEntry { key: "k / Up", description: "Navigate to previous frame" },
            HelpEntry { key: "h / Left", description: "Move cursor left within frame" },
            HelpEntry { key: "l / Right", description: "Move cursor right within frame" },
            HelpEntry { key: "Ctrl+Left", description: "Horizontal scroll left (frame mode)" },
            HelpEntry { key: "Ctrl+Right", description: "Horizontal scroll right (frame mode)" },
            HelpEntry { key: "0", description: "Jump to start of current frame" },
            HelpEntry { key: "$", description: "Jump to end of current frame" },
            HelpEntry { key: "gg", description: "Jump to first frame" },
            HelpEntry { key: "G", description: "Jump to last frame" },
            HelpEntry { key: "Ctrl+F / Ctrl+B", description: "Page down/up by frames" },
            HelpEntry { key: "F2", description: "Toggle frame mode (raw <-> frame view)" },
        ],
    },
];

pub fn find_section_index(topic: &str) -> Option<usize> {
    HELP_SECTIONS.iter().position(|s| s.id == topic)
}

pub fn section_start_line(index: usize) -> usize {
    // 每个 section: 1 行标题 + 1 空行 + entries 数 + 1 空行尾部
    HELP_SECTIONS[..index].iter().map(|s| s.entries.len() + 3).sum()
}
