/// 编辑操作类型
#[derive(Debug, Clone, PartialEq)]
pub enum ActionType {
    SetByte,      // 覆盖/替换字节
    InsertByte,   // 插入字节
    RemoveByte,   // 删除字节
    #[allow(dead_code)]
    InsertBytes,  // 批量插入
    #[allow(dead_code)]
    RemoveBytes,  // 批量删除
}

/// 单个编辑动作
#[derive(Debug, Clone)]
pub struct EditAction {
    pub offset: usize,
    pub old_bytes: Vec<u8>,   // 操作前的字节（用于撤销）
    pub new_bytes: Vec<u8>,   // 操作后的字节（用于重做）
    pub action_type: ActionType,
}

impl EditAction {
    /// 创建 SetByte 动作
    pub fn set_byte(offset: usize, old: u8, new: u8) -> Self {
        Self {
            offset,
            old_bytes: vec![old],
            new_bytes: vec![new],
            action_type: ActionType::SetByte,
        }
    }

    /// 创建 InsertByte 动作
    pub fn insert_byte(offset: usize, byte: u8) -> Self {
        Self {
            offset,
            old_bytes: vec![],
            new_bytes: vec![byte],
            action_type: ActionType::InsertByte,
        }
    }

    /// 创建 RemoveByte 动作
    pub fn remove_byte(offset: usize, byte: u8) -> Self {
        Self {
            offset,
            old_bytes: vec![byte],
            new_bytes: vec![],
            action_type: ActionType::RemoveByte,
        }
    }
}

/// 操作组 - 将连续的相关操作合并为一个撤销单元
#[derive(Debug, Clone)]
pub struct UndoGroup {
    pub actions: Vec<EditAction>,
    #[allow(dead_code)]
    pub description: String,  // 可选描述，如 "replace bytes"
}

impl UndoGroup {
    pub fn new(description: &str) -> Self {
        Self {
            actions: Vec::new(),
            description: description.to_string(),
        }
    }

    /// 添加一个动作到组中
    pub fn push(&mut self, action: EditAction) {
        self.actions.push(action);
    }

    /// 是否为空组
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// 撤销/重做管理器
pub struct UndoManager {
    undo_stack: Vec<UndoGroup>,   // 撤销栈
    redo_stack: Vec<UndoGroup>,   // 重做栈
    current_group: Option<UndoGroup>, // 当前正在累积的操作组
}

impl UndoManager {
    /// 创建一个新的 UndoManager
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_group: None,
        }
    }

    /// 开始一个新的操作组
    pub fn begin_group(&mut self, description: &str) {
        // 如果当前有未结束的分组，先结束它
        if let Some(group) = self.current_group.take() {
            if !group.is_empty() {
                self.undo_stack.push(group);
            }
        }
        self.current_group = Some(UndoGroup::new(description));
    }

    /// 结束当前操作组，推入 undo 栈
    pub fn end_group(&mut self) {
        if let Some(group) = self.current_group.take() {
            if !group.is_empty() {
                self.undo_stack.push(group);
            }
        }
    }

    /// 记录一个编辑动作
    /// 如果没有显式调用 begin_group，自动创建一个单操作组
    /// 同时清空 redo 栈
    pub fn record(&mut self, action: EditAction) {
        // 新操作会清空 redo 栈
        self.redo_stack.clear();

        match &mut self.current_group {
            Some(group) => {
                // 检查是否可以与组内最后一个动作合并
                if let Some(last_action) = group.actions.last() {
                    if Self::can_merge(last_action, &action) {
                        // 合并两个动作
                        let merged = Self::merge_actions(last_action, &action);
                        group.actions.pop();
                        group.push(merged);
                        return;
                    }
                }
                group.push(action);
            }
            None => {
                // 没有显式分组，自动创建一个单操作组
                let mut group = UndoGroup::new("auto");
                group.push(action);
                self.undo_stack.push(group);
            }
        }
    }

    /// 判断两个连续动作是否可以合并
    /// 条件：同类型且相邻
    fn can_merge(prev: &EditAction, next: &EditAction) -> bool {
        if prev.action_type != next.action_type {
            return false;
        }

        match prev.action_type {
            ActionType::SetByte => {
                // 连续替换相邻字节可以合并
                prev.offset + prev.new_bytes.len() == next.offset
            }
            ActionType::InsertByte | ActionType::InsertBytes => {
                // 连续插入相邻字节可以合并
                prev.offset + prev.new_bytes.len() == next.offset
            }
            ActionType::RemoveByte | ActionType::RemoveBytes => {
                // 连续删除相邻字节可以合并（注意删除位置的变化）
                prev.offset == next.offset + next.old_bytes.len()
                    || prev.offset == next.offset
            }
        }
    }

    /// 合并两个动作
    fn merge_actions(prev: &EditAction, next: &EditAction) -> EditAction {
        let mut old_bytes = prev.old_bytes.clone();
        let mut new_bytes = prev.new_bytes.clone();

        match prev.action_type {
            ActionType::SetByte | ActionType::InsertByte | ActionType::InsertBytes => {
                old_bytes.extend_from_slice(&next.old_bytes);
                new_bytes.extend_from_slice(&next.new_bytes);
                EditAction {
                    offset: prev.offset,
                    old_bytes,
                    new_bytes,
                    action_type: prev.action_type.clone(),
                }
            }
            ActionType::RemoveByte | ActionType::RemoveBytes => {
                // 删除操作：next 在 prev 之前删除的
                if next.offset + next.old_bytes.len() == prev.offset {
                    old_bytes = next.old_bytes.clone();
                    old_bytes.extend_from_slice(&prev.old_bytes);
                } else {
                    old_bytes.extend_from_slice(&next.old_bytes);
                }
                EditAction {
                    offset: next.offset.min(prev.offset),
                    old_bytes,
                    new_bytes: vec![],
                    action_type: prev.action_type.clone(),
                }
            }
        }
    }

    /// 撤销：弹出 undo 栈顶的操作组，推入 redo 栈
    pub fn undo(&mut self) -> Option<UndoGroup> {
        // 先结束当前组
        self.end_group();

        if let Some(group) = self.undo_stack.pop() {
            self.redo_stack.push(group.clone());
            Some(group)
        } else {
            None
        }
    }

    /// 重做：弹出 redo 栈顶的操作组，推入 undo 栈
    pub fn redo(&mut self) -> Option<UndoGroup> {
        if let Some(group) = self.redo_stack.pop() {
            self.undo_stack.push(group.clone());
            Some(group)
        } else {
            None
        }
    }

    /// 是否可以撤销
    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty() || self.current_group.as_ref().map_or(false, |g| !g.is_empty())
    }

    /// 是否可以重做
    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// 清空所有撤销/重做历史
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.current_group = None;
    }
}

impl Default for UndoManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_action_constructors() {
        let set = EditAction::set_byte(10, 0x00, 0xFF);
        assert_eq!(set.offset, 10);
        assert_eq!(set.old_bytes, vec![0x00]);
        assert_eq!(set.new_bytes, vec![0xFF]);
        assert_eq!(set.action_type, ActionType::SetByte);

        let insert = EditAction::insert_byte(5, 0xAB);
        assert_eq!(insert.offset, 5);
        assert_eq!(insert.old_bytes, Vec::<u8>::new());
        assert_eq!(insert.new_bytes, vec![0xAB]);
        assert_eq!(insert.action_type, ActionType::InsertByte);

        let remove = EditAction::remove_byte(3, 0xCD);
        assert_eq!(remove.offset, 3);
        assert_eq!(remove.old_bytes, vec![0xCD]);
        assert_eq!(remove.new_bytes, Vec::<u8>::new());
        assert_eq!(remove.action_type, ActionType::RemoveByte);
    }

    #[test]
    fn test_basic_undo_redo() {
        let mut manager = UndoManager::new();

        // 记录一些操作
        manager.record(EditAction::set_byte(0, 0x00, 0xFF));
        manager.record(EditAction::set_byte(1, 0x11, 0xEE));

        assert!(manager.can_undo());
        assert!(!manager.can_redo());

        // Undo 最后一个操作
        let group = manager.undo().unwrap();
        assert_eq!(group.actions.len(), 1);
        assert_eq!(group.actions[0].offset, 1);

        assert!(manager.can_undo());
        assert!(manager.can_redo());

        // Undo 第一个操作
        let group = manager.undo().unwrap();
        assert_eq!(group.actions.len(), 1);
        assert_eq!(group.actions[0].offset, 0);

        assert!(!manager.can_undo());
        assert!(manager.can_redo());

        // Redo 第一个操作
        let group = manager.redo().unwrap();
        assert_eq!(group.actions[0].offset, 0);

        assert!(manager.can_undo());
        assert!(manager.can_redo());

        // Redo 第二个操作
        let group = manager.redo().unwrap();
        assert_eq!(group.actions[0].offset, 1);

        assert!(manager.can_undo());
        assert!(!manager.can_redo());
    }

    #[test]
    fn test_redo_stack_cleared_on_new_action() {
        let mut manager = UndoManager::new();

        manager.record(EditAction::set_byte(0, 0x00, 0xFF));
        manager.record(EditAction::set_byte(1, 0x11, 0xEE));

        // Undo 两个操作
        manager.undo();
        manager.undo();
        assert!(manager.can_redo());

        // 新操作会清空 redo 栈
        manager.record(EditAction::set_byte(2, 0x22, 0xDD));
        assert!(!manager.can_redo());
        assert!(manager.can_undo());
    }

    #[test]
    fn test_explicit_group() {
        let mut manager = UndoManager::new();

        manager.begin_group("replace bytes");
        manager.record(EditAction::set_byte(0, 0x00, 0xFF));
        manager.record(EditAction::set_byte(5, 0x11, 0xEE)); // 非相邻，不合并
        manager.record(EditAction::set_byte(10, 0x22, 0xDD)); // 非相邻，不合并
        manager.end_group();

        // 三个操作应该在一个组里
        let group = manager.undo().unwrap();
        assert_eq!(group.actions.len(), 3);
        assert_eq!(group.description, "replace bytes");
    }

    #[test]
    fn test_auto_merge_adjacent_set_bytes() {
        let mut manager = UndoManager::new();

        manager.begin_group("test merge");
        manager.record(EditAction::set_byte(0, 0x00, 0xFF));
        manager.record(EditAction::set_byte(1, 0x11, 0xEE));
        manager.record(EditAction::set_byte(2, 0x22, 0xDD));
        manager.end_group();

        let group = manager.undo().unwrap();
        // 三个相邻的 set_byte 应该合并为一个动作
        assert_eq!(group.actions.len(), 1);
        assert_eq!(group.actions[0].offset, 0);
        assert_eq!(group.actions[0].old_bytes, vec![0x00, 0x11, 0x22]);
        assert_eq!(group.actions[0].new_bytes, vec![0xFF, 0xEE, 0xDD]);
    }

    #[test]
    fn test_no_merge_non_adjacent() {
        let mut manager = UndoManager::new();

        manager.begin_group("test no merge");
        manager.record(EditAction::set_byte(0, 0x00, 0xFF));
        manager.record(EditAction::set_byte(2, 0x11, 0xEE)); // 不连续，不合并
        manager.end_group();

        let group = manager.undo().unwrap();
        assert_eq!(group.actions.len(), 2);
    }

    #[test]
    fn test_no_merge_different_types() {
        let mut manager = UndoManager::new();

        manager.begin_group("test no merge");
        manager.record(EditAction::set_byte(0, 0x00, 0xFF));
        // 下一个操作不是 set_byte，偏移虽然相邻但类型不同
        manager.record(EditAction {
            offset: 1,
            old_bytes: vec![0x11],
            new_bytes: vec![0xEE],
            action_type: ActionType::InsertByte,
        });
        manager.end_group();

        let group = manager.undo().unwrap();
        assert_eq!(group.actions.len(), 2);
    }

    #[test]
    fn test_empty_undo_redo() {
        let mut manager = UndoManager::new();

        assert!(!manager.can_undo());
        assert!(!manager.can_redo());
        assert!(manager.undo().is_none());
        assert!(manager.redo().is_none());
    }

    #[test]
    fn test_clear() {
        let mut manager = UndoManager::new();

        manager.record(EditAction::set_byte(0, 0x00, 0xFF));
        manager.undo();

        assert!(manager.can_undo() || manager.can_redo());

        manager.clear();

        assert!(!manager.can_undo());
        assert!(!manager.can_redo());
        assert!(manager.undo().is_none());
        assert!(manager.redo().is_none());
    }

    #[test]
    fn test_auto_single_group_without_begin_group() {
        let mut manager = UndoManager::new();

        // 不调用 begin_group，每次 record 自动创建单操作组
        manager.record(EditAction::set_byte(0, 0x00, 0xFF));
        manager.record(EditAction::set_byte(1, 0x11, 0xEE));

        // 每个操作应该独立成一个组
        let group1 = manager.undo().unwrap();
        assert_eq!(group1.actions.len(), 1);
        assert_eq!(group1.actions[0].offset, 1);

        let group2 = manager.undo().unwrap();
        assert_eq!(group2.actions.len(), 1);
        assert_eq!(group2.actions[0].offset, 0);
    }

    #[test]
    fn test_merge_insert_bytes() {
        let mut manager = UndoManager::new();

        manager.begin_group("insert");
        manager.record(EditAction::insert_byte(0, 0xAA));
        manager.record(EditAction::insert_byte(1, 0xBB));
        manager.record(EditAction::insert_byte(2, 0xCC));
        manager.end_group();

        let group = manager.undo().unwrap();
        assert_eq!(group.actions.len(), 1);
        assert_eq!(group.actions[0].offset, 0);
        assert_eq!(group.actions[0].new_bytes, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_merge_remove_bytes() {
        let mut manager = UndoManager::new();

        manager.begin_group("remove");
        manager.record(EditAction::remove_byte(2, 0xCC));
        manager.record(EditAction::remove_byte(1, 0xBB));
        manager.record(EditAction::remove_byte(0, 0xAA));
        manager.end_group();

        let group = manager.undo().unwrap();
        assert_eq!(group.actions.len(), 1);
        assert_eq!(group.actions[0].offset, 0);
        assert_eq!(group.actions[0].old_bytes, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_undo_group_order() {
        let mut manager = UndoManager::new();

        manager.record(EditAction::set_byte(0, 0x00, 0xA0));
        manager.record(EditAction::set_byte(1, 0x11, 0xA1));
        manager.record(EditAction::set_byte(2, 0x22, 0xA2));

        // undo 应该按 LIFO 顺序返回
        let g1 = manager.undo().unwrap();
        assert_eq!(g1.actions[0].offset, 2);

        let g2 = manager.undo().unwrap();
        assert_eq!(g2.actions[0].offset, 1);

        let g3 = manager.undo().unwrap();
        assert_eq!(g3.actions[0].offset, 0);
    }

    #[test]
    fn test_redo_group_order() {
        let mut manager = UndoManager::new();

        manager.record(EditAction::set_byte(0, 0x00, 0xA0));
        manager.record(EditAction::set_byte(1, 0x11, 0xA1));

        manager.undo();
        manager.undo();

        // redo 应该按 LIFO 顺序返回（先 undo 的后 redo）
        let g1 = manager.redo().unwrap();
        assert_eq!(g1.actions[0].offset, 0);

        let g2 = manager.redo().unwrap();
        assert_eq!(g2.actions[0].offset, 1);
    }

    #[test]
    fn test_undo_then_new_action_clears_redo() {
        let mut manager = UndoManager::new();

        manager.record(EditAction::set_byte(0, 0x00, 0xA0));
        manager.record(EditAction::set_byte(1, 0x11, 0xA1));

        manager.undo();

        // 此时 redo 栈有一个操作
        assert!(manager.can_redo());

        // 新操作会清空 redo 栈
        manager.record(EditAction::set_byte(2, 0x22, 0xA2));

        assert!(!manager.can_redo());
        assert!(manager.can_undo());

        // 验证 undo 栈内容
        let g1 = manager.undo().unwrap();
        assert_eq!(g1.actions[0].offset, 2);

        let g2 = manager.undo().unwrap();
        assert_eq!(g2.actions[0].offset, 0);
    }
}
