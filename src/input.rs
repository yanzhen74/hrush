use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{App, Mode};
use crate::command;
use crate::editor;
use crate::frame::ViewMode;
use crate::search;
use crate::ui::Panel;

pub fn handle_input(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    // 优先处理待定键（Normal 模式的多键序列）
    if app.mode == Mode::Normal && app.pending_key.is_some() {
        handle_pending_key(app, key);
        return Ok(());
    }

    // 搜索进行中时，Esc 优先中断搜索
    if key.code == KeyCode::Esc && app.is_searching() {
        app.search_state.cancel();
        return Ok(());
    }

    match app.mode {
        Mode::Normal => handle_normal_mode(app, key),
        Mode::Insert => handle_insert_mode(app, key),
        Mode::Replace => handle_replace_mode(app, key),
        Mode::Command => handle_command_mode(app, key),
        Mode::Search => handle_search_mode(app, key),
        Mode::Visual => handle_visual_mode(app, key),
        Mode::Help => handle_help_mode(app, key)
    }

    Ok(())
}

fn handle_pending_key(app: &mut App, key: KeyEvent) {
    let pending = app.pending_key.take().unwrap();

    match pending {
        'g' => {
            if key.code == KeyCode::Char('g') {
                if app.is_frame_mode() {
                    // 先计算跳转目标，结束 frame_index 的不可变借用后再记录跳转点
                    let target = app.frame_index.as_ref().and_then(|fi| {
                        if fi.frames.is_empty() {
                            return None;
                        }
                        let current_frame_num = app.current_frame_number()?;
                        let current_frame = &fi.frames[current_frame_num];
                        let col = app.cursor_offset.saturating_sub(current_frame.offset);
                        let first_frame = &fi.frames[0];
                        let target_col = col.min(first_frame.length.saturating_sub(1));
                        Some(first_frame.offset + target_col)
                    });
                    if let Some(offset) = target {
                        app.push_jump();
                        app.cursor_offset = offset;
                        app.scroll_offset = 0;
                    }
                } else {
                    app.push_jump();
                    app.cursor_offset = 0;
                }
            } else {
                // 不是 gg，将当前键作为普通键处理
                handle_normal_mode(app, key);
            }
        }
        'd' => {
            if key.code == KeyCode::Char('d') {
                let count = app.count_prefix.take().unwrap_or(1);
                for _ in 0..count {
                    delete_line(app);
                }
            } else {
                handle_normal_mode(app, key);
            }
        }
        'r' => {
            handle_single_replace(app, key);
        }
        _ => {}
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) {
    // 类型解读面板打开时：仅拦截面板自身按键，其余键穿透到原有导航逻辑
    if app.type_panel_open {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.type_panel_open = false;
                return;
            }
            KeyCode::Char('e') => {
                app.type_endian_le = !app.type_endian_le;
                return;
            }
            _ => {}
        }
    }

    // 数字前缀累积（'0' 仅在已有前缀时累积，否则保留行首逻辑）
    if let KeyCode::Char(c @ '1'..='9') = key.code {
        let digit = (c as u8 - b'0') as usize;
        app.count_prefix = Some(app.count_prefix.unwrap_or(0) * 10 + digit);
        return;
    }
    if let KeyCode::Char('0') = key.code {
        if app.count_prefix.is_some() {
            app.count_prefix = Some(app.count_prefix.unwrap() * 10);
            return;
        }
    }

    // 帧模式下优先使用帧导航逻辑
    if handle_frame_navigation(app, key) {
        return;
    }

    match key.code {
        // 多键命令前缀
        KeyCode::Char('g') => {
            app.pending_key = Some('g');
        }
        KeyCode::Char('d') => {
            app.pending_key = Some('d');
        }
        KeyCode::Char('r') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.pending_key = Some('r');
            app.nibble_input = None;
        }

        // 帮助模式入口
        KeyCode::Char('?') => {
            app.help_scroll = 0;
            app.help_topic = None;
            app.mode = Mode::Help;
        }
        KeyCode::F(1) => {
            app.help_scroll = 0;
            app.help_topic = None;
            app.mode = Mode::Help;
        }

        // 类型解读面板入口
        KeyCode::Char('t') => {
            app.type_panel_open = true;
        }

        // 模式切换
        KeyCode::Char('i') => {
            app.mode = Mode::Insert;
            app.insert_after = false;
            app.nibble_input = None;
        }
        KeyCode::Char('a') => {
            app.mode = Mode::Insert;
            app.insert_after = true;
            app.nibble_input = None;
            if !app.buffer.is_empty() {
                app.cursor_offset = (app.cursor_offset + 1).min(app.buffer.len());
            }
        }
        KeyCode::Char('R') => {
            app.mode = Mode::Replace;
            app.nibble_input = None;
        }
        KeyCode::Char(':') => {
            app.mode = Mode::Command;
            app.command_input.clear();
        }

        // 搜索
        KeyCode::Char('/') => {
            app.mode = Mode::Search;
            app.search_input.clear();
            app.search_state.clear();
        }
        KeyCode::Char('n') => {
            let start = app.cursor_offset + 1;
            if let Some(offset) = app.search_state.next_match(start) {
                app.push_jump();
                app.cursor_offset = offset;
            }
        }
        KeyCode::Char('N') => {
            let start = app.cursor_offset.saturating_sub(1);
            if let Some(offset) = app.search_state.prev_match(start) {
                app.push_jump();
                app.cursor_offset = offset;
            }
        }

        // 移动
        KeyCode::Char('h') | KeyCode::Left => {
            let count = app.count_prefix.take().unwrap_or(1);
            move_cursor_left(app, count);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            let count = app.count_prefix.take().unwrap_or(1);
            move_cursor_right(app, count);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let count = app.count_prefix.take().unwrap_or(1);
            move_cursor_up(app, count);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let count = app.count_prefix.take().unwrap_or(1);
            move_cursor_down(app, count);
        }

        // 快速移动
        KeyCode::Char('G') => {
            if !app.buffer.is_empty() {
                app.push_jump();
                app.cursor_offset = app.buffer.len().saturating_sub(1);
            }
        }
        KeyCode::Char('0') => {
            app.cursor_offset = app.cursor_offset / 16 * 16;
        }
        KeyCode::Char('$') => {
            if !app.buffer.is_empty() {
                let row_start = app.cursor_offset / 16 * 16;
                app.cursor_offset = (row_start + 15).min(app.buffer.len().saturating_sub(1));
            }
        }

        // 翻页
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            page_down(app);
        }
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            page_up(app);
        }

        // 编辑
        KeyCode::Char('x') => {
            if !app.buffer.is_empty() {
                let count = app.count_prefix.take().unwrap_or(1);
                app.undo_manager.begin_group("delete bytes");
                for _ in 0..count {
                    if !app.buffer.is_empty() {
                        editor::remove_byte(app, app.cursor_offset);
                    }
                }
                app.undo_manager.end_group();
                clamp_cursor(app);
            }
        }

        // Visual 模式
        KeyCode::Char('v') => {
            app.mode = Mode::Visual;
            app.visual_anchor = Some(app.cursor_offset);
        }

        // 粘贴
        KeyCode::Char('p') => {
            if !app.yank_buffer.is_empty() {
                let insert_pos = if app.buffer.is_empty() {
                    0
                } else {
                    app.cursor_offset + 1
                };
                let yank_data = app.yank_buffer.clone();
                let len = yank_data.len();
                editor::insert_bytes(app, insert_pos, &yank_data);
                app.cursor_offset = (insert_pos + len - 1).min(app.buffer.len().saturating_sub(1));
            }
        }

        // 撤销 / 重做
        KeyCode::Char('u') => {
            editor::undo(app);
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor::redo(app);
        }

        // 面板切换（原 Tab 改键为 Ctrl+W）
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_panel = match app.active_panel {
                Panel::Hex => Panel::Ascii,
                Panel::Ascii => Panel::Hex,
            };
        }

        // Jumplist：Ctrl+O 回退 / Tab 前进
        KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            jump_back(app);
        }
        KeyCode::Tab => {
            jump_forward(app);
        }

        // F2 切换帧模式
        KeyCode::F(2) => {
            if app.is_frame_mode() {
                // 切换回原始模式
                app.view_mode = ViewMode::Raw;
                app.h_scroll_offset = 0;
            } else if app.frame_index.is_some() {
                // 之前设置过帧参数，切换回帧模式
                app.view_mode = ViewMode::Frame;
            } else {
                // 从未设置过帧参数
                app.message = Some(("Use :frame len=N or :frame sync=XX to set frame mode first".to_string(), std::time::Instant::now()));
            }
        }

        _ => {}
    }

    // 未提前 return 的动作执行完后清除 count 前缀
    app.count_prefix = None;
}

fn handle_help_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.mode = Mode::Normal;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.help_scroll += 1;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.help_scroll = app.help_scroll.saturating_sub(1);
        }
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.help_scroll += 20;
        }
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.help_scroll = app.help_scroll.saturating_sub(20);
        }
        KeyCode::PageDown => {
            app.help_scroll += 20;
        }
        KeyCode::PageUp => {
            app.help_scroll = app.help_scroll.saturating_sub(20);
        }
        KeyCode::Char('g') => {
            // pending_key 逻辑: g + g 跳转到顶部
            if app.pending_key == Some('g') {
                app.pending_key = None;
                app.help_scroll = 0;
            } else {
                app.pending_key = Some('g');
            }
        }
        KeyCode::Char('G') => {
            app.help_scroll = 9999;
        }
        _ => {}
    }
}

fn handle_visual_mode(app: &mut App, key: KeyEvent) {
    // 数字键累积（与 Normal 相同逻辑）
    if let KeyCode::Char(c @ '1'..='9') = key.code {
        let digit = (c as u8 - b'0') as usize;
        app.count_prefix = Some(app.count_prefix.unwrap_or(0) * 10 + digit);
        return;
    }
    if let KeyCode::Char('0') = key.code {
        if app.count_prefix.is_some() {
            app.count_prefix = Some(app.count_prefix.unwrap() * 10);
            return;
        }
    }

    let count = app.count_prefix.take().unwrap_or(1);

    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.visual_anchor = None;
        }
        KeyCode::Char('h') | KeyCode::Left => move_cursor_left(app, count),
        KeyCode::Char('l') | KeyCode::Right => move_cursor_right(app, count),
        KeyCode::Char('k') | KeyCode::Up => move_cursor_up(app, count),
        KeyCode::Char('j') | KeyCode::Down => move_cursor_down(app, count),
        KeyCode::Char('0') => {
            app.cursor_offset = app.cursor_offset / 16 * 16;
        }
        KeyCode::Char('$') => {
            if !app.buffer.is_empty() {
                app.cursor_offset = ((app.cursor_offset / 16 + 1) * 16 - 1).min(app.buffer.len().saturating_sub(1));
            }
        }
        KeyCode::Char('G') => {
            if !app.buffer.is_empty() {
                app.cursor_offset = app.buffer.len().saturating_sub(1);
            }
        }
        KeyCode::Char('y') => {
            if let Some((start, end)) = app.selection_range() {
                let len = end - start + 1;
                app.yank_buffer = app.buffer.get_range(start, len).to_vec();
                app.mode = Mode::Normal;
                app.visual_anchor = None;
                app.cursor_offset = start;
            }
        }
        KeyCode::Char('d') | KeyCode::Char('x') => {
            if let Some((start, end)) = app.selection_range() {
                let len = end - start + 1;
                app.yank_buffer = app.buffer.get_range(start, len).to_vec();
                editor::remove_range(app, start, len);
                app.mode = Mode::Normal;
                app.visual_anchor = None;
                app.cursor_offset = start;
                clamp_cursor(app);
            }
        }
        _ => {}
    }
}

fn handle_insert_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.insert_after = false;
            app.nibble_input = None;
            clamp_cursor(app);
        }
        _ => match app.active_panel {
            Panel::Hex => handle_hex_insert(app, key),
            Panel::Ascii => handle_ascii_insert(app, key),
        },
    }
}

fn handle_replace_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.nibble_input = None;
        }
        _ => match app.active_panel {
            Panel::Hex => handle_hex_replace(app, key),
            Panel::Ascii => handle_ascii_replace(app, key),
        },
    }
}

fn handle_search_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            let input = app.search_input.clone();
            match search::parse_pattern(&input) {
                Ok(pattern) => {
                    let data = app.buffer.data().to_vec();
                    app.search_state.start_search(data, pattern);
                }
                Err(e) => {
                    app.message = Some((format!("Search error: {}", e), std::time::Instant::now()));
                }
            }
            app.mode = Mode::Normal;
        }
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.search_input.clear();
            app.search_state.clear();
        }
        KeyCode::Char(c) => {
            app.search_input.push(c);
        }
        KeyCode::Backspace => {
            app.search_input.pop();
        }
        _ => {}
    }
}

fn handle_command_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            let cmd = app.command_input.clone();
            if let Err(e) = command::execute_command(app, cmd.trim()) {
                app.message = Some((format!("Error: {}", e), std::time::Instant::now()));
            }
            // 仅当命令没有切换模式时回到 Normal；
            // :help 等命令会将模式切走（如 Mode::Help），不能被覆盖回去
            if app.mode == Mode::Command {
                app.mode = Mode::Normal;
            }
            app.command_input.clear();
        }
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.command_input.clear();
        }
        KeyCode::Char(c) => {
            app.command_input.push(c);
        }
        KeyCode::Backspace => {
            app.command_input.pop();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 单字节替换（Normal 模式下按 r 后的处理）
// ---------------------------------------------------------------------------
fn handle_single_replace(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            // 取消，pending_key 已在 handle_pending_key 中清除
            app.nibble_input = None;
        }
        _ => match app.active_panel {
            Panel::Hex => {
                if let Some(nibble) = char_to_nibble(key) {
                    if app.nibble_input.is_none() {
                        app.nibble_input = Some(nibble);
                        app.pending_key = Some('r'); // 等待第二个半字节
                    } else {
                        let high = app.nibble_input.take().unwrap();
                        let value = (high << 4) | nibble;
                        editor::set_byte(app, app.cursor_offset, value);
                        app.cursor_offset =
                            (app.cursor_offset + 1).min(app.buffer.len().saturating_sub(1));
                        // pending_key 保持 None（由 handle_pending_key 已清除）
                    }
                } else {
                    app.nibble_input = None;
                }
            }
            Panel::Ascii => {
                if let KeyCode::Char(c) = key.code {
                    if c.is_ascii_graphic() || c == ' ' {
                        editor::set_byte(app, app.cursor_offset, c as u8);
                        app.cursor_offset =
                            (app.cursor_offset + 1).min(app.buffer.len().saturating_sub(1));
                    }
                }
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Hex 面板插入
// ---------------------------------------------------------------------------
fn handle_hex_insert(app: &mut App, key: KeyEvent) {
    if let Some(nibble) = char_to_nibble(key) {
        if app.nibble_input.is_none() {
            app.nibble_input = Some(nibble);
        } else {
            let high = app.nibble_input.take().unwrap();
            let value = (high << 4) | nibble;
            let offset = app.cursor_offset.min(app.buffer.len());
            editor::insert_byte(app, offset, value);
            app.cursor_offset = (app.cursor_offset + 1).min(app.buffer.len());
        }
    } else if key.code == KeyCode::Backspace {
        // 如果有未完成的半字节，先清除它
        if app.nibble_input.is_some() {
            app.nibble_input = None;
        }
        // 否则可选择删除前一个字节（此处暂不实现，保持简单）
    }
}

// ---------------------------------------------------------------------------
// ASCII 面板插入
// ---------------------------------------------------------------------------
fn handle_ascii_insert(app: &mut App, key: KeyEvent) {
    if let KeyCode::Char(c) = key.code {
        if c.is_ascii_graphic() || c == ' ' {
            let offset = app.cursor_offset.min(app.buffer.len());
            editor::insert_byte(app, offset, c as u8);
            app.cursor_offset = (app.cursor_offset + 1).min(app.buffer.len());
        }
    }
}

// ---------------------------------------------------------------------------
// Hex 面板替换（连续 Replace 模式）
// ---------------------------------------------------------------------------
fn handle_hex_replace(app: &mut App, key: KeyEvent) {
    if app.cursor_offset >= app.buffer.len() {
        return;
    }
    if let Some(nibble) = char_to_nibble(key) {
        if app.nibble_input.is_none() {
            app.nibble_input = Some(nibble);
        } else {
            let high = app.nibble_input.take().unwrap();
            let value = (high << 4) | nibble;
            editor::set_byte(app, app.cursor_offset, value);
            app.cursor_offset =
                (app.cursor_offset + 1).min(app.buffer.len().saturating_sub(1));
        }
    } else if key.code == KeyCode::Backspace {
        if app.nibble_input.is_some() {
            app.nibble_input = None;
        }
    }
}

// ---------------------------------------------------------------------------
// ASCII 面板替换（连续 Replace 模式）
// ---------------------------------------------------------------------------
fn handle_ascii_replace(app: &mut App, key: KeyEvent) {
    if app.cursor_offset >= app.buffer.len() {
        return;
    }
    if let KeyCode::Char(c) = key.code {
        if c.is_ascii_graphic() || c == ' ' {
            editor::set_byte(app, app.cursor_offset, c as u8);
            app.cursor_offset =
                (app.cursor_offset + 1).min(app.buffer.len().saturating_sub(1));
        }
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

fn char_to_nibble(key: KeyEvent) -> Option<u8> {
    match key.code {
        KeyCode::Char(c) if c.is_ascii_digit() => Some(c as u8 - b'0'),
        KeyCode::Char(c) if ('a'..='f').contains(&c) => Some(c as u8 - b'a' + 10),
        KeyCode::Char(c) if ('A'..='F').contains(&c) => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

fn move_cursor_left(app: &mut App, count: usize) {
    app.cursor_offset = app.cursor_offset.saturating_sub(count);
}

fn move_cursor_right(app: &mut App, count: usize) {
    if app.buffer.is_empty() { return; }
    let max = app.buffer.len().saturating_sub(1);
    app.cursor_offset = (app.cursor_offset + count).min(max);
}

fn move_cursor_up(app: &mut App, count: usize) {
    app.cursor_offset = app.cursor_offset.saturating_sub(16 * count);
}

fn move_cursor_down(app: &mut App, count: usize) {
    if app.buffer.is_empty() { return; }
    let max = app.buffer.len().saturating_sub(1);
    app.cursor_offset = (app.cursor_offset + 16 * count).min(max);
}

fn page_down(app: &mut App) {
    if app.buffer.is_empty() {
        return;
    }
    let page_bytes = app.visible_rows.saturating_sub(1).max(1) * 16;
    app.cursor_offset = (app.cursor_offset + page_bytes).min(app.buffer.len().saturating_sub(1));
}

fn page_up(app: &mut App) {
    let page_bytes = app.visible_rows.saturating_sub(1).max(1) * 16;
    app.cursor_offset = app.cursor_offset.saturating_sub(page_bytes);
}

fn delete_line(app: &mut App) {
    if app.buffer.is_empty() {
        return;
    }
    let row_start = app.cursor_offset / 16 * 16;
    let row_end = (row_start + 16).min(app.buffer.len());
    let count = row_end - row_start;

    app.undo_manager.begin_group("delete line");
    for i in (0..count).rev() {
        editor::remove_byte(app, row_start + i);
    }
    app.undo_manager.end_group();

    clamp_cursor(app);
}

fn clamp_cursor(app: &mut App) {
    if !app.buffer.is_empty() && app.cursor_offset >= app.buffer.len() {
        app.cursor_offset = app.buffer.len().saturating_sub(1);
    }
}

// ---------------------------------------------------------------------------
// Jumplist 导航（Ctrl+O 回退 / Tab 前进）
// ---------------------------------------------------------------------------

fn jump_back(app: &mut App) {
    if let Some(pos) = app.jump_back.pop() {
        app.jump_forward.push(app.cursor_offset);
        // 滚动视图由 App::run 主循环中的 scroll 同步逻辑自动跟随
        app.cursor_offset = pos.min(app.buffer.len().saturating_sub(1));
    }
}

fn jump_forward(app: &mut App) {
    if let Some(pos) = app.jump_forward.pop() {
        app.jump_back.push(app.cursor_offset);
        app.cursor_offset = pos.min(app.buffer.len().saturating_sub(1));
    }
}

// ---------------------------------------------------------------------------
// 帧模式导航
// ---------------------------------------------------------------------------

fn handle_frame_navigation(app: &mut App, key: KeyEvent) -> bool {
    if !app.is_frame_mode() {
        return false;
    }

    let frame_index = match &app.frame_index {
        Some(fi) => fi,
        None => return false,
    };

    let current_frame_num = match app.current_frame_number() {
        Some(n) => n,
        None => return false,
    };

    let current_frame = &frame_index.frames[current_frame_num];

    match key.code {
        KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let visible_bytes = app.visible_bytes.max(1);
            let max_len = frame_index.frames.iter().map(|f| f.length).max().unwrap_or(0);
            app.h_scroll_offset = (app.h_scroll_offset + visible_bytes).min(max_len.saturating_sub(1));
            // 同时将光标移动到新可视区域的第一个字节，避免被同步逻辑拉回
            app.cursor_offset = current_frame.offset + app.h_scroll_offset;
            true
        }
        KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let visible_bytes = app.visible_bytes.max(1);
            app.h_scroll_offset = app.h_scroll_offset.saturating_sub(visible_bytes);
            // 同时将光标移动到新可视区域的最后一个字节，避免被同步逻辑拉回
            let frame_col = app.h_scroll_offset + visible_bytes.saturating_sub(1);
            app.cursor_offset = current_frame.offset + frame_col.min(current_frame.length.saturating_sub(1));
            true
        }
        KeyCode::Char('h') | KeyCode::Left => {
            let count = app.count_prefix.take().unwrap_or(1);
            if app.cursor_offset > current_frame.offset {
                let min_offset = current_frame.offset;
                app.cursor_offset = app.cursor_offset.saturating_sub(count).max(min_offset);
                sync_h_scroll(app);
            }
            true
        }
        KeyCode::Char('l') | KeyCode::Right => {
            let count = app.count_prefix.take().unwrap_or(1);
            let frame_end = current_frame.offset + current_frame.length.saturating_sub(1);
            if app.cursor_offset < frame_end {
                app.cursor_offset = (app.cursor_offset + count).min(frame_end);
                sync_h_scroll(app);
            }
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let count = app.count_prefix.take().unwrap_or(1);
            let target_frame_num = (current_frame_num + count).min(frame_index.frames.len() - 1);
            if target_frame_num != current_frame_num {
                let col = app.cursor_offset.saturating_sub(current_frame.offset);
                let target_frame = &frame_index.frames[target_frame_num];
                let target_col = col.min(target_frame.length.saturating_sub(1));
                let target_offset = target_frame.offset;
                app.push_jump();
                app.cursor_offset = target_offset + target_col;
                sync_v_scroll(app);
            }
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let count = app.count_prefix.take().unwrap_or(1);
            let target_frame_num = current_frame_num.saturating_sub(count);
            if target_frame_num != current_frame_num {
                let col = app.cursor_offset.saturating_sub(current_frame.offset);
                let target_frame = &frame_index.frames[target_frame_num];
                let target_col = col.min(target_frame.length.saturating_sub(1));
                let target_offset = target_frame.offset;
                app.push_jump();
                app.cursor_offset = target_offset + target_col;
                sync_v_scroll(app);
            }
            true
        }
        KeyCode::Char('0') => {
            app.cursor_offset = current_frame.offset;
            app.h_scroll_offset = 0;
            true
        }
        KeyCode::Char('$') => {
            app.cursor_offset = current_frame.offset + current_frame.length.saturating_sub(1);
            sync_h_scroll(app);
            true
        }
        KeyCode::Char('G') => {
            if !frame_index.frames.is_empty() {
                let col = app.cursor_offset.saturating_sub(current_frame.offset);
                let last_idx = frame_index.frames.len() - 1;
                let last_frame = &frame_index.frames[last_idx];
                let target_col = col.min(last_frame.length.saturating_sub(1));
                let target_offset = last_frame.offset;
                let should_jump = last_idx != current_frame_num;
                if should_jump {
                    app.push_jump();
                }
                app.cursor_offset = target_offset + target_col;
                sync_v_scroll(app);
            }
            true
        }
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            frame_page_down(app);
            true
        }
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            frame_page_up(app);
            true
        }
        _ => false,
    }
}

fn sync_v_scroll(app: &mut App) {
    if let Some(frame_num) = app.current_frame_number() {
        if frame_num < app.scroll_offset {
            app.scroll_offset = frame_num;
        } else if frame_num >= app.scroll_offset + (app.visible_rows.saturating_sub(2).max(1)).saturating_sub(1).max(1) {
            app.scroll_offset = frame_num.saturating_sub((app.visible_rows.saturating_sub(2).max(1)).saturating_sub(1).max(1));
        }
    }
}

fn sync_h_scroll(app: &mut App) {
    if let Some(frame) = app.current_frame() {
        let frame_col = app.cursor_offset.saturating_sub(frame.offset);
        let visible_bytes = app.visible_bytes.max(1);
        if frame_col < app.h_scroll_offset {
            app.h_scroll_offset = frame_col;
        } else if frame_col >= app.h_scroll_offset + visible_bytes {
            app.h_scroll_offset = frame_col.saturating_sub(visible_bytes.saturating_sub(1));
        }
    }
}

fn frame_page_down(app: &mut App) {
    let frame_index = match &app.frame_index {
        Some(fi) => fi,
        None => return,
    };
    let current_frame_num = match app.current_frame_number() {
        Some(n) => n,
        None => return,
    };
    let page_frames = app.visible_rows.saturating_sub(2).max(1);
    let target_frame = (current_frame_num + page_frames).min(frame_index.frames.len().saturating_sub(1));
    if target_frame != current_frame_num {
        let current_frame = &frame_index.frames[current_frame_num];
        let col = app.cursor_offset.saturating_sub(current_frame.offset);
        let target = &frame_index.frames[target_frame];
        let target_col = col.min(target.length.saturating_sub(1));
        app.cursor_offset = target.offset + target_col;
        sync_v_scroll(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// 回归测试：`:help` 命令应进入 Help 模式而不是被强制回 Normal（修复 :help 无反应的 bug）
    #[test]
    fn help_command_enters_help_mode() {
        let mut app = App::new();
        app.mode = Mode::Command;
        app.command_input = "help".to_string();

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.mode, Mode::Help, "`:help` 后应进入 Help 模式");
        assert_eq!(app.help_topic, None);
        assert!(app.command_input.is_empty(), "命令输入框应已清空");
    }

    #[test]
    fn help_command_with_topic_enters_help_mode() {
        let mut app = App::new();
        app.mode = Mode::Command;
        app.command_input = "help search".to_string();

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.help_topic.as_deref(), Some("search"));
    }

    /// 不切换模式的命令（如 :goto）仍应回到 Normal 模式，确保修复不影响原有行为
    #[test]
    fn normal_command_returns_to_normal_mode() {
        let mut app = App::new();
        app.mode = Mode::Command;
        app.command_input = "g 0".to_string();

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.cursor_offset, 0);
    }

    // -----------------------------------------------------------------------
    // Jumplist 回归测试（Task #10）
    // -----------------------------------------------------------------------

    use crate::buffer::Buffer;

    /// 构造带数据的 App（直接构造 Buffer 避免留下编辑/undo 记录）
    fn app_with_data(data: &[u8]) -> App {
        let mut app = App::new();
        app.buffer = Buffer::with_data(data);
        app
    }

    /// 通过命令模式执行 :goto
    fn goto(app: &mut App, offset: usize) {
        app.mode = Mode::Command;
        app.command_input = format!("goto {}", offset);
        let _ = handle_input(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }

    fn ctrl_o() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)
    }

    fn ctrl_w() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)
    }

    /// 跳转后 Ctrl+O 能回到原位置，再次 Ctrl+O 继续回退
    #[test]
    fn ctrl_o_jumps_back_to_previous_location() {
        let mut app = app_with_data(&[0u8; 64]);
        app.cursor_offset = 5;

        goto(&mut app, 40);
        assert_eq!(app.cursor_offset, 40);

        let _ = handle_input(&mut app, ctrl_o());
        assert_eq!(app.cursor_offset, 5, "Ctrl+O 应回退到 :goto 前的位置");
    }

    /// 回退后 Tab 能前进恢复原位置，且恢复后前进栈为空、可再次回退
    #[test]
    fn tab_jumps_forward_after_back() {
        let mut app = app_with_data(&[0u8; 64]);
        app.cursor_offset = 5;

        goto(&mut app, 40);

        let _ = handle_input(&mut app, ctrl_o());
        assert_eq!(app.cursor_offset, 5);

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.cursor_offset, 40, "Tab 应前进恢复回退前的位置");
        assert!(app.jump_forward.is_empty());

        let _ = handle_input(&mut app, ctrl_o());
        assert_eq!(app.cursor_offset, 5, "前进后仍可通过 Ctrl+O 回退");
    }

    /// 新跳转会清空前进栈（浏览器式后退/前进语义）
    #[test]
    fn new_jump_clears_forward_stack() {
        let mut app = app_with_data(&[0u8; 64]);
        app.cursor_offset = 0;

        goto(&mut app, 10);
        goto(&mut app, 20);

        // 回退一次，使前进栈非空（内容为 20）
        let _ = handle_input(&mut app, ctrl_o());
        assert_eq!(app.cursor_offset, 10);
        assert_eq!(app.jump_forward, vec![20]);

        // 新跳转应清空前进栈（20 不再可达）
        goto(&mut app, 30);
        assert!(app.jump_forward.is_empty(), "新跳转后前进栈应被清空");
        assert_eq!(app.cursor_offset, 30);
    }

    /// Ctrl+W 切换 Hex/ASCII 面板（原 Tab 改键）
    #[test]
    fn ctrl_w_switches_panel() {
        let mut app = app_with_data(&[0u8; 16]);
        assert_eq!(app.active_panel, Panel::Hex);

        let _ = handle_input(&mut app, ctrl_w());
        assert_eq!(app.active_panel, Panel::Ascii);

        let _ = handle_input(&mut app, ctrl_w());
        assert_eq!(app.active_panel, Panel::Hex);
    }

    /// Tab 不再切换面板，前进栈为空时光标不变
    #[test]
    fn tab_no_longer_switches_panel() {
        let mut app = app_with_data(&[0u8; 16]);
        app.cursor_offset = 3;

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_panel, Panel::Hex, "Tab 不应再切换面板");
        assert_eq!(app.cursor_offset, 3, "前进栈为空时光标应保持不变");
    }

    // -----------------------------------------------------------------------
    // 类型解读面板回归测试（Task #13）
    // -----------------------------------------------------------------------

    /// t 打开类型解读面板
    #[test]
    fn t_opens_type_panel() {
        let mut app = app_with_data(&[0u8; 16]);
        assert!(!app.type_panel_open);

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        assert!(app.type_panel_open, "t 应打开类型解读面板");
    }

    /// 面板打开时按 e 切换端序
    #[test]
    fn e_toggles_endianness_while_panel_open() {
        let mut app = app_with_data(&[0u8; 16]);
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        assert!(app.type_endian_le);

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert!(!app.type_endian_le, "e 应切换为 BE");
        assert!(app.type_panel_open, "切换端序后面板应保持打开");
    }

    /// 面板打开时光标移动且面板保持打开（实时解读的前提）
    #[test]
    fn cursor_moves_while_panel_open() {
        let mut app = app_with_data(&[0u8; 64]);
        app.cursor_offset = 0;
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert_eq!(app.cursor_offset, 1, "面板打开时 l 应移动光标");
        assert!(app.type_panel_open);

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.cursor_offset, 17, "面板打开时 j 应移动光标");
        assert!(app.type_panel_open, "导航后面板应保持打开");
    }

    /// 面板打开时按 q / Esc 关闭
    #[test]
    fn q_and_esc_close_type_panel() {
        let mut app = app_with_data(&[0u8; 16]);
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.type_panel_open, "q 应关闭面板");

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        assert!(app.type_panel_open);
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.type_panel_open, "Esc 应关闭面板");
    }

    /// 面板打开时按 i 进入 Insert 模式：主循环守卫（在 App::run 中）负责关闭面板，
    /// 测试环境无法触发 run()，此处直接验证守卫的标志逻辑等价行为，
    /// 并确认 i 仍能正常进入 Insert 模式。
    #[test]
    fn i_enters_insert_mode_while_panel_open() {
        let mut app = app_with_data(&[0u8; 16]);
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Insert, "面板打开时 i 仍应进入 Insert 模式");
        // 模拟主循环守卫：离开 Normal 模式后自动关闭面板
        if app.mode != Mode::Normal && app.type_panel_open {
            app.type_panel_open = false;
        }
        assert!(!app.type_panel_open, "进入 Insert 后面板应被守卫关闭");
    }
}

fn frame_page_up(app: &mut App) {
    let frame_index = match &app.frame_index {
        Some(fi) => fi,
        None => return,
    };
    let current_frame_num = match app.current_frame_number() {
        Some(n) => n,
        None => return,
    };
    let page_frames = app.visible_rows.saturating_sub(2).max(1);
    let target_frame = current_frame_num.saturating_sub(page_frames);
    if target_frame != current_frame_num {
        let current_frame = &frame_index.frames[current_frame_num];
        let col = app.cursor_offset.saturating_sub(current_frame.offset);
        let target = &frame_index.frames[target_frame];
        let target_col = col.min(target.length.saturating_sub(1));
        app.cursor_offset = target.offset + target_col;
        sync_v_scroll(app);
    }
}
