use crate::app::App;
use crate::undo::{ActionType, EditAction};

/// 覆盖字节，记录 undo
pub fn set_byte(app: &mut App, offset: usize, value: u8) {
    if let Some(old) = app.buffer.get_byte(offset) {
        if old != value {
            let action = EditAction::set_byte(offset, old, value);
            app.buffer.set_byte(offset, value);
            app.undo_manager.record(action);
        }
    }
}

/// 插入字节，记录 undo
pub fn insert_byte(app: &mut App, offset: usize, value: u8) {
    let action = EditAction::insert_byte(offset, value);
    app.buffer.insert_byte(offset, value);
    app.undo_manager.record(action);
}

/// 删除字节，记录 undo
pub fn remove_byte(app: &mut App, offset: usize) {
    if let Some(old) = app.buffer.remove_byte(offset) {
        let action = EditAction::remove_byte(offset, old);
        app.undo_manager.record(action);
    }
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
                ActionType::InsertByte | ActionType::InsertBytes => {
                    for _ in 0..action.new_bytes.len() {
                        app.buffer.remove_byte(action.offset);
                    }
                    app.cursor_offset = action.offset;
                }
                ActionType::RemoveByte | ActionType::RemoveBytes => {
                    for (i, &byte) in action.old_bytes.iter().enumerate() {
                        app.buffer.insert_byte(action.offset + i, byte);
                    }
                    app.cursor_offset = action.offset;
                }
            }
        }
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
                ActionType::InsertByte | ActionType::InsertBytes => {
                    for (i, &byte) in action.new_bytes.iter().enumerate() {
                        app.buffer.insert_byte(action.offset + i, byte);
                    }
                    app.cursor_offset = action.offset + action.new_bytes.len();
                }
                ActionType::RemoveByte | ActionType::RemoveBytes => {
                    for _ in 0..action.old_bytes.len() {
                        app.buffer.remove_byte(action.offset);
                    }
                    app.cursor_offset = action.offset;
                }
            }
        }
    }
}
