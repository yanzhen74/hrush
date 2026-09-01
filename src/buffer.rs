use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSource {
    Binary(PathBuf),
    HexImport(PathBuf),
    New,
}

pub struct Buffer {
    /// 文件字节数据。用 Arc 包装以便搜索线程零拷贝共享快照；
    /// 编辑路径通过 Arc::make_mut 写时分离：无共享时原地修改（零开销），
    /// 搜索进行中发生编辑时自动克隆，搜索线程继续持有旧快照，
    /// 语义与原先搜索前整份 to_vec 快照完全一致。
    data: Arc<Vec<u8>>,
    modified: HashSet<usize>,
    dirty: bool,
    source: FileSource,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Vec::new()),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::New,
        }
    }

    /// 用已有数据构造 Buffer（主要用于测试）
    #[allow(dead_code)]
    pub fn with_data(data: &[u8]) -> Self {
        Self {
            data: Arc::new(data.to_vec()),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::New,
        }
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let data = fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        Ok(Self {
            data: Arc::new(data),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::Binary(path.to_path_buf()),
        })
    }

    pub fn from_hex_import(path: &Path) -> Result<Self> {
        let data = crate::import::parse_hex_file(path)?;
        Ok(Self {
            data: Arc::new(data),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::HexImport(path.to_path_buf()),
        })
    }

    pub fn get_byte(&self, offset: usize) -> Option<u8> {
        self.data.get(offset).copied()
    }

    pub fn get_range(&self, offset: usize, len: usize) -> &[u8] {
        let start = offset.min(self.data.len());
        let end = (offset + len).min(self.data.len());
        &self.data[start..end]
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn data(&self) -> &[u8] {
        &self.data[..]
    }

    /// 克隆一份数据共享句柄（仅增加引用计数，零拷贝），
    /// 供异步搜索线程持有快照。
    pub fn shared_data(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.data)
    }

    pub fn set_byte(&mut self, offset: usize, value: u8) {
        // 先用只读路径判断是否需要写入，避免同值写入触发 make_mut 克隆
        if self.data.get(offset).map_or(false, |&b| b != value) {
            Arc::make_mut(&mut self.data)[offset] = value;
            self.modified.insert(offset);
            self.dirty = true;
        }
    }

    pub fn insert_byte(&mut self, offset: usize, value: u8) {
        let offset = offset.min(self.data.len());
        let data = Arc::make_mut(&mut self.data);
        data.insert(offset, value);

        // 先平移已有的 modified 索引
        let mut new_modified = HashSet::new();
        for &idx in &self.modified {
            if idx >= offset {
                new_modified.insert(idx + 1);
            } else {
                new_modified.insert(idx);
            }
        }
        // 再标记新插入的字节
        new_modified.insert(offset);
        self.modified = new_modified;

        self.dirty = true;
    }

    pub fn remove_byte(&mut self, offset: usize) -> Option<u8> {
        if offset >= self.data.len() {
            return None;
        }
        let value = Arc::make_mut(&mut self.data).remove(offset);
        // Shift modified indices
        let mut new_modified = HashSet::new();
        for &idx in &self.modified {
            if idx == offset {
                continue;
            } else if idx > offset {
                new_modified.insert(idx - 1);
            } else {
                new_modified.insert(idx);
            }
        }
        self.modified = new_modified;
        self.dirty = true;
        Some(value)
    }

    pub fn insert_bytes(&mut self, offset: usize, bytes: &[u8]) {
        let data = Arc::make_mut(&mut self.data);
        let offset = offset.min(data.len());
        let len = bytes.len();

        // 使用 splice 一次性插入
        data.splice(offset..offset, bytes.iter().copied());

        // 批量更新 modified 索引：offset 以上的全部 +len
        let mut new_modified = HashSet::new();
        for &idx in &self.modified {
            if idx >= offset {
                new_modified.insert(idx + len);
            } else {
                new_modified.insert(idx);
            }
        }
        // 标记新插入的字节为 modified
        for i in 0..len {
            new_modified.insert(offset + i);
        }
        self.modified = new_modified;
        self.dirty = true;
    }

    /// 批量插入多段字节（一次遍历完成，避免逐段插入的 O(n²) 开销）。
    /// `inserts` 为 (offset, bytes) 列表，内部按 offset 升序应用，
    /// 相同 offset 的多段按传入顺序依次拼接。
    pub fn insert_bytes_batch(&mut self, inserts: &[(usize, &[u8])]) {
        if inserts.is_empty() {
            return;
        }
        let mut sorted: Vec<(usize, &[u8])> = inserts.to_vec();
        sorted.sort_by_key(|&(off, _)| off);
        let old_len = self.data.len();
        let total: usize = sorted.iter().map(|(_, b)| b.len()).sum();

        // 一次遍历构造新数据（分段拷贝 + 插入点拼接）；
        // 直接替换 Arc（而非 make_mut），搜索线程持有的旧快照不受影响且不触发额外克隆
        let mut new_data = Vec::with_capacity(old_len + total);
        let mut prev = 0usize;
        for k in 0..=sorted.len() {
            if k == sorted.len() {
                new_data.extend_from_slice(&self.data[prev..]);
                break;
            }
            let off = sorted[k].0.min(old_len);
            new_data.extend_from_slice(&self.data[prev..off]);
            new_data.extend_from_slice(sorted[k].1);
            prev = off;
        }
        self.data = Arc::new(new_data);

        // 一次遍历重映射 modified 索引（双指针，避免逐段重建集合）
        let mut sorted_idx: Vec<usize> = self.modified.iter().copied().collect();
        sorted_idx.sort_unstable();
        let mut new_modified = HashSet::with_capacity(sorted_idx.len() + total);
        let mut k = 0usize;
        let mut shift = 0usize;
        for idx in sorted_idx {
            while k < sorted.len() && sorted[k].0.min(old_len) <= idx {
                shift += sorted[k].1.len();
                k += 1;
            }
            new_modified.insert(idx + shift);
        }
        // 标记新插入的字节为 modified（新坐标 = 原坐标 + 之前各段长度之和）
        let mut acc_shift = 0usize;
        for &(off, bytes) in &sorted {
            let base = off.min(old_len) + acc_shift;
            for i in 0..bytes.len() {
                new_modified.insert(base + i);
            }
            acc_shift += bytes.len();
        }
        self.modified = new_modified;
        self.dirty = true;
    }

    pub fn remove_range(&mut self, offset: usize, len: usize) -> Vec<u8> {
        let data = Arc::make_mut(&mut self.data);
        let end = (offset + len).min(data.len());
        let actual_len = end - offset;
        let removed: Vec<u8> = data.drain(offset..end).collect();

        // 批量更新 modified 索引
        let mut new_modified = HashSet::new();
        for &idx in &self.modified {
            if idx >= end {
                new_modified.insert(idx - actual_len);
            } else if idx < offset {
                new_modified.insert(idx);
            }
            // idx in [offset, end) 的被删除，不保留
        }
        self.modified = new_modified;
        self.dirty = true;
        removed
    }

    /// 批量删除多段字节（一次遍历完成，避免逐段删除的 O(n²) 开销）。
    /// 各段的 (offset, len) 均以**当前数据坐标**为准，内部按 offset 升序应用。
    pub fn remove_ranges_batch(&mut self, ranges: &[(usize, usize)]) {
        if ranges.is_empty() {
            return;
        }
        let old_len = self.data.len();
        // 钳制到缓冲区内并过滤空段（排序后合并重叠段，避免越界）
        let mut sorted: Vec<(usize, usize)> = ranges.to_vec();
        sorted.sort_by_key(|&(off, _)| off);
        let mut clipped: Vec<(usize, usize)> = Vec::new();
        for (off, len) in sorted {
            let off = off.min(old_len);
            let len = len.min(old_len - off);
            if len == 0 {
                continue;
            }
            if let Some(last) = clipped.last_mut() {
                let (lo, ll) = *last;
                if off <= lo + ll {
                    // 与上一段重叠/相邻：扩展上一段覆盖（与逐段删除略有差异，
                    // 仅作为重叠输入的兼容处理；调用方应保证各段不重叠）
                    let end = (off + len).max(lo + ll);
                    *last = (lo, end - lo);
                    continue;
                }
            }
            clipped.push((off, len));
        }
        if clipped.is_empty() {
            return;
        }

        // 一次遍历构造新数据（跳过被删段）；直接替换 Arc，避免 make_mut 额外克隆
        let total: usize = clipped.iter().map(|&(_, l)| l).sum();
        let mut new_data = Vec::with_capacity(old_len.saturating_sub(total));
        let mut prev = 0usize;
        for &(off, len) in &clipped {
            new_data.extend_from_slice(&self.data[prev..off]);
            prev = off + len;
        }
        new_data.extend_from_slice(&self.data[prev..]);
        self.data = Arc::new(new_data);

        // 一次遍历重映射 modified 索引（落在被删段内的丢弃，其后的左移）
        let mut sorted_idx: Vec<usize> = self.modified.iter().copied().collect();
        sorted_idx.sort_unstable();
        let mut new_modified = HashSet::with_capacity(sorted_idx.len());
        let mut k = 0usize;
        let mut shift = 0usize;
        for idx in sorted_idx {
            while k < clipped.len() && clipped[k].0 + clipped[k].1 <= idx {
                shift += clipped[k].1;
                k += 1;
            }
            if k < clipped.len() && idx >= clipped[k].0 {
                continue; // 落在被删段内，丢弃（k 未前进，后续索引继续对比本段）
            }
            new_modified.insert(idx - shift);
        }
        self.modified = new_modified;
        self.dirty = true;
    }

    pub fn is_modified(&self, offset: usize) -> bool {
        self.modified.contains(&offset)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn save(&mut self) -> Result<()> {
        match &self.source {
            FileSource::Binary(path) => {
                fs::write(path, &self.data[..])
                    .with_context(|| format!("Failed to save file: {}", path.display()))?;
            }
            FileSource::HexImport(path) => {
                let bin_path = Self::infer_bin_path(path);
                fs::write(&bin_path, &self.data[..])
                    .with_context(|| format!("Failed to save file: {}", bin_path.display()))?;
            }
            FileSource::New => {
                anyhow::bail!("Cannot save buffer with no file path. Use save_as instead.");
            }
        }
        self.dirty = false;
        self.modified.clear();
        Ok(())
    }

    fn infer_bin_path(path: &Path) -> PathBuf {
        if let Some(stem) = path.file_stem() {
            path.with_file_name(stem).with_extension("bin")
        } else {
            path.with_extension("bin")
        }
    }

    pub fn save_as(&mut self, path: &Path) -> Result<()> {
        fs::write(path, &self.data[..])
            .with_context(|| format!("Failed to save file: {}", path.display()))?;
        self.dirty = false;
        self.modified.clear();
        Ok(())
    }

    #[allow(dead_code)]
    pub fn source(&self) -> &FileSource {
        &self.source
    }

    pub fn set_source(&mut self, source: FileSource) {
        self.source = source;
    }

    pub fn file_name(&self) -> Option<String> {
        match &self.source {
            FileSource::Binary(path) | FileSource::HexImport(path) => {
                path.file_name().map(|n| n.to_string_lossy().to_string())
            }
            FileSource::New => None,
        }
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_snapshot_isolated_from_later_edits() {
        // 写时分离语义：共享快照后再编辑，快照保留旧数据，
        // 与原先搜索前 to_vec 快照的行为一致（搜索线程不受后续编辑影响）
        let mut buf = Buffer::with_data(&[1, 2, 3]);
        let snap = buf.shared_data();
        buf.set_byte(0, 0xFF);
        buf.insert_byte(3, 0x99);
        assert_eq!(snap.as_slice(), &[1, 2, 3], "快照不应受后续编辑影响");
        assert_eq!(buf.data(), &[0xFF, 2, 3, 0x99]);
    }

    #[test]
    fn edits_without_sharing_are_in_place() {
        // 无共享时编辑不应发生克隆（引用计数保持 1）
        let mut buf = Buffer::with_data(&[1, 2, 3]);
        buf.set_byte(0, 0xAA);
        buf.insert_byte(1, 0xBB);
        buf.remove_byte(3);
        assert_eq!(Arc::strong_count(&buf.data), 1);
        assert_eq!(buf.data(), &[0xAA, 0xBB, 2]);
    }
}
