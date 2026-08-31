/// 帧切分配置
#[derive(Clone, Debug)]
pub enum FrameConfig {
    FixedLength { length: usize },
    SyncWord { pattern: Vec<u8> },
}

/// 单帧信息
#[derive(Clone, Debug)]
pub struct Frame {
    pub offset: usize,
    pub length: usize,
}

/// 帧索引
#[derive(Clone, Debug)]
pub struct FrameIndex {
    pub frames: Vec<Frame>,
    pub config: FrameConfig,
}

/// 视图模式
#[derive(Clone, Debug)]
pub enum ViewMode {
    Raw,
    Frame,
}

impl Default for ViewMode {
    fn default() -> Self {
        ViewMode::Raw
    }
}

/// 根据配置构建帧索引
pub fn build_frame_index(data: &[u8], config: &FrameConfig) -> FrameIndex {
    let frames = match config {
        FrameConfig::FixedLength { length } => build_fixed_length_frames(data, *length),
        FrameConfig::SyncWord { pattern } => build_sync_word_frames(data, pattern),
    };

    FrameIndex {
        frames,
        config: config.clone(),
    }
}

/// 重建帧索引（编辑后调用）
pub fn rebuild_frame_index(frame_index: &mut FrameIndex, data: &[u8]) {
    frame_index.frames = match &frame_index.config {
        FrameConfig::FixedLength { length } => build_fixed_length_frames(data, *length),
        FrameConfig::SyncWord { pattern } => build_sync_word_frames(data, pattern),
    };
}

/// 批量插入后增量调整帧索引（块操作专用，避免按配置重切导致帧边界错位）：
/// 插入点所在帧长度增长，其后各帧 offset 右移。
/// `inserts` 为 (offset, len)，offset 以**插入前**数据坐标为准，各段互不重叠。
/// 插入点恰在帧尾时归属该帧（视为追加到帧末）。
pub fn adjust_for_inserts(fi: &mut FrameIndex, inserts: &[(usize, usize)]) {
    if inserts.is_empty() || fi.frames.is_empty() {
        return;
    }
    let mut sorted: Vec<(usize, usize)> = inserts.to_vec();
    sorted.sort_by_key(|&(off, _)| off);
    // 单遍扫描：帧与插入点均升序，累计 shift 一次完成 offset 平移与帧长增长，
    // 避免逐插入点 O(帧数) 查找/平移造成的 O(帧数²) 假死
    let mut new_frames: Vec<Frame> = Vec::with_capacity(fi.frames.len());
    let mut it = sorted.iter().peekable();
    let mut shift = 0usize;
    for fr in &fi.frames {
        let mut extra = 0usize;
        while let Some(&(off, len)) = it.peek().copied() {
            // 插入前坐标中 off <= 帧尾即归属本帧（帧尾插入视为追加到帧末）
            if off <= fr.offset + fr.length {
                extra += len;
                it.next();
            } else {
                break;
            }
        }
        new_frames.push(Frame {
            offset: fr.offset + shift,
            length: fr.length + extra,
        });
        shift += extra;
    }
    // 超出末帧尾的插入点（理论上已被钳制）兜底追加到末帧
    let mut tail = 0usize;
    for &(off, len) in it {
        let _ = off;
        tail += len;
    }
    if tail > 0 {
        if let Some(last) = new_frames.last_mut() {
            last.length += tail;
        }
    }
    fi.frames = new_frames;
}

/// 批量删除后增量调整帧索引（块操作专用）：
/// 删除段所在帧长度收缩，其后各帧 offset 左移。
/// `ranges` 为 (offset, len)，offset 以**删除前**数据坐标为准，各段互不重叠且位于单帧内。
pub fn adjust_for_removals(fi: &mut FrameIndex, ranges: &[(usize, usize)]) {
    if ranges.is_empty() || fi.frames.is_empty() {
        return;
    }
    let mut sorted: Vec<(usize, usize)> = ranges.to_vec();
    sorted.sort_by_key(|&(off, _)| off);
    // 单遍扫描：帧与删除段均升序（删除前坐标），累计 shift 一次完成 offset 平移与帧长收缩
    let mut new_frames: Vec<Frame> = Vec::with_capacity(fi.frames.len());
    let mut it = sorted.iter().peekable();
    let mut shift = 0usize;
    for fr in &fi.frames {
        let mut take = 0usize;
        while let Some(&(off, len)) = it.peek().copied() {
            if off < fr.offset + fr.length {
                // 钳制到本帧剩余长度内（调用方应保证各段不重叠且位于单帧内）
                let avail = fr.offset + fr.length - off - take;
                take += len.min(avail);
                it.next();
            } else {
                break;
            }
        }
        new_frames.push(Frame {
            offset: fr.offset.saturating_sub(shift),
            length: fr.length.saturating_sub(take),
        });
        shift += take;
    }
    fi.frames = new_frames;
}

/// 根据 buffer 偏移量找到所在帧的索引号
pub fn frame_at_offset(frame_index: &FrameIndex, offset: usize) -> Option<usize> {
    for (idx, frame) in frame_index.frames.iter().enumerate() {
        if offset >= frame.offset && offset < frame.offset + frame.length {
            return Some(idx);
        }
    }
    None
}

fn build_fixed_length_frames(data: &[u8], length: usize) -> Vec<Frame> {
    if data.is_empty() || length == 0 {
        return Vec::new();
    }

    let mut frames = Vec::new();
    let total = data.len();
    let count = (total + length - 1) / length;

    for i in 0..count {
        let offset = i * length;
        let frame_len = length.min(total - offset);
        frames.push(Frame { offset, length: frame_len });
    }

    frames
}

fn build_sync_word_frames(data: &[u8], pattern: &[u8]) -> Vec<Frame> {
    if data.is_empty() {
        return Vec::new();
    }

    if pattern.is_empty() {
        return vec![Frame {
            offset: 0,
            length: data.len(),
        }];
    }

    let mut matches = Vec::new();
    let pat_len = pattern.len();
    let data_len = data.len();

    if pat_len <= data_len {
        for i in 0..=data_len - pat_len {
            if &data[i..i + pat_len] == pattern {
                matches.push(i);
            }
        }
    }

    if matches.is_empty() {
        return vec![Frame {
            offset: 0,
            length: data.len(),
        }];
    }

    let mut frames = Vec::new();

    if matches[0] > 0 {
        frames.push(Frame {
            offset: 0,
            length: matches[0],
        });
    }

    for i in 0..matches.len() {
        let offset = matches[i];
        let length = if i + 1 < matches.len() {
            matches[i + 1] - matches[i]
        } else {
            data_len - matches[i]
        };
        frames.push(Frame { offset, length });
    }

    frames
}
