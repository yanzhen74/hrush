use crate::app::App;
use crate::undo::{ActionType, EditAction};

/// 编辑后根据需要重建帧索引
fn sync_frame_index(app: &mut App) {
    if app.is_frame_mode() {
        if let Some(ref mut frame_index) = app.frame_index {
            crate::frame::rebuild_frame_index(frame_index, app.buffer.data());
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
    sync_frame_index(app);
}

/// 插入字节，记录 undo
pub fn insert_byte(app: &mut App, offset: usize, value: u8) {
    let action = EditAction::insert_byte(offset, value);
    app.buffer.insert_byte(offset, value);
    app.undo_manager.record(action);
    sync_frame_index(app);
}

/// 删除字节，记录 undo
pub fn remove_byte(app: &mut App, offset: usize) {
    if let Some(old) = app.buffer.remove_byte(offset) {
        let action = EditAction::remove_byte(offset, old);
        app.undo_manager.record(action);
    }
    sync_frame_index(app);
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
    sync_frame_index(app);
}

/// 批量删除字节，记录 undo
pub fn remove_range(app: &mut App, offset: usize, len: usize) -> Vec<u8> {
    let removed = app.buffer.remove_range(offset, len);
    let action = EditAction {
        offset,
        old_bytes: removed.clone(),
        new_bytes: vec![],
        action_type: ActionType::RemoveBytes,
    };
    app.undo_manager.record(action);
    sync_frame_index(app);
    removed
}

/// 执行撤销
pub fn undo(app: &mut App) {
    if let Some(group) = app.undo_manager.undo() {
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
                    app.cursor_offset = action.offset;
                }
                ActionType::InsertBytes => {
                    app.buffer.remove_range(action.offset, action.new_bytes.len());
                    app.cursor_offset = action.offset;
                }
                ActionType::RemoveByte => {
                    for (i, &byte) in action.old_bytes.iter().enumerate() {
                        app.buffer.insert_byte(action.offset + i, byte);
                    }
                    app.cursor_offset = action.offset;
                }
                ActionType::RemoveBytes => {
                    app.buffer.insert_bytes(action.offset, &action.old_bytes);
                    app.cursor_offset = action.offset;
                }
            }
        }
        sync_frame_index(app);
    }
}

/// 执行重做
pub fn redo(app: &mut App) {
    if let Some(group) = app.undo_manager.redo() {
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
                    app.cursor_offset = action.offset + action.new_bytes.len();
                }
                ActionType::InsertBytes => {
                    app.buffer.insert_bytes(action.offset, &action.new_bytes);
                    app.cursor_offset = action.offset + action.new_bytes.len();
                }
                ActionType::RemoveByte => {
                    for _ in 0..action.old_bytes.len() {
                        app.buffer.remove_byte(action.offset);
                    }
                    app.cursor_offset = action.offset;
                }
                ActionType::RemoveBytes => {
                    app.buffer.remove_range(action.offset, action.old_bytes.len());
                    app.cursor_offset = action.offset;
                }
            }
        }
        sync_frame_index(app);
    }
}
