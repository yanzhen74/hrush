use anyhow::{Result, bail};
use memchr::{memchr, memmem};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};

use crate::app::App;
use crate::buffer::DataSnapshot;
use crate::editor;

#[derive(Clone, Debug)]
pub enum SearchPattern {
    /// hex 模式：Some(b) 必须匹配，None 为通配字节（`??`）
    Hex(Vec<Option<u8>>),
    Ascii(Vec<u8>),
}

impl SearchPattern {
    /// 展开为逐字节模式：Some(b) 必须匹配，None 为通配字节
    pub fn pattern(&self) -> Vec<Option<u8>> {
        match self {
            SearchPattern::Hex(b) => b.clone(),
            SearchPattern::Ascii(b) => b.iter().map(|&x| Some(x)).collect(),
        }
    }

    /// 无通配时返回原始字节（可直接用于精确比较）
    #[allow(dead_code)] // 保留供外部快路径判断使用（如增量搜索场景）
    pub fn exact_bytes(&self) -> Option<Vec<u8>> {
        self.pattern().into_iter().collect()
    }

    pub fn len(&self) -> usize {
        match self {
            SearchPattern::Hex(b) => b.len(),
            SearchPattern::Ascii(b) => b.len(),
        }
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

    /// 启动后台线程搜索（data 由调用方通过 Buffer::search_snapshot 零拷贝构造）
    pub fn start_search(&mut self, data: DataSnapshot, pattern: SearchPattern) {
        self.clear();

        let pat_len = pattern.len();
        if pat_len == 0 || pat_len > data.base().len() {
            self.pattern = Some(pattern);
            return;
        }

        self.pattern = Some(pattern.clone());

        // 进度总量：所有可能的匹配起始位置数（由 find_all_matches 内部维护实际扫描进度）
        let total = if data.base().len() >= pat_len {
            data.base().len() - pat_len + 1
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

        let pat = pattern.pattern();

        let handle = thread::spawn(move || {
            match &data {
                DataSnapshot::Mem(_) => {
                    find_all_matches(data.base(), &pat, Some(&cancel_flag), Some(&progress))
                }
                DataSnapshot::Mapped { mmap, overlay } => find_all_matches_overlaid(
                    &mmap[..],
                    overlay,
                    &pat,
                    Some(&cancel_flag),
                    Some(&progress),
                ),
            }
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

    if !app.buffer.can_resize() && new_len != old_len {
        bail!("Large-file mode: replacement must have the same length as the pattern");
    }

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
    let pat = old.pattern();

    if !app.buffer.can_resize() && new_bytes.len() != old_len {
        bail!("Large-file mode: replacement must have the same length as the pattern");
    }

    if old_len <= buf_len {
        // 覆写感知（大文件模式看到已编辑内容），内存模式等价于原扫描
        matches = app.buffer.find_pattern_matches(&pat);
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

/// 核心扫描：返回所有匹配起始偏移（保留重叠匹配语义）
///
/// - 精确模式（无通配）：使用 `memmem::Finder`（Two-Way 算法 + SIMD）
/// - 通配模式：以第一个固定字节为锚点，用 `memchr`（SIMD）跳到候选位置后验证整个模式
/// - 全通配模式：所有位置均匹配，直接批量生成（仍支持取消与进度上报）
pub fn find_all_matches(
    data: &[u8],
    pat: &[Option<u8>],
    cancel_flag: Option<&AtomicBool>,
    progress: Option<&Mutex<SearchProgress>>,
) -> Vec<usize> {
    let mut matches = Vec::new();
    let pat_len = pat.len();
    if pat_len == 0 || data.len() < pat_len {
        if let Some(p) = progress {
            let mut p = p.lock().unwrap();
            p.matches_found = 0;
            p.done = true;
        }
        return matches;
    }
    let max_start = data.len() - pat_len;
    let total = max_start + 1;

    let fixed: Vec<(usize, u8)> = pat
        .iter()
        .enumerate()
        .filter_map(|(i, b)| b.map(|b| (i, b)))
        .collect();

    // 全通配：每个位置都匹配，批量生成并保留取消/进度支持
    if fixed.is_empty() {
        let mut pos = 0usize;
        while pos <= max_start {
            if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
                if let Some(p) = progress {
                    let mut p = p.lock().unwrap();
                    p.cancelled = true;
                    p.done = true;
                }
                return matches;
            }
            let batch_end = (pos + 65536).min(max_start + 1);
            matches.extend(pos..batch_end);
            pos = batch_end;
            if let Some(p) = progress {
                let mut p = p.lock().unwrap();
                p.scanned = pos;
                p.matches_found = matches.len();
            }
        }
        if let Some(p) = progress {
            let mut p = p.lock().unwrap();
            p.scanned = total;
            p.matches_found = matches.len();
            p.done = true;
        }
        return matches;
    }

    // 按模式特征选择 SIMD 扫描策略
    enum ScanMode<'a> {
        /// 精确字节串：Two-Way + SIMD 子串搜索（needle 生命周期绑定模式字节）
        Exact(memmem::Finder<'a>),
        /// 通配模式：锚定字节索引与值（用 memchr 跳跃后验证全模式）
        Anchored(usize, u8),
    }
    let needle: Vec<u8> = pat.iter().filter_map(|b| *b).collect();
    let anchor = fixed[0];
    let mode = if fixed.len() == pat_len {
        ScanMode::Exact(memmem::Finder::new(&needle))
    } else {
        ScanMode::Anchored(anchor.0, anchor.1)
    };

    let mut pos = 0usize;
    let mut last_reported = 0usize;

    loop {
        if pos > max_start {
            break;
        }
        if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
            if let Some(p) = progress {
                let mut p = p.lock().unwrap();
                p.cancelled = true;
                p.done = true;
            }
            return matches;
        }

        match &mode {
            ScanMode::Exact(finder) => {
                // 精确字节串：SIMD 加速子串搜索
                match finder.find(&data[pos..]) {
                    Some(rel) => {
                        matches.push(pos + rel);
                        // 从匹配位置 +1 继续，保留重叠匹配语义
                        pos = pos + rel + 1;
                    }
                    None => break,
                }
            }
            ScanMode::Anchored(anchor_idx, anchor_byte) => {
                // 通配模式：锚定字节跳跃 + 全模式验证
                let (anchor_idx, anchor_byte) = (*anchor_idx, *anchor_byte);
                let search_from = pos + anchor_idx;
                match memchr(anchor_byte, &data[search_from..]) {
                    Some(rel) => {
                        let candidate = search_from + rel - anchor_idx;
                        if candidate > max_start {
                            break;
                        }
                        if matches_at(data, candidate, pat) {
                            matches.push(candidate);
                        }
                        pos = candidate + 1;
                    }
                    None => break,
                }
            }
        }

        // 每扫描约 4096 字节更新一次进度
        if let Some(p) = progress {
            if pos >= last_reported + 4096 {
                let mut p = p.lock().unwrap();
                p.scanned = pos;
                p.matches_found = matches.len();
                last_reported = pos;
            }
        }
    }

    if let Some(p) = progress {
        let mut p = p.lock().unwrap();
        p.scanned = total;
        p.matches_found = matches.len();
        p.done = true;
    }
    matches
}

/// 判断 data 在 offset 处是否匹配模式（None 为通配字节）
pub fn matches_at(data: &[u8], offset: usize, pat: &[Option<u8>]) -> bool {
    for j in 0..pat.len() {
        if let Some(b) = pat[j] {
            if data[offset + j] != b {
                return false;
            }
        }
    }
    true
}

/// 覆写感知扫描（大文件模式）：结果与在补丁后数据上做朴素扫描完全一致（含重叠语义）。
///
/// 策略：基座先走 SIMD 快扫，再将落在“脏区扩展带”内的匹配剔除重算。
/// 一个匹配起始位置 s 会覆盖脏偏移 off 当且仅当 s ∈ [off-(pat_len-1), off]，
/// 将这些候选区间合并为不重叠的带，带内逐位置用覆写层补全字节重新验证，
/// 最后与带外的基座匹配有序合并。
pub fn find_all_matches_overlaid(
    base: &[u8],
    overlay: &BTreeMap<usize, u8>,
    pat: &[Option<u8>],
    cancel_flag: Option<&AtomicBool>,
    progress: Option<&Mutex<SearchProgress>>,
) -> Vec<usize> {
    let pat_len = pat.len();
    if overlay.is_empty() || pat_len == 0 || base.len() < pat_len {
        return find_all_matches(base, pat, cancel_flag, progress);
    }
    let max_start = base.len() - pat_len;

    // 脏区扩展带：[lo, hi) 形式的候选起始位置区间（BTreeMap 有序，合并后仍有序）
    let mut bands: Vec<(usize, usize)> = Vec::new();
    for &off in overlay.keys() {
        let lo = off.saturating_sub(pat_len - 1);
        let hi = (off + 1).min(max_start + 1);
        if lo >= hi {
            continue;
        }
        if let Some(last) = bands.last_mut() {
            if lo <= last.1 {
                last.1 = last.1.max(hi);
                continue;
            }
        }
        bands.push((lo, hi));
    }

    // 基座 SIMD 扫描（进度/取消由它上报）
    let base_matches = find_all_matches(base, pat, cancel_flag, progress);
    if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
        return base_matches; // 已取消，部分结果直接丢弃于上层清理
    }

    // 带内逐位置覆写感知重扫（带通常极小，成本可忽）
    let mut band_matches = Vec::new();
    for &(lo, hi) in &bands {
        for s in lo..hi {
            if matches_overlaid(base, overlay, s, pat) {
                band_matches.push(s);
            }
        }
    }

    // 合并：基座匹配剔除落在带内的，与带内结果有序归并（两序列均升序）
    let mut result = Vec::with_capacity(base_matches.len());
    let mut bi = 0usize;
    for &m in &base_matches {
        while bi < bands.len() && bands[bi].1 <= m {
            bi += 1;
        }
        if bi < bands.len() && m >= bands[bi].0 {
            continue; // 落在带内，用带内重扫结果代替（避免重复）
        }
        result.push(m);
    }
    let mut merged = Vec::with_capacity(result.len() + band_matches.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < result.len() && j < band_matches.len() {
        if result[i] <= band_matches[j] {
            merged.push(result[i]);
            i += 1;
        } else {
            merged.push(band_matches[j]);
            j += 1;
        }
    }
    merged.extend_from_slice(&result[i..]);
    merged.extend_from_slice(&band_matches[j..]);

    if let Some(p) = progress {
        let mut p = p.lock().unwrap();
        p.matches_found = merged.len();
    }
    merged
}

/// 覆写感知单点匹配：逐字节先查覆写层，未命中再取基座字节。
/// 调用方需保证 offset + pat.len() <= base.len()。
fn matches_overlaid(
    base: &[u8],
    overlay: &BTreeMap<usize, u8>,
    offset: usize,
    pat: &[Option<u8>],
) -> bool {
    for j in 0..pat.len() {
        if let Some(b) = pat[j] {
            let actual = overlay.get(&(offset + j)).copied().unwrap_or(base[offset + j]);
            if actual != b {
                return false;
            }
        }
    }
    true
}

/// 解析搜索/替换文本
/// 以 `x:` 开头为 hex 模式（如 `x:AABB`），否则为 ASCII；
/// hex 模式中 `??` 为通配字节（如 `x:AA??BB`）
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
            let pair = &cleaned[i..i + 2];
            if pair == "??" {
                bytes.push(None);
                continue;
            }
            if pair.contains('?') {
                bail!("Invalid wildcard: use ?? for a full byte");
            }
            let byte = u8::from_str_radix(pair, 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex: {}", e))?;
            bytes.push(Some(byte));
        }
        Ok(SearchPattern::Hex(bytes))
    } else {
        Ok(SearchPattern::Ascii(input.as_bytes().to_vec()))
    }
}

/// 解析替换内容，逻辑与 parse_pattern 相同，但直接返回字节（不允许通配）
pub fn parse_replacement(input: &str) -> Result<Vec<u8>> {
    let bytes = parse_pattern(input)?.pattern()
        .into_iter()
        .collect::<Option<Vec<u8>>>();
    bytes.ok_or_else(|| anyhow::anyhow!("Wildcards not allowed in replacement"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同步执行一次搜索并返回匹配位置（内部仍走异步线程 + poll）
    fn run_search(data: Vec<u8>, pattern: SearchPattern) -> Vec<usize> {
        let mut state = SearchState::new();
        state.start_search(DataSnapshot::Mem(Arc::new(data)), pattern);
        while !state.poll_result() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        state.matches
    }

    #[test]
    fn wildcard_matches_any_byte() {
        let pat = parse_pattern("x:41??43").unwrap();
        assert_eq!(run_search(vec![0x41, 0x00, 0x43], pat.clone()), vec![0]);
        assert_eq!(run_search(vec![0x41, 0xFF, 0x43], pat.clone()), vec![0]);
        assert!(run_search(vec![0x41, 0x00, 0x44], pat).is_empty());
    }

    #[test]
    fn full_wildcard_matches_any_bytes() {
        let pat = parse_pattern("x:????").unwrap();
        assert_eq!(run_search(vec![0x12, 0x34, 0x56], pat), vec![0, 1]);
    }

    #[test]
    fn exact_hex_behavior_unchanged() {
        let pat = parse_pattern("x:AABB").unwrap();
        assert_eq!(run_search(vec![0x00, 0xAA, 0xBB, 0xAA, 0xBB], pat), vec![1, 3]);
    }

    #[test]
    fn ascii_mode_no_wildcards() {
        let pat = parse_pattern("a?b").unwrap();
        match &pat {
            SearchPattern::Ascii(b) => assert_eq!(b, b"a?b"),
            _ => panic!("expected Ascii pattern"),
        }
        // `?` 在 ASCII 模式按字面量匹配，不通配：匹配含字面 `?` 的数据，不匹配其他字节
        assert_eq!(run_search(b"a?b".to_vec(), pat.clone()), vec![0]);
        assert!(run_search(b"axb".to_vec(), pat).is_empty());
    }

    #[test]
    fn invalid_patterns_error_without_panic() {
        assert!(parse_pattern("x:4?").is_err()); // 半字节通配
        assert!(parse_pattern("x:?4").is_err());
        assert!(parse_pattern("x:GG").is_err()); // 非法 hex
        assert!(parse_pattern("x:A").is_err()); // 奇数长度
        assert!(parse_pattern("x:").is_err()); // 空模式不搜索
    }

    #[test]
    fn replacement_rejects_wildcards() {
        assert!(parse_replacement("x:AA??BB").is_err());
        assert_eq!(parse_replacement("x:AABB").unwrap(), vec![0xAA, 0xBB]);
    }

    #[test]
    fn parse_wildcard_pattern_structure() {
        match parse_pattern("x:AA??BB").unwrap() {
            SearchPattern::Hex(b) => {
                assert_eq!(b, vec![Some(0xAA), None, Some(0xBB)]);
            }
            _ => panic!("expected Hex pattern"),
        }
    }

    #[test]
    fn whitespace_in_hex_pattern_allowed() {
        let pat = parse_pattern("x:AA ?? BB").unwrap();
        match pat {
            SearchPattern::Hex(b) => assert_eq!(b, vec![Some(0xAA), None, Some(0xBB)]),
            _ => panic!("expected Hex pattern"),
        }
    }

    #[test]
    fn overlapping_matches_preserved() {
        // 精确模式：重叠匹配不能丢失（SIMD 快路径）
        // AA AA AA AA 中搜 x:AAAA：候选位置 0/1/2 均匹配（与旧朴素扫描语义一致）
        let pat = parse_pattern("x:AAAA").unwrap();
        assert_eq!(run_search(vec![0xAA; 4], pat), vec![0, 1, 2]);
        // ASCII 模式同理："aaa" 中搜 "aa" → [0, 1]
        let pat = parse_pattern("aa").unwrap();
        assert_eq!(run_search(b"aaa".to_vec(), pat), vec![0, 1]);
    }

    #[test]
    fn wildcard_anchor_finds_sparse_matches() {
        // 通配模式：锚定字节跳跃后仍能正确验证全模式（含重叠语义）
        let mut data = vec![0u8; 100_000];
        data[1000] = 0xDE;
        data[1001] = 0xAD;
        data[99_998] = 0xDE;
        data[99_999] = 0xAD;
        let pat = parse_pattern("x:??DEAD").unwrap();
        assert_eq!(run_search(data, pat), vec![999, 99_997]);
    }

    #[test]
    fn large_file_exact_search_is_fast() {
        // 64MB 数据、模式仅出现一次：SIMD 扫描应在秒内完成（旧朴素扫描在 debug 下通常远超 2 秒）
        let n = 64 * 1024 * 1024usize;
        let mut data = vec![0xABu8; n];
        let pos = n / 2;
        data[pos..pos + 4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let pat = parse_pattern("x:DEADBEEF").unwrap();

        let start = std::time::Instant::now();
        let matches = run_search(data, pat);
        let elapsed = start.elapsed();

        assert_eq!(matches, vec![pos]);
        assert!(
            elapsed.as_secs() < 2,
            "64MB 精确搜索耗时 {:?}，SIMD 快路径应远快于此",
            elapsed
        );
    }

    #[test]
    #[ignore] // 手动执行：cargo test --release search_peak_memory_report -- --ignored --nocapture
    fn search_peak_memory_report() {
        // 256MB 数据：搜索线程零拷贝共享 Arc，搜索期间峰值 RSS 应≈一份数据；
        // 优化前（to_vec 快照）约为两份。
        let data = vec![0xABu8; 256 * 1024 * 1024];
        let mut state = SearchState::new();
        state.start_search(
            DataSnapshot::Mem(Arc::new(data)),
            parse_pattern("x:DEADBEEF").unwrap(),
        );
        while !state.poll_result() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(state.matches.is_empty());
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let hwm = status.lines().find(|l| l.starts_with("VmHWM")).unwrap_or("VmHWM: n/a");
        eprintln!("搜索完成后峰值 RSS（{}）", hwm.trim());
    }

    #[test]
    fn overlaid_search_matches_naive_on_patched_data() {
        // 覆写感知扫描必须与在补丁后数据上的朴素扫描结果完全一致（含通配与重叠）
        let n = 4096usize;
        let mut base = vec![0xAAu8; n];
        base[100] = 0x00;
        base[2000] = 0x00;
        let mut overlay = BTreeMap::new();
        overlay.insert(100, 0xAA);   // 撤销一个基座空洞 → 新增匹配
        overlay.insert(2000, 0xBB);  // 破坏一个潜在匹配 → 减少匹配
        overlay.insert(3500, 0x00);  // 引入新空洞 → 影响带内多个候选位置
        let pat = parse_pattern("x:AAAA").unwrap().pattern();

        // 朴素基准：手工应用覆写后逐位置扫描（重叠语义）
        let patched: Vec<u8> = (0..n)
            .map(|i| overlay.get(&i).copied().unwrap_or(base[i]))
            .collect();
        let mut naive = Vec::new();
        for i in 0..=n - pat.len() {
            if matches_at(&patched, i, &pat) {
                naive.push(i);
            }
        }

        let got = find_all_matches_overlaid(&base, &overlay, &pat, None, None);
        assert_eq!(got, naive);

        // 空覆写层：退化为基座扫描，结果一致
        let empty = BTreeMap::new();
        let got_empty = find_all_matches_overlaid(&base, &empty, &pat, None, None);
        assert_eq!(got_empty, find_all_matches(&base, &pat, None, None));
    }

    #[test]
    fn simd_search_beats_naive_scan() {
        // 同一数据上对比旧朴素扫描与新 SIMD 扫描，结果必须一致且更快
        let n = 16 * 1024 * 1024usize;
        let mut data = vec![0x00u8; n];
        data[n - 4..].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        data[123] = 0xDE;
        data[124] = 0xAD;
        data[125] = 0xBE;
        data[126] = 0xEF;
        let pat = parse_pattern("x:DEADBEEF").unwrap().pattern();

        // 朴素扫描（优化前实现）
        let start = std::time::Instant::now();
        let mut naive_matches = Vec::new();
        for i in 0..=n - pat.len() {
            if matches_at(&data, i, &pat) {
                naive_matches.push(i);
            }
        }
        let naive_elapsed = start.elapsed();

        // SIMD 扫描（新实现）
        let start = std::time::Instant::now();
        let simd_matches = find_all_matches(&data, &pat, None, None);
        let simd_elapsed = start.elapsed();

        assert_eq!(simd_matches, naive_matches, "两种扫描结果必须一致");
        assert!(
            simd_elapsed < naive_elapsed,
            "SIMD 扫描 {:?} 应快于朴素扫描 {:?}",
            simd_elapsed,
            naive_elapsed
        );
    }
}
