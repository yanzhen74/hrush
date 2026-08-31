use crate::app::App;
use crate::undo::{ActionType, EditAction, UndoGroup};

/// 编辑后根据需要重建帧索引。
/// 常规编辑不再调用本函数（帧模式下改为增量调整，见 adjust_frames_for_*），
/// 保留作为全量重建入口（如手动重新扫描场景）。
#[allow(dead_code)]
fn sync_frame_index(app: &mut App) {
    if app.is_frame_mode() {
        if let Some(ref mut frame_index) = app.frame_index {
            crate::frame::rebuild_frame_index(frame_index, app.buffer.data());
        }
    }
}

/// 帧模式下批量插入后增量调整帧索引（插入帧增长、后续帧右移，保持对齐）
fn adjust_frames_for_inserts(app: &mut App, inserts: &[(usize, usize)]) {
    if app.is_frame_mode() {
        if let Some(ref mut frame_index) = app.frame_index {
            crate::frame::adjust_for_inserts(frame_index, inserts);
        }
    }
}

/// 帧模式下批量删除后增量调整帧索引（删除帧收缩、后续帧左移，保持对齐）
fn adjust_frames_for_removals(app: &mut App, ranges: &[(usize, usize)]) {
    if app.is_frame_mode() {
        if let Some(ref mut frame_index) = app.frame_index {
            crate::frame::adjust_for_removals(frame_index, ranges);
        }
    }
}

/// 覆盖字节，记录 undo
pub fn set_byte(app: &mut App, offset: usize, value: u8) {
    if let Some(old) = app.buffer.get_byte(offset) {
        if old != value {
            let action = EditAction::set_byte(offset, old, value);
            app.buffer.set_byte(offset, value);
            app.undo_manager.record(action);
        }
    }
    // 覆盖不改变长度，帧索引无需变动
}

/// 插入字节，记录 undo
pub fn insert_byte(app: &mut App, offset: usize, value: u8) {
    let action = EditAction::insert_byte(offset, value);
    app.buffer.insert_byte(offset, value);
    app.undo_manager.record(action);
    adjust_frames_for_inserts(app, &[(offset, 1)]);
}

/// 删除字节，记录 undo
pub fn remove_byte(app: &mut App, offset: usize) {
    if let Some(old) = app.buffer.remove_byte(offset) {
        let action = EditAction::remove_byte(offset, old);
        app.undo_manager.record(action);
        adjust_frames_for_removals(app, &[(offset, 1)]);
    }
}

/// 批量插入字节，记录 undo
pub fn insert_bytes(app: &mut App, offset: usize, bytes: &[u8]) {
    let action = EditAction {
        offset,
        old_bytes: vec![],
        new_bytes: bytes.to_vec(),
        action_type: ActionType::InsertBytes,
    };
    app.buffer.insert_bytes(offset, bytes);
    app.undo_manager.record(action);
    adjust_frames_for_inserts(app, &[(offset, bytes.len())]);
}

/// 批量插入多段字节（缓冲区一次完成，避免逐段插入的 O(n²) 开销），
/// 逐段记录 undo（按传入顺序），保持与逐段插入相同的撤销/重做语义
pub fn insert_bytes_batch(app: &mut App, inserts: &[(usize, Vec<u8>)]) {
    if inserts.is_empty() {
        return;
    }
    let refs: Vec<(usize, &[u8])> = inserts.iter().map(|(o, b)| (*o, b.as_slice())).collect();
    app.buffer.insert_bytes_batch(&refs);
    for (offset, bytes) in inserts {
        let action = EditAction {
            offset: *offset,
            old_bytes: vec![],
            new_bytes: bytes.clone(),
            action_type: ActionType::InsertBytes,
        };
        app.undo_manager.record(action);
    }
    // 帧模式下增量调整帧长（如 L24→L25），而非按配置重切导致帧边界错位
    let lens: Vec<(usize, usize)> = inserts.iter().map(|(o, b)| (*o, b.len())).collect();
    adjust_frames_for_inserts(app, &lens);
}

/// 批量删除字节，记录 undo
pub fn remove_range(app: &mut App, offset: usize, len: usize) -> Vec<u8> {
    let removed = app.buffer.remove_range(offset, len);
    let removed_len = removed.len();
    let action = EditAction {
        offset,
        old_bytes: removed.clone(),
        new_bytes: vec![],
        action_type: ActionType::RemoveBytes,
    };
    app.undo_manager.record(action);
    adjust_frames_for_removals(app, &[(offset, removed_len)]);
    removed
}

/// 撤销整组的批量快速路径（避免块粘贴/块删除等大组逐动作应用的 O(n²) 假死）。
/// 仅当组内动作全为同一批量类型且偏移模式可证明等价时返回 true，
/// 否则返回 false 回退逐动作处理。
fn try_undo_batch(app: &mut App, group: &UndoGroup) -> bool {
    let actions = &group.actions;
    if actions.is_empty() {
        return false;
    }
    let all_insert = actions.iter().all(|a| {
        matches!(a.action_type, ActionType::InsertByte | ActionType::InsertBytes)
    });
    let all_remove = actions.iter().all(|a| {
        matches!(a.action_type, ActionType::RemoveByte | ActionType::RemoveBytes)
    });
    if !all_insert && !all_remove {
        return false;
    }
    // 撤销按记录逆序应用；要求记录顺序偏移严格降序（逆序即升序），
    // 批量换算才与逐段等价；相同偏移等非单调形状回退逐段处理。
    // 块插入会话组形状：[最低偏移的键入段, 其余严格降序]（键入段先应用，
    // 其余段坐标已含键入位移且均在其上方，不位移键入段）
    let strict_desc = actions.windows(2).all(|w| w[0].offset > w[1].offset);
    let session_shape = all_insert
        && actions.len() >= 2
        && actions[1..].windows(2).all(|w| w[0].offset > w[1].offset)
        && actions[0].offset < actions[1..].iter().map(|a| a.offset).min().unwrap_or(0);
    if !strict_desc && !session_shape {
        return false;
    }

    if all_insert {
        // 逐段撤销插入 = 按记录偏移升序删；换算到当前（全部已插入）坐标：
        // 第 k 段实际位置 = 记录偏移 + 之前各段长度之和（上方插入使其右移）
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(actions.len());
        let mut prefix = 0usize;
        if session_shape && !strict_desc {
            // 会话形状：其余段逆序换算（offset + 逆序前缀和），键入段不受其后插入位移，直接用记录偏移
            for action in actions[1..].iter().rev() {
                ranges.push((action.offset + prefix, action.new_bytes.len()));
                prefix += action.new_bytes.len();
            }
            ranges.push((actions[0].offset, actions[0].new_bytes.len()));
        } else {
            for action in actions.iter().rev() {
                ranges.push((action.offset + prefix, action.new_bytes.len()));
                prefix += action.new_bytes.len();
            }
        }
        app.buffer.remove_ranges_batch(&ranges);
        adjust_frames_for_removals(app, &ranges);
    } else {
        // 撤销删除 = 重新插入；记录偏移为删除前坐标，需换算到当前（已收缩）数据坐标：
        // 第 k 段插入位置 = 记录偏移 - 之前各段长度之和（更小偏移的段尚未回到数据中）。
        // 先校验各段在原始坐标中不重叠（块删除选区天然满足），否则回退逐段。
        let rev: Vec<&EditAction> = actions.iter().rev().collect();
        if !rev.windows(2).all(|w| w[0].offset + w[0].old_bytes.len() <= w[1].offset) {
            return false;
        }
        let mut inserts: Vec<(usize, &[u8])> = Vec::with_capacity(actions.len());
        let mut prefix = 0usize;
        for action in rev {
            inserts.push((action.offset - prefix, action.old_bytes.as_slice()));
            prefix += action.old_bytes.len();
        }
        app.buffer.insert_bytes_batch(&inserts);
        let lens: Vec<(usize, usize)> = inserts.iter().map(|&(o, s)| (o, s.len())).collect();
        adjust_frames_for_inserts(app, &lens);
    }
    // 与逐段路径一致：光标停在最后处理（即最先记录）的动作偏移处
    app.cursor_offset = actions[0].offset;
    true
}

/// 重做整组的批量快速路径（同 try_undo_batch 的安全约束）
fn try_redo_batch(app: &mut App, group: &UndoGroup) -> bool {
    let actions = &group.actions;
    if actions.is_empty() {
        return false;
    }
    let all_insert = actions.iter().all(|a| {
        matches!(a.action_type, ActionType::InsertByte | ActionType::InsertBytes)
    });
    let all_remove = actions.iter().all(|a| {
        matches!(a.action_type, ActionType::RemoveByte | ActionType::RemoveBytes)
    });
    if !all_insert && !all_remove {
        return false;
    }
    // 重做按记录顺序应用；要求记录偏移严格降序（自底向上记录，偏移 = 原始坐标；
    // 相同偏移的边界情形回退逐段）。
    // 块插入会话组形状同 try_undo_batch：[最低偏移的键入段, 其余严格降序]
    let strict_desc = actions.windows(2).all(|w| w[0].offset > w[1].offset);
    let session_shape = all_insert
        && actions.len() >= 2
        && actions[1..].windows(2).all(|w| w[0].offset > w[1].offset)
        && actions[0].offset < actions[1..].iter().map(|a| a.offset).min().unwrap_or(0);
    if !strict_desc && !session_shape {
        return false;
    }

    if all_insert {
        // 记录偏移即原始坐标，批量插入一次完成（与逐段自底向上插入等价）
        let mut inserts: Vec<(usize, &[u8])> = Vec::with_capacity(actions.len());
        if session_shape && !strict_desc {
            // 会话形状：撤销已移除全部插入，其余段记录偏移含键入段位移，
            // 需减去键入段长度换算回无插入坐标；不满足安全条件则回退逐段
            let len0 = actions[0].new_bytes.len();
            inserts.push((actions[0].offset, actions[0].new_bytes.as_slice()));
            for action in actions[1..].iter() {
                if action.offset < actions[0].offset + len0
                    || action.offset - len0 == actions[0].offset
                {
                    return false;
                }
                inserts.push((action.offset - len0, action.new_bytes.as_slice()));
            }
        } else {
            for action in actions.iter() {
                inserts.push((action.offset, action.new_bytes.as_slice()));
            }
        }
        app.buffer.insert_bytes_batch(&inserts);
        let lens: Vec<(usize, usize)> = inserts.iter().map(|&(o, s)| (o, s.len())).collect();
        adjust_frames_for_inserts(app, &lens);
        // 光标 = 最后应用段（最低偏移段）插入后的末尾（批量升序应用，其余段均在其上方）
        let last = &actions[actions.len() - 1];
        app.cursor_offset = last.offset + last.new_bytes.len();
    } else {
        // 记录偏移为删除前坐标且降序（高偏移先删不漂移），等价于一次批量删除；
        // 额外校验各段不重叠，否则回退
        let mut ranges: Vec<(usize, usize)> = actions.iter()
            .map(|a| (a.offset, a.old_bytes.len()))
            .collect();
        ranges.sort_by_key(|&(off, _)| off);
        let disjoint = ranges.windows(2).all(|w| w[0].0 + w[0].1 <= w[1].0);
        if !disjoint {
            return false;
        }
        app.buffer.remove_ranges_batch(&ranges);
        adjust_frames_for_removals(app, &ranges);
        let last = &actions[actions.len() - 1];
        app.cursor_offset = last.offset;
    }
    true
}

/// 批量删除多段字节（缓冲区一次完成，避免逐段删除的 O(n²) 开销），
/// 逐段记录 undo（按传入顺序，通常自高偏移到低偏移），
/// 保持与逐段删除相同的撤销语义；各段需为当前坐标且互不重叠
pub fn remove_ranges_batch(app: &mut App, ranges: &[(usize, usize)]) {
    if ranges.is_empty() {
        return;
    }
    // 先在删除前抓取各段内容作为撤销依据（各段互不重叠，顺序无关）
    let old: Vec<Vec<u8>> = ranges.iter()
        .map(|&(off, len)| app.buffer.get_range(off, len).to_vec())
        .collect();
    app.buffer.remove_ranges_batch(ranges);
    for (i, &(offset, _len)) in ranges.iter().enumerate() {
        let action = EditAction {
            offset,
            old_bytes: old[i].clone(),
            new_bytes: vec![],
            action_type: ActionType::RemoveBytes,
        };
        app.undo_manager.record(action);
    }
    // 帧模式下增量调整帧长，而非按配置重切导致帧边界错位
    adjust_frames_for_removals(app, ranges);
}

/// 执行撤销
pub fn undo(app: &mut App) {
    if let Some(group) = app.undo_manager.undo() {
        if try_undo_batch(app, &group) {
            // 帧索引已在 try_undo_batch 内增量调整，不能重切
            return;
        }
        for action in group.actions.iter().rev() {
            match action.action_type {
                ActionType::SetByte => {
                    for (i, &byte) in action.old_bytes.iter().enumerate() {
                        if app.buffer.get_byte(action.offset + i).is_some() {
                            app.buffer.set_byte(action.offset + i, byte);
                        }
                    }
                    app.cursor_offset = action.offset;
                }
                ActionType::InsertByte => {
                    for _ in 0..action.new_bytes.len() {
                        app.buffer.remove_byte(action.offset);
                    }
                    adjust_frames_for_removals(app, &[(action.offset, action.new_bytes.len())]);
                    app.cursor_offset = action.offset;
                }
                ActionType::InsertBytes => {
                    app.buffer.remove_range(action.offset, action.new_bytes.len());
                    adjust_frames_for_removals(app, &[(action.offset, action.new_bytes.len())]);
                    app.cursor_offset = action.offset;
                }
                ActionType::RemoveByte => {
                    for (i, &byte) in action.old_bytes.iter().enumerate() {
                        app.buffer.insert_byte(action.offset + i, byte);
                    }
                    adjust_frames_for_inserts(app, &[(action.offset, action.old_bytes.len())]);
                    app.cursor_offset = action.offset;
                }
                ActionType::RemoveBytes => {
                    app.buffer.insert_bytes(action.offset, &action.old_bytes);
                    adjust_frames_for_inserts(app, &[(action.offset, action.old_bytes.len())]);
                    app.cursor_offset = action.offset;
                }
            }
        }
    }
}

/// 执行重做
pub fn redo(app: &mut App) {
    if let Some(group) = app.undo_manager.redo() {
        if try_redo_batch(app, &group) {
            // 帧索引已在 try_redo_batch 内增量调整，不能重切
            return;
        }
        for action in group.actions.iter() {
            match action.action_type {
                ActionType::SetByte => {
                    for (i, &byte) in action.new_bytes.iter().enumerate() {
                        if app.buffer.get_byte(action.offset + i).is_some() {
                            app.buffer.set_byte(action.offset + i, byte);
                        }
                    }
                    app.cursor_offset = action.offset;
                }
                ActionType::InsertByte => {
                    for (i, &byte) in action.new_bytes.iter().enumerate() {
                        app.buffer.insert_byte(action.offset + i, byte);
                    }
                    adjust_frames_for_inserts(app, &[(action.offset, action.new_bytes.len())]);
                    app.cursor_offset = action.offset + action.new_bytes.len();
                }
                ActionType::InsertBytes => {
                    app.buffer.insert_bytes(action.offset, &action.new_bytes);
                    adjust_frames_for_inserts(app, &[(action.offset, action.new_bytes.len())]);
                    app.cursor_offset = action.offset + action.new_bytes.len();
                }
                ActionType::RemoveByte => {
                    for _ in 0..action.old_bytes.len() {
                        app.buffer.remove_byte(action.offset);
                    }
                    adjust_frames_for_removals(app, &[(action.offset, action.old_bytes.len())]);
                    app.cursor_offset = action.offset;
                }
                ActionType::RemoveBytes => {
                    app.buffer.remove_range(action.offset, action.old_bytes.len());
                    adjust_frames_for_removals(app, &[(action.offset, action.old_bytes.len())]);
                    app.cursor_offset = action.offset;
                }
            }
        }
    }
}
