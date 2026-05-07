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
