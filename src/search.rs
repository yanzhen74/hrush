use anyhow::{Result, bail};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};

use crate::app::App;
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

pub struct SearchProgress {
    pub scanned: usize,
    pub total: usize,
    pub matches_found: usize,
    pub done: bool,
    pub cancelled: bool,
}

pub struct SearchState {
    pub pattern: Option<SearchPattern>,
    pub matches: Vec<usize>,
    pub current_match: Option<usize>,
    // 异步搜索字段
    pub progress: Arc<Mutex<SearchProgress>>,
    pub cancel_flag: Arc<AtomicBool>,
    pub search_handle: Option<JoinHandle<Vec<usize>>>,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            pattern: None,
            matches: Vec::new(),
            current_match: None,
            progress: Arc::new(Mutex::new(SearchProgress {
                scanned: 0,
                total: 0,
                matches_found: 0,
                done: false,
                cancelled: false,
            })),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            search_handle: None,
        }
    }

    /// 启动后台线程搜索
    pub fn start_search(&mut self, data: Vec<u8>, pattern: SearchPattern) {
        self.clear();

        let pat_len = pattern.len();
        if pat_len == 0 || pat_len > data.len() {
            self.pattern = Some(pattern);
            return;
        }

        self.pattern = Some(pattern.clone());

        let pat_bytes: Vec<u8> = pattern.as_bytes().to_vec();
        let total = if data.len() >= pat_len {
            data.len() - pat_len + 1
        } else {
            0
        };

        // 为本次搜索创建全新的进度与取消标志
        self.progress = Arc::new(Mutex::new(SearchProgress {
            scanned: 0,
            total,
            matches_found: 0,
            done: false,
            cancelled: false,
        }));
        self.cancel_flag = Arc::new(AtomicBool::new(false));

        let progress = Arc::clone(&self.progress);
        let cancel_flag = Arc::clone(&self.cancel_flag);

        let handle = thread::spawn(move || {
            let mut matches = Vec::new();
            let data_len = data.len();

            for i in 0..=(data_len - pat_len) {
                // 检查取消标志
                if cancel_flag.load(Ordering::SeqCst) {
                    let mut p = progress.lock().unwrap();
                    p.cancelled = true;
                    p.done = true;
                    return matches;
                }

                // 线性匹配
                if data[i..].starts_with(&pat_bytes) {
                    matches.push(i);
                }

                // 每 4096 字节更新一次进度
                if i % 4096 == 0 {
                    let mut p = progress.lock().unwrap();
                    p.scanned = i + 1;
                    p.matches_found = matches.len();
                }
            }

            // 最终更新进度
            {
                let mut p = progress.lock().unwrap();
                p.scanned = total;
                p.matches_found = matches.len();
                p.done = true;
            }

            matches
        });

        self.search_handle = Some(handle);
    }

    /// 检查线程是否完成，收集结果到 self.matches
    /// 返回 true 表示搜索刚刚完成（本次调用收集了结果）
    pub fn poll_result(&mut self) -> bool {
        if let Some(handle) = self.search_handle.take() {
            if handle.is_finished() {
                match handle.join() {
                    Ok(result_matches) => {
                        self.matches = result_matches;
                    }
                    Err(_) => {
                        self.matches.clear();
                    }
                }
                return true;
            } else {
                // 线程还没完成，放回 handle
                self.search_handle = Some(handle);
            }
        }
        false
    }

    /// 设置取消标志
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    /// 判断当前是否有搜索在执行
    pub fn is_searching(&self) -> bool {
        if let Some(ref handle) = self.search_handle {
            !handle.is_finished()
        } else {
            false
        }
    }

    /// 获取第一个匹配偏移
    pub fn first_match(&self) -> Option<usize> {
        self.matches.first().copied()
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
        // 如果有正在进行的搜索，先取消
        if self.is_searching() {
            self.cancel_flag.store(true, Ordering::SeqCst);
        }
        // 等待线程结束（非阻塞地尝试 join）
        if let Some(handle) = self.search_handle.take() {
            // 如果线程已完成则 join 丢弃结果，否则放弃 handle 让线程自行结束
            if handle.is_finished() {
                let _ = handle.join();
            }
            // 如果线程未完成，handle 被 drop 会导致 detach，
            // 但 cancel_flag 已设置，线程很快会自行退出
        }

        self.pattern = None;
        self.matches.clear();
        self.current_match = None;

        // 重置进度
        {
            let mut progress = self.progress.lock().unwrap();
            progress.scanned = 0;
            progress.total = 0;
            progress.matches_found = 0;
            progress.done = false;
            progress.cancelled = false;
        }
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
