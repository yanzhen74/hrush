use anyhow::{Result, bail};
use crate::app::App;
use crate::buffer::Buffer;
use crate::editor;

#[derive(Clone, Debug)]
pub enum SearchPattern {
    Hex(Vec<u8>),
    Ascii(Vec<u8>),
}

impl SearchPattern {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            SearchPattern::Hex(b) => b,
            SearchPattern::Ascii(b) => b,
        }
    }

    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct SearchState {
    pub pattern: Option<SearchPattern>,
    pub matches: Vec<usize>,
    pub current_match: Option<usize>,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            pattern: None,
            matches: Vec::new(),
            current_match: None,
        }
    }

    pub fn search(&mut self, buffer: &Buffer, pattern: SearchPattern) {
        self.clear();
        let pat_len = pattern.len();
        if pat_len == 0 {
            self.pattern = Some(pattern);
            return;
        }

        let pat_bytes: Vec<u8> = pattern.as_bytes().to_vec();
        let buf_len = buffer.len();

        if pat_len > buf_len {
            self.pattern = Some(pattern);
            return;
        }

        for i in 0..=buf_len - pat_len {
            let window = buffer.get_range(i, pat_len);
            if window == pat_bytes.as_slice() {
                self.matches.push(i);
            }
        }

        self.pattern = Some(pattern);
    }

    pub fn next_match(&mut self, cursor: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }

        for (idx, &offset) in self.matches.iter().enumerate() {
            if offset >= cursor {
                self.current_match = Some(idx);
                return Some(offset);
            }
        }

        self.current_match = Some(0);
        Some(self.matches[0])
    }

    pub fn prev_match(&mut self, cursor: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }

        for (idx, &offset) in self.matches.iter().enumerate().rev() {
            if offset <= cursor {
                self.current_match = Some(idx);
                return Some(offset);
            }
        }

        let last = self.matches.len() - 1;
        self.current_match = Some(last);
        Some(self.matches[last])
    }

    pub fn clear(&mut self) {
        self.pattern = None;
        self.matches.clear();
        self.current_match = None;
    }

    pub fn current_match_offset(&self) -> Option<usize> {
        self.current_match.and_then(|idx| self.matches.get(idx).copied())
    }

    pub fn current_match_len(&self) -> usize {
        self.pattern.as_ref().map(|p| p.len()).unwrap_or(0)
    }

    /// 检查指定偏移是否处于任意匹配范围内
    pub fn is_match_byte(&self, offset: usize) -> bool {
        if self.matches.is_empty() || self.pattern.is_none() {
            return false;
        }
        let len = self.current_match_len();
        self.matches.iter().any(|&start| offset >= start && offset < start + len)
    }

    /// 检查指定偏移是否处于当前选中匹配范围内
    pub fn is_current_match_byte(&self, offset: usize) -> bool {
        if let Some(cur_idx) = self.current_match {
            if let Some(&cur_off) = self.matches.get(cur_idx) {
                let len = self.current_match_len();
                return offset >= cur_off && offset < cur_off + len;
            }
        }
        false
    }
}

/// 替换当前匹配
pub fn replace_current(app: &mut App, new_bytes: &[u8]) -> Result<()> {
    let (start, old_len) = {
        let state = &app.search_state;
        let start = state.current_match_offset()
            .ok_or_else(|| anyhow::anyhow!("No current match"))?;
        let old_len = state.current_match_len();
        (start, old_len)
    };

    if old_len == 0 {
        bail!("Empty match");
    }

    let new_len = new_bytes.len();

    app.undo_manager.begin_group("replace current");

    if new_len == old_len {
        for i in 0..new_len {
            editor::set_byte(app, start + i, new_bytes[i]);
        }
    } else if new_len < old_len {
        for i in 0..new_len {
            editor::set_byte(app, start + i, new_bytes[i]);
        }
        for i in (new_len..old_len).rev() {
            editor::remove_byte(app, start + i);
        }
    } else {
        for i in 0..old_len {
            editor::set_byte(app, start + i, new_bytes[i]);
        }
        for i in old_len..new_len {
            editor::insert_byte(app, start + i, new_bytes[i]);
        }
    }

    app.undo_manager.end_group();
    app.search_state.clear();

    Ok(())
}

/// 全局替换
pub fn replace_all(app: &mut App, old: &SearchPattern, new_bytes: &[u8]) -> Result<()> {
    let old_len = old.len();
    if old_len == 0 {
        bail!("Empty search pattern");
    }

    let mut matches = Vec::new();
    let buf_len = app.buffer.len();
    let pat = old.as_bytes();

    if old_len <= buf_len {
        for i in 0..=buf_len - old_len {
            let window = app.buffer.get_range(i, old_len);
            if window == pat {
                matches.push(i);
            }
        }
    }

    if matches.is_empty() {
        bail!("Pattern not found");
    }

    let new_len = new_bytes.len();

    app.undo_manager.begin_group("replace all");

    for &start in matches.iter().rev() {
        if new_len == old_len {
            for i in 0..new_len {
                editor::set_byte(app, start + i, new_bytes[i]);
            }
        } else if new_len < old_len {
            for i in 0..new_len {
                editor::set_byte(app, start + i, new_bytes[i]);
            }
            for i in (new_len..old_len).rev() {
                editor::remove_byte(app, start + i);
            }
        } else {
            for i in 0..old_len {
                editor::set_byte(app, start + i, new_bytes[i]);
            }
            for i in old_len..new_len {
                editor::insert_byte(app, start + i, new_bytes[i]);
            }
        }
    }

    app.undo_manager.end_group();
    app.search_state.clear();

    Ok(())
}

/// 解析搜索/替换文本
/// 以 `x:` 开头为 hex 模式（如 `x:AABB`），否则为 ASCII
pub fn parse_pattern(input: &str) -> Result<SearchPattern> {
    if input.starts_with("x:") || input.starts_with("X:") {
        let hex_str = &input[2..];
        if hex_str.is_empty() {
            bail!("Empty hex pattern");
        }
        let cleaned: String = hex_str.chars().filter(|c| !c.is_whitespace()).collect();
        if cleaned.len() % 2 != 0 {
            bail!("Hex pattern must have even number of digits");
        }
        let mut bytes = Vec::with_capacity(cleaned.len() / 2);
        for i in (0..cleaned.len()).step_by(2) {
            let byte = u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex: {}", e))?;
            bytes.push(byte);
        }
        Ok(SearchPattern::Hex(bytes))
    } else {
        Ok(SearchPattern::Ascii(input.as_bytes().to_vec()))
    }
}

/// 解析替换内容，逻辑与 parse_pattern 相同，但直接返回字节
pub fn parse_replacement(input: &str) -> Result<Vec<u8>> {
    parse_pattern(input).map(|pat| pat.as_bytes().to_vec())
}
