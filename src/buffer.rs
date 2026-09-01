use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use memmap2::Mmap;

/// 超过该阈值的文件自动以 mmap 大文件模式打开（只读 + 原地覆写，
/// 不支持插入/删除），内存占用≈常驻页 + 编辑量，而非整份读入。
pub const LARGE_FILE_THRESHOLD: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSource {
    Binary(PathBuf),
    HexImport(PathBuf),
    New,
}

/// 数据存储：内存模式（全功能）或 mmap 大文件模式（只读 + 覆写层）
enum Storage {
    /// 内存模式：Arc 包装以便搜索线程零拷贝共享快照；
    /// 编辑路径通过 Arc::make_mut 写时分离：无共享时原地修改（零开销），
    /// 搜索进行中发生编辑时自动克隆，搜索线程继续持有旧快照，
    /// 语义与原先搜索前整份 to_vec 快照完全一致。
    Mem(Arc<Vec<u8>>),
    /// 大文件模式：mmap 只读基座 + 覆写层；不支持插入/删除（长度不变）。
    /// overlay 在 :w 后保留（mmap 视图不反映已写盘的修改，读取仍需它）。
    Mapped {
        mmap: Arc<Mmap>,
        overlay: BTreeMap<usize, u8>,
    },
}

pub struct Buffer {
    storage: Storage,
    modified: HashSet<usize>,
    dirty: bool,
    source: FileSource,
}

/// 异步搜索数据快照：零拷贝共享，避免整份文件复制。
/// 大文件模式额外携带覆写层副本（通常极小），保证搜索看到已编辑内容。
pub enum DataSnapshot {
    Mem(Arc<Vec<u8>>),
    Mapped {
        mmap: Arc<Mmap>,
        overlay: BTreeMap<usize, u8>,
    },
}

impl DataSnapshot {
    /// 基座字节（不含覆写层）
    pub fn base(&self) -> &[u8] {
        match self {
            Self::Mem(v) => &v[..],
            Self::Mapped { mmap, .. } => &mmap[..],
        }
    }

    /// 覆写层（仅大文件模式有值）
    #[allow(dead_code)] // 保留供外部需要覆写层的场景使用（如增量导出）
    pub fn overlay(&self) -> Option<&BTreeMap<usize, u8>> {
        match self {
            Self::Mem(_) => None,
            Self::Mapped { overlay, .. } => Some(overlay),
        }
    }
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            storage: Storage::Mem(Arc::new(Vec::new())),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::New,
        }
    }

    /// 用已有数据构造 Buffer（主要用于测试）
    #[allow(dead_code)]
    pub fn with_data(data: &[u8]) -> Self {
        Self {
            storage: Storage::Mem(Arc::new(data.to_vec())),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::New,
        }
    }

    /// 打开二进制文件：超过阈值且可 mmap 时进入大文件模式，
    /// 否则全量读入内存（保留插入/删除等全功能）。
    pub fn from_file(path: &Path) -> Result<Self> {
        let meta = fs::metadata(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        if meta.len() as usize >= LARGE_FILE_THRESHOLD {
            let file = File::open(path)
                .with_context(|| format!("Failed to open file: {}", path.display()))?;
            match unsafe { Mmap::map(&file) } {
                Ok(mmap) => {
                    return Ok(Self {
                        storage: Storage::Mapped {
                            mmap: Arc::new(mmap),
                            overlay: BTreeMap::new(),
                        },
                        modified: HashSet::new(),
                        dirty: false,
                        source: FileSource::Binary(path.to_path_buf()),
                    });
                }
                Err(_) => {
                    // mmap 失败（如文件类型不支持）时回退全量读入；
                    // 超大文件此步可能因内存不足失败，错误向上冒泡。
                }
            }
        }
        let data = fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        Ok(Self {
            storage: Storage::Mem(Arc::new(data)),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::Binary(path.to_path_buf()),
        })
    }

    pub fn from_hex_import(path: &Path) -> Result<Self> {
        let data = crate::import::parse_hex_file(path)?;
        Ok(Self {
            storage: Storage::Mem(Arc::new(data)),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::HexImport(path.to_path_buf()),
        })
    }

    /// 是否为大文件模式（不支持插入/删除）
    pub fn is_large(&self) -> bool {
        matches!(self.storage, Storage::Mapped { .. })
    }

    /// 是否支持改变文件长度的操作（插入/删除）
    pub fn can_resize(&self) -> bool {
        !self.is_large()
    }

    pub fn get_byte(&self, offset: usize) -> Option<u8> {
        match &self.storage {
            Storage::Mem(data) => data.get(offset).copied(),
            Storage::Mapped { mmap, overlay } => overlay
                .get(&offset)
                .copied()
                .or_else(|| mmap.get(offset).copied()),
        }
    }

    /// 读取字节区间。大文件模式下区间与覆写层相交时返回补丁后的副本，
    /// 其余情况（含全部内存模式）零拷贝借用。
    pub fn get_range(&self, offset: usize, len: usize) -> Cow<'_, [u8]> {
        match &self.storage {
            Storage::Mem(data) => {
                let start = offset.min(data.len());
                let end = (offset + len).min(data.len());
                Cow::Borrowed(&data[start..end])
            }
            Storage::Mapped { mmap, overlay } => {
                let start = offset.min(mmap.len());
                let end = (offset + len).min(mmap.len());
                if overlay.range(start..end).next().is_none() {
                    return Cow::Borrowed(&mmap[start..end]);
                }
                let mut v = mmap[start..end].to_vec();
                for (&off, &byte) in overlay.range(start..end) {
                    v[off - start] = byte;
                }
                Cow::Owned(v)
            }
        }
    }

    pub fn len(&self) -> usize {
        match &self.storage {
            Storage::Mem(data) => data.len(),
            Storage::Mapped { mmap, .. } => mmap.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 全量数据切片。注意：大文件模式下返回 mmap 原始数据（**不含**覆写层修改），
    /// 用于帧索引/展示等可接受原始视图的场景；覆写感知读取请用 get_byte/get_range。
    pub fn data(&self) -> &[u8] {
        match &self.storage {
            Storage::Mem(data) => &data[..],
            Storage::Mapped { mmap, .. } => &mmap[..],
        }
    }

    /// 构造搜索快照（零拷贝），供异步搜索线程持有。
    pub fn search_snapshot(&self) -> DataSnapshot {
        match &self.storage {
            Storage::Mem(data) => DataSnapshot::Mem(Arc::clone(data)),
            Storage::Mapped { mmap, overlay } => DataSnapshot::Mapped {
                mmap: Arc::clone(mmap),
                overlay: overlay.clone(),
            },
        }
    }

    /// 查找模式的所有匹配位置（覆写感知，同步，供 :s/:%s 使用）
    pub fn find_pattern_matches(&self, pat: &[Option<u8>]) -> Vec<usize> {
        match &self.storage {
            Storage::Mem(data) => crate::search::find_all_matches(data, pat, None, None),
            Storage::Mapped { mmap, overlay } => {
                crate::search::find_all_matches_overlaid(&mmap[..], overlay, pat, None, None)
            }
        }
    }

    pub fn set_byte(&mut self, offset: usize, value: u8) {
        let changed = match &mut self.storage {
            Storage::Mem(data) => {
                // 先用只读路径判断是否需要写入，避免同值写入触发 make_mut 克隆
                if data.get(offset).map_or(false, |&b| b != value) {
                    Arc::make_mut(data)[offset] = value;
                    true
                } else {
                    false
                }
            }
            Storage::Mapped { mmap, overlay } => {
                if offset >= mmap.len() {
                    false
                } else {
                    let current = overlay.get(&offset).copied().unwrap_or(mmap[offset]);
                    if current != value {
                        overlay.insert(offset, value);
                        true
                    } else {
                        false
                    }
                }
            }
        };
        if changed {
            self.modified.insert(offset);
            self.dirty = true;
        }
    }

    /// 插入单字节。大文件模式不支持插入（长度不变），静默忽略。
    pub fn insert_byte(&mut self, offset: usize, value: u8) {
        if self.is_large() {
            return;
        }
        let offset = offset.min(self.len());
        if let Storage::Mem(data) = &mut self.storage {
            Arc::make_mut(data).insert(offset, value);
        }

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

    /// 删除单字节。大文件模式不支持删除，返回 None。
    pub fn remove_byte(&mut self, offset: usize) -> Option<u8> {
        if self.is_large() || offset >= self.len() {
            return None;
        }
        let value = match &mut self.storage {
            Storage::Mem(data) => Arc::make_mut(data).remove(offset),
            Storage::Mapped { .. } => return None,
        };
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
        if self.is_large() {
            return;
        }
        let len = bytes.len();
        let offset = match &mut self.storage {
            Storage::Mem(data) => {
                let data = Arc::make_mut(data);
                let offset = offset.min(data.len());
                // 使用 splice 一次性插入
                data.splice(offset..offset, bytes.iter().copied());
                offset
            }
            Storage::Mapped { .. } => return,
        };

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
        if self.is_large() || inserts.is_empty() {
            return;
        }
        let mut sorted: Vec<(usize, &[u8])> = inserts.to_vec();
        sorted.sort_by_key(|&(off, _)| off);
        let old_len = self.len();
        let total: usize = sorted.iter().map(|(_, b)| b.len()).sum();

        // 一次遍历构造新数据（分段拷贝 + 插入点拼接）；
        // 直接替换 Arc（而非 make_mut），搜索线程持有的旧快照不受影响且不触发额外克隆
        if let Storage::Mem(data) = &mut self.storage {
            let base = &data[..];
            let mut new_data = Vec::with_capacity(old_len + total);
            let mut prev = 0usize;
            for k in 0..=sorted.len() {
                if k == sorted.len() {
                    new_data.extend_from_slice(&base[prev..]);
                    break;
                }
                let off = sorted[k].0.min(old_len);
                new_data.extend_from_slice(&base[prev..off]);
                new_data.extend_from_slice(sorted[k].1);
                prev = off;
            }
            *data = Arc::new(new_data);
        }

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

    /// 删除区间。大文件模式不支持删除，返回空。
    pub fn remove_range(&mut self, offset: usize, len: usize) -> Vec<u8> {
        if self.is_large() {
            return Vec::new();
        }
        let (removed, offset, end) = match &mut self.storage {
            Storage::Mem(data) => {
                let data = Arc::make_mut(data);
                let offset = offset.min(data.len());
                let end = (offset + len).min(data.len());
                let removed: Vec<u8> = data.drain(offset..end).collect();
                (removed, offset, end)
            }
            Storage::Mapped { .. } => return Vec::new(),
        };
        let actual_len = end - offset;

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
        if self.is_large() || ranges.is_empty() {
            return;
        }
        let old_len = self.len();
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
        if let Storage::Mem(data) = &mut self.storage {
            let base = &data[..];
            let mut new_data = Vec::with_capacity(old_len.saturating_sub(total));
            let mut prev = 0usize;
            for &(off, len) in &clipped {
                new_data.extend_from_slice(&base[prev..off]);
                prev = off + len;
            }
            new_data.extend_from_slice(&base[prev..]);
            *data = Arc::new(new_data);
        }

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
        // 先确定写盘目标（clone 避开与后续 &mut self 方法的借用冲突）
        let target = match &self.source {
            FileSource::Binary(path) => Some(path.clone()),
            FileSource::HexImport(path) => Some(Self::infer_bin_path(path)),
            FileSource::New => None,
        };
        let target = target.ok_or_else(|| {
            anyhow::anyhow!("Cannot save buffer with no file path. Use save_as instead.")
        })?;
        self.write_back(&target)?;
        self.dirty = false;
        self.modified.clear();
        // 注意：大文件模式的 overlay 保留不清（mmap 视图不反映已写盘的修改，
        // 读取仍需覆写层）；高亮由 modified 清空控制。
        Ok(())
    }

    /// 按当前存储写盘：内存模式全量写，大文件模式就地补丁写。
    fn write_back(&self, path: &Path) -> Result<()> {
        match &self.storage {
            Storage::Mem(data) => {
                fs::write(path, &data[..])
                    .with_context(|| format!("Failed to save file: {}", path.display()))?;
            }
            Storage::Mapped { overlay, .. } => {
                Self::patch_file(path, overlay)?;
            }
        }
        Ok(())
    }

    /// 就地补丁写：只回写修改过的字节（相邻脏字节合并为连续 run 后
    /// seek+write），不重写整文件，适合超大文件的少量编辑。
    fn patch_file(path: &Path, overlay: &BTreeMap<usize, u8>) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(path)
            .with_context(|| format!("Failed to open file for writing: {}", path.display()))?;
        // BTreeMap 按偏移有序：相邻脏字节合并为连续 run，减少 seek 次数
        let mut runs: Vec<(usize, Vec<u8>)> = Vec::new();
        for (&off, &byte) in overlay {
            if let Some(last) = runs.last_mut() {
                if last.0 + last.1.len() == off {
                    last.1.push(byte);
                    continue;
                }
            }
            runs.push((off, vec![byte]));
        }
        for (off, bytes) in runs {
            file.seek(SeekFrom::Start(off as u64))
                .with_context(|| format!("Failed to seek file: {}", path.display()))?;
            file.write_all(&bytes)
                .with_context(|| format!("Failed to write file: {}", path.display()))?;
        }
        file.flush()
            .with_context(|| format!("Failed to flush file: {}", path.display()))?;
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
        match &self.storage {
            Storage::Mem(data) => {
                fs::write(path, &data[..])
                    .with_context(|| format!("Failed to save file: {}", path.display()))?;
            }
            Storage::Mapped { mmap, overlay } => {
                Self::stream_patched_to(path, mmap, overlay)?;
            }
        }
        self.dirty = false;
        self.modified.clear();
        Ok(())
    }

    /// 流式合并写：按序合并 mmap 基座与覆写层写出完整补丁后数据，
    /// 不需要整份数据的副本（内存开销仅写缓冲）。
    fn stream_patched_to(path: &Path, mmap: &Mmap, overlay: &BTreeMap<usize, u8>) -> Result<()> {
        let mut file = File::create(path)
            .with_context(|| format!("Failed to create file: {}", path.display()))?;
        let data = &mmap[..];
        let mut prev = 0usize;
        for (&off, &byte) in overlay {
            file.write_all(&data[prev..off])
                .with_context(|| format!("Failed to write file: {}", path.display()))?;
            file.write_all(&[byte])
                .with_context(|| format!("Failed to write file: {}", path.display()))?;
            prev = off + 1;
        }
        file.write_all(&data[prev..])
            .with_context(|| format!("Failed to write file: {}", path.display()))?;
        file.flush()
            .with_context(|| format!("Failed to flush file: {}", path.display()))?;
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
        let snap = buf.search_snapshot();
        buf.set_byte(0, 0xFF);
        buf.insert_byte(3, 0x99);
        assert_eq!(snap.base(), &[1, 2, 3], "快照不应受后续编辑影响");
        assert_eq!(buf.data(), &[0xFF, 2, 3, 0x99]);
    }

    #[test]
    fn edits_without_sharing_are_in_place() {
        // 无共享时编辑不应发生克隆（引用计数保持 1）
        let mut buf = Buffer::with_data(&[1, 2, 3]);
        buf.set_byte(0, 0xAA);
        buf.insert_byte(1, 0xBB);
        buf.remove_byte(3);
        let strong = match &buf.storage {
            Storage::Mem(v) => Arc::strong_count(v),
            Storage::Mapped { .. } => 0,
        };
        assert_eq!(strong, 1);
        assert_eq!(buf.data(), &[0xAA, 0xBB, 2]);
    }

    #[test]
    fn large_file_mode_end_to_end() {
        use std::io::{Read, Seek};

        // 稀疏文件：超过阈值但不实际占盘（set_len 只分配逻辑长度）
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hrush_large_test_{}.bin", std::process::id()));
        {
            let f = File::create(&path).unwrap();
            f.set_len((LARGE_FILE_THRESHOLD + 1) as u64).unwrap();
        }
        let mut buf = Buffer::from_file(&path).unwrap();
        assert!(buf.is_large(), "超阈值文件应进入大文件模式");
        assert!(!buf.can_resize());

        // 覆写走覆写层，读取（含区间）反映修改后内容；含末字节边界
        buf.set_byte(100, 0xAB);
        buf.set_byte(LARGE_FILE_THRESHOLD, 0xCD);
        assert_eq!(buf.get_byte(100), Some(0xAB));
        assert_eq!(buf.get_byte(LARGE_FILE_THRESHOLD), Some(0xCD));
        assert_eq!(&buf.get_range(99, 3)[..], &[0, 0xAB, 0]);
        assert_eq!(&buf.get_range(0, 2)[..], &[0, 0]); // 与覆写层不相交：零拷贝路径
        assert!(buf.is_dirty());

        // 插入/删除无效（长度不变）
        let len_before = buf.len();
        buf.insert_byte(0, 0x11);
        assert_eq!(buf.len(), len_before);
        assert!(buf.remove_byte(0).is_none());
        assert_eq!(buf.remove_range(0, 4), Vec::<u8>::new());
        assert_eq!(buf.len(), len_before);

        // :w 就地补丁写：磁盘上仅修改处变化，其余仍为 0（稀疏）；高亮清空但读仍正确（overlay 保留）
        buf.save().unwrap();
        assert!(!buf.is_dirty());
        assert!(!buf.is_modified(100));
        assert_eq!(buf.get_byte(100), Some(0xAB), "保存后覆写层保留，读取仍见修改");
        let mut check = vec![0u8; 3];
        {
            let mut f = File::open(&path).unwrap();
            f.seek(std::io::SeekFrom::Start(99)).unwrap();
            f.read_exact(&mut check).unwrap();
        }
        assert_eq!(check, vec![0, 0xAB, 0]);

        // save_as 流式合并输出：整份内容 = 补丁后视图（含末字节边界）
        let out_path = dir.join(format!("hrush_large_out_{}.bin", std::process::id()));
        buf.save_as(&out_path).unwrap();
        {
            let f = File::open(&out_path).unwrap();
            let mmap = unsafe { Mmap::map(&f).unwrap() };
            assert_eq!(mmap.len(), len_before);
            assert_eq!(mmap[99], 0);
            assert_eq!(mmap[100], 0xAB);
            assert_eq!(mmap[LARGE_FILE_THRESHOLD], 0xCD);
        }

        std::fs::remove_file(&path).ok(); // 清理测试自建的临时文件
        std::fs::remove_file(&out_path).ok();
    }
}
