use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{App, LastChange, Mode};
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
                let mut deleted = 0usize;
                for _ in 0..count {
                    deleted += delete_line(app);
                }
                if deleted > 0 {
                    app.last_change = Some(LastChange::Delete { len: deleted });
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
    // 匹配列表浮层打开时：拦截列表自身按键，其余键穿透到原有导航逻辑（仿 type panel 范式）
    if app.match_list_open {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.match_list_open = false;
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let count = app.count_prefix.take().unwrap_or(1);
                app.match_list_sel = app.match_list_sel.saturating_sub(count);
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let count = app.count_prefix.take().unwrap_or(1);
                let last = app.search_state.matches.len().saturating_sub(1);
                app.match_list_sel = app.match_list_sel.saturating_add(count).min(last);
                return;
            }
            KeyCode::Enter => {
                if let Some(&offset) = app.search_state.matches.get(app.match_list_sel) {
                    if offset != app.cursor_offset {
                        app.push_jump();
                    }
                    app.cursor_offset = offset;
                    // 同步 search_state 当前匹配，使 n/N 与高亮语义一致（无副作用则跳过）
                    app.search_state.current_match = Some(app.match_list_sel);
                }
                app.match_list_open = false;
                // 视图滚动由主循环的 scroll 同步逻辑自动跟随光标
                return;
            }
            _ => {}
        }
    }

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

    // 校验和浮层打开时：拦截 q/Esc 关闭，其余键穿透到原有导航逻辑
    if app.sum_open {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            app.sum_open = false;
            return;
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

        // 搜索匹配列表入口（与 :list 等价）
        KeyCode::Char('L') => {
            command::open_match_list(app);
        }

        // 模式切换
        KeyCode::Char('i') => {
            app.mode = Mode::Insert;
            app.insert_after = false;
            app.nibble_input = None;
            app.change_start = Some(app.cursor_offset);
        }
        KeyCode::Char('a') => {
            app.mode = Mode::Insert;
            app.insert_after = true;
            app.nibble_input = None;
            if !app.buffer.is_empty() {
                app.cursor_offset = (app.cursor_offset + 1).min(app.buffer.len());
            }
            app.change_start = Some(app.cursor_offset);
        }
        KeyCode::Char('R') => {
            app.mode = Mode::Replace;
            app.nibble_input = None;
            app.change_start = Some(app.cursor_offset);
        }
        KeyCode::Char(':') => {
            app.mode = Mode::Command;
            app.command_input.clear();
            app.history_index = None;
        }

        // 搜索
        KeyCode::Char('/') => {
            app.mode = Mode::Search;
            app.search_input.clear();
            app.search_state.clear();
            app.history_index = None;
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
                let mut deleted = 0usize;
                for _ in 0..count {
                    if app.cursor_offset < app.buffer.len() {
                        editor::remove_byte(app, app.cursor_offset);
                        deleted += 1;
                    }
                }
                app.undo_manager.end_group();
                if deleted > 0 {
                    app.last_change = Some(LastChange::Delete { len: deleted });
                }
                clamp_cursor(app);
            }
        }

        // 重复上次修改
        KeyCode::Char('.') => {
            let count = app.count_prefix.take().unwrap_or(1);
            repeat_last_change(app, count);
        }

        // Visual 模式（V 进入行选模式）
        KeyCode::Char('v') => {
            app.mode = Mode::Visual;
            app.visual_anchor = Some(app.cursor_offset);
            app.visual_line = false;
        }
        KeyCode::Char('V') => {
            app.mode = Mode::Visual;
            app.visual_anchor = Some(app.cursor_offset);
            app.visual_line = true;
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
                app.last_change = Some(LastChange::Paste);
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

    // 帧模式下复用帧导航逻辑（与 Normal 模式一致，按帧行宽/帧边界移动，
    // 避免落入下方写死 16 字节行宽的通用移动）
    if handle_frame_navigation(app, key) {
        return;
    }

    let count = app.count_prefix.take().unwrap_or(1);

    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.visual_anchor = None;
            app.visual_line = false;
        }
        // v / V 在字符选区与行选区之间切换（锚点不变）
        KeyCode::Char('v') | KeyCode::Char('V') => {
            app.visual_line = !app.visual_line;
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
        // 进入 Command 模式：选区范围暂存到 pending_range（校验和命令优先使用），
        // 同时退出 Visual（visual_anchor 清空，选区高亮消失）
        KeyCode::Char(':') => {
            app.pending_range = app.selection_range();
            app.visual_anchor = None;
            app.visual_line = false;
            app.mode = Mode::Command;
            app.command_input.clear();
            app.history_index = None;
        }
        KeyCode::Char('y') => {
            if let Some((start, end)) = app.selection_range() {
                let len = end - start + 1;
                app.yank_buffer = app.buffer.get_range(start, len).to_vec();
                app.mode = Mode::Normal;
                app.visual_anchor = None;
                app.visual_line = false;
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
                app.visual_line = false;
                app.cursor_offset = start;
                clamp_cursor(app);
                app.last_change = Some(LastChange::Delete { len });
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
            // 截取本次会话插入的字节（空会话不记录，保留上次修改）
            if let Some(start) = app.change_start.take() {
                let end = app.cursor_offset.min(app.buffer.len());
                if end > start {
                    let bytes = app.buffer.get_range(start, end - start).to_vec();
                    app.last_change = Some(LastChange::Insert { bytes });
                }
            }
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
            // 截取本次会话覆盖的字节（空会话不记录，保留上次修改）
            if let Some(start) = app.change_start.take() {
                // 覆盖写入不增长缓冲区；若在 EOF 处提前停止，
                // 结束位置以缓冲区长度为准（正常不会超过）
                let end = app.cursor_offset.min(app.buffer.len());
                if end > start {
                    let bytes = app.buffer.get_range(start, end - start).to_vec();
                    app.last_change = Some(LastChange::Overwrite { bytes });
                }
            }
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
            push_history(&mut app.search_history, &input);
            app.history_index = None;
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
        KeyCode::Up => {
            history_up(&app.search_history, &mut app.history_index, &mut app.search_input);
        }
        KeyCode::Down => {
            history_down(&app.search_history, &mut app.history_index, &mut app.search_input);
        }
        _ => {}
    }
}

fn handle_command_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            let cmd = app.command_input.clone();
            push_history(&mut app.command_history, &cmd);
            app.history_index = None;
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
        KeyCode::Up => {
            history_up(&app.command_history, &mut app.history_index, &mut app.command_input);
        }
        KeyCode::Down => {
            history_down(&app.command_history, &mut app.history_index, &mut app.command_input);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 命令/搜索历史（↑↓ 回溯）
// ---------------------------------------------------------------------------

/// 非空且与末条不重复时入队，上限 100 条（超出移除最旧）
fn push_history(history: &mut Vec<String>, entry: &str) {
    if entry.is_empty() {
        return;
    }
    if history.last().map(|s| s.as_str()) == Some(entry) {
        return;
    }
    history.push(entry.to_string());
    if history.len() > 100 {
        history.remove(0);
    }
}

/// Up：从最新一条开始向前浏览，到头后保持不动
fn history_up(history: &[String], index: &mut Option<usize>, input: &mut String) {
    if history.is_empty() {
        return;
    }
    let idx = match *index {
        None => history.len() - 1,
        Some(i) => i.saturating_sub(1),
    };
    *index = Some(idx);
    *input = history[idx].clone();
}

/// Down：向后浏览，越过最新一条后清空输入并退出浏览状态
fn history_down(history: &[String], index: &mut Option<usize>, input: &mut String) {
    if let Some(i) = *index {
        let next = i + 1;
        if next >= history.len() {
            *index = None;
            input.clear();
        } else {
            *index = Some(next);
            *input = history[next].clone();
        }
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
                        app.last_change = Some(LastChange::ReplaceByte { value });
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
                        app.last_change = Some(LastChange::ReplaceByte { value: c as u8 });
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

fn delete_line(app: &mut App) -> usize {
    if app.buffer.is_empty() {
        return 0;
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
    count
}

fn clamp_cursor(app: &mut App) {
    if !app.buffer.is_empty() && app.cursor_offset >= app.buffer.len() {
        app.cursor_offset = app.buffer.len().saturating_sub(1);
    }
}

// ---------------------------------------------------------------------------
// `.` 重复上次修改（支持数字前缀，重放不改变 last_change）
// ---------------------------------------------------------------------------
fn repeat_last_change(app: &mut App, count: usize) {
    if count == 0 {
        return;
    }
    let change = match &app.last_change {
        Some(c) => c.clone(),
        None => return,
    };

    match change {
        LastChange::Insert { bytes } => {
            if bytes.is_empty() {
                return;
            }
            let mut repeated = Vec::with_capacity(bytes.len() * count);
            for _ in 0..count {
                repeated.extend_from_slice(&bytes);
            }
            let pos = app.cursor_offset.min(app.buffer.len());
            editor::insert_bytes(app, pos, &repeated);
            // 与插入会话退出后的光标位置语义一致：停留在插入内容之后（钳制到文件尾最后一个字节）
            if !app.buffer.is_empty() {
                app.cursor_offset = (pos + repeated.len()).min(app.buffer.len() - 1);
            }
        }
        LastChange::Overwrite { bytes } => {
            if bytes.is_empty() || app.cursor_offset >= app.buffer.len() {
                return;
            }
            let mut repeated = Vec::with_capacity(bytes.len() * count);
            for _ in 0..count {
                repeated.extend_from_slice(&bytes);
            }
            // 钳制到文件尾，超出部分截断
            repeated.truncate(app.buffer.len() - app.cursor_offset);
            app.undo_manager.begin_group("overwrite bytes");
            for (i, &b) in repeated.iter().enumerate() {
                editor::set_byte(app, app.cursor_offset + i, b);
            }
            app.undo_manager.end_group();
            // 与 R 模式光标语义一致：越过覆盖内容（钳制到文件尾最后一个字节）
            app.cursor_offset = (app.cursor_offset + repeated.len())
                .min(app.buffer.len().saturating_sub(1));
        }
        LastChange::ReplaceByte { value } => {
            if app.cursor_offset >= app.buffer.len() {
                return;
            }
            let n = count.min(app.buffer.len() - app.cursor_offset);
            app.undo_manager.begin_group("replace bytes");
            for i in 0..n {
                editor::set_byte(app, app.cursor_offset + i, value);
            }
            app.undo_manager.end_group();
            // 与 r 命令光标语义一致：越过被替换字节（钳制到文件尾最后一个字节）
            app.cursor_offset = (app.cursor_offset + n)
                .min(app.buffer.len().saturating_sub(1));
        }
        LastChange::Delete { len } => {
            if len == 0 || app.cursor_offset >= app.buffer.len() {
                return;
            }
            let total = len.saturating_mul(count).min(app.buffer.len() - app.cursor_offset);
            if total == 0 {
                return;
            }
            editor::remove_range(app, app.cursor_offset, total);
            clamp_cursor(app);
        }
        LastChange::Paste => {
            if app.yank_buffer.is_empty() {
                return;
            }
            let yank_data = app.yank_buffer.clone();
            let mut repeated = Vec::with_capacity(yank_data.len() * count);
            for _ in 0..count {
                repeated.extend_from_slice(&yank_data);
            }
            let insert_pos = if app.buffer.is_empty() { 0 } else { app.cursor_offset + 1 };
            editor::insert_bytes(app, insert_pos, &repeated);
            app.cursor_offset = (insert_pos + repeated.len() - 1)
                .min(app.buffer.len().saturating_sub(1));
        }
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

    // -----------------------------------------------------------------------
    // 校验和回归测试（Task #18）
    // -----------------------------------------------------------------------

    /// Visual 选区后按 `:` 进入 Command 模式：选区暂存到 pending_range，
    /// visual_anchor 清空；空命令执行后 pending_range 也被清空；
    /// 带选区执行 `:md5` 消息行只包含选区（"abc"）的 MD5
    #[test]
    fn visual_colon_stashes_selection_and_md5_uses_it() {
        let mut app = app_with_data(b"0123456789");
        app.cursor_offset = 2;
        let _ = handle_input(&mut app, key('v'));
        let _ = handle_input(&mut app, key('l'));
        let _ = handle_input(&mut app, key('l')); // 选区 2..=4（3 字节）
        let _ = handle_input(&mut app, key(':'));
        assert_eq!(app.mode, Mode::Command);
        assert_eq!(app.pending_range, Some((2, 4)), "选区应暂存到 pending_range");
        assert_eq!(app.visual_anchor, None, "进入 Command 后应退出 Visual");

        // 空命令执行后暂存范围应清空，不污染后续命令
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.pending_range, None, "命令执行后应清空 pending_range");

        // 再来一次：选区后执行 :md5，只算选区字节（3 字节 0x32 0x33 0x34）
        let mut app = app_with_data(b"0123456789");
        app.cursor_offset = 2;
        let _ = handle_input(&mut app, key('v'));
        let _ = handle_input(&mut app, key('l'));
        let _ = handle_input(&mut app, key('l'));
        let _ = handle_input(&mut app, key(':'));
        app.command_input = "md5".to_string();
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let (msg, _) = app.message.as_ref().expect("应有消息行");
        assert_eq!(
            msg,
            "MD5: 289dff07669d7a23de0ef88d2f7129e7",
            ":md5 应只计算选区字节 0x32..0x34"
        );
    }

    /// `:sum` 打开校验和浮层，q / Esc 关闭（其余键穿透不影响导航）
    #[test]
    fn sum_panel_opens_and_closes_with_q_or_esc() {
        let mut app = app_with_data(b"abc");
        app.mode = Mode::Command;
        app.command_input = "sum".to_string();
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.sum_open, ":sum 应打开校验和浮层");
        assert!(app.sum_info.is_some());

        let _ = handle_input(&mut app, key('q'));
        assert!(!app.sum_open, "q 应关闭校验和浮层");
        // 光标未移动，确认 q 只用于关闭浮层（同 type panel 拦截语义）
        assert_eq!(app.cursor_offset, 0);

        // Esc 同样关闭；关闭后导航键恢复正常（l 移动光标）
        app.mode = Mode::Command;
        app.command_input = "sum".to_string();
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.sum_open);
        let _ = handle_input(&mut app, esc());
        assert!(!app.sum_open, "Esc 应关闭校验和浮层");
        let _ = handle_input(&mut app, key('l'));
        assert_eq!(app.cursor_offset, 1, "关闭后导航键应恢复正常");
    }

    // -----------------------------------------------------------------------
    // 命令/搜索历史回归测试（Task #15）
    // -----------------------------------------------------------------------

    /// 执行一条命令后重新进入 Command 模式，按 Up 载入该命令
    #[test]
    fn up_loads_last_command_after_execution() {
        let mut app = app_with_data(&[0u8; 64]);
        goto(&mut app, 40);
        assert_eq!(app.command_history, vec!["goto 40"]);

        // 重新进入 Command 模式（按 :）
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Command);
        assert!(app.command_input.is_empty());
        assert_eq!(app.history_index, None, "进入 Command 模式时应重置浏览位置");

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.command_input, "goto 40", "Up 应载入最近一条命令");
        assert_eq!(app.history_index, Some(0));
    }

    /// 连续 Up 不越界：到头后保持在最旧一条
    #[test]
    fn repeated_up_does_not_underflow() {
        let mut app = app_with_data(&[0u8; 64]);
        goto(&mut app, 10);
        goto(&mut app, 20);
        assert_eq!(app.command_history, vec!["goto 10", "goto 20"]);

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        for _ in 0..5 {
            let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        }
        assert_eq!(app.history_index, Some(0), "连续 Up 应停在最旧一条不越界");
        assert_eq!(app.command_input, "goto 10");
    }

    /// Down 越过最新一条后清空输入并退出浏览状态；中途 Down 载入对应历史
    #[test]
    fn down_past_newest_clears_input() {
        let mut app = app_with_data(&[0u8; 64]);
        goto(&mut app, 10);
        goto(&mut app, 20);

        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        // 无浏览状态时 Down 无操作
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(app.command_input.is_empty());

        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let _ = handle_input(&mut app, up);
        let _ = handle_input(&mut app, up);
        assert_eq!(app.command_input, "goto 10");

        let _ = handle_input(&mut app, down);
        assert_eq!(app.command_input, "goto 20", "Down 应载入下一条历史");
        assert_eq!(app.history_index, Some(1));

        let _ = handle_input(&mut app, down);
        assert!(app.command_input.is_empty(), "Down 到底后应清空输入");
        assert_eq!(app.history_index, None);
    }

    /// 连续重复命令只存一条；空命令不入库；上限 100 条移除最旧
    #[test]
    fn history_dedupes_and_caps_entries() {
        let mut app = app_with_data(&[0u8; 64]);
        goto(&mut app, 10);
        goto(&mut app, 10);
        assert_eq!(app.command_history, vec!["goto 10"], "连续重复命令只存一条");
        goto(&mut app, 20);

        // 空命令不入库（直接按 Enter）
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.command_history.len(), 2);

        // 填满超过 100 条后移除最旧
        for i in 0..110 {
            push_history(&mut app.command_history, &format!("cmd{}", i));
        }
        assert_eq!(app.command_history.len(), 100, "历史上限 100 条");
        assert_eq!(app.command_history[0], "cmd10", "超出上限时移除最旧条目");
    }

    /// Search 模式：提交后 Up 能载入上次搜索词，重复提交不重复入库
    #[test]
    fn search_history_up_loads_last_query() {
        let mut app = app_with_data(b"hello world");
        app.mode = Mode::Search;
        app.search_input = "world".to_string();
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.search_history, vec!["world"]);
        assert_eq!(app.mode, Mode::Normal);

        // 重复提交相同搜索词不重复入库（先模拟异步搜索结果回收）
        let _ = app.search_state.poll_result();
        app.mode = Mode::Search;
        app.search_input = "world".to_string();
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.search_history.len(), 1, "连续重复搜索词只存一条");
        let _ = app.search_state.poll_result();

        // 按 / 重新进入 Search 模式，Up 载入上次搜索词
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Search);
        assert_eq!(app.history_index, None, "进入 Search 模式时应重置浏览位置");
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.search_input, "world", "Up 应载入最近一条搜索词");
    }

    // -----------------------------------------------------------------------
    // `.` 重复上次修改回归测试（Task #17）
    // -----------------------------------------------------------------------

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn esc() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    /// 模拟 Hex 面板插入会话：i + 十六进制数字 + Esc（默认 active_panel 为 Hex）
    fn insert_session(app: &mut App, hex_digits: &str) {
        let _ = handle_input(app, key('i'));
        for d in hex_digits.chars() {
            let _ = handle_input(app, key(d));
        }
        let _ = handle_input(app, esc());
    }

    fn dot(app: &mut App) {
        let _ = handle_input(app, key('.'));
    }

    /// 插入会话后 `.` 重复插入相同字节；连续 `.` 重复同一修改（重放不改变 last_change）
    #[test]
    fn dot_repeats_insert_session() {
        let mut app = app_with_data(&[0u8; 8]);
        insert_session(&mut app, "abcd"); // 插入 0xAB 0xCD，缓冲区变为 10 字节
        assert_eq!(app.buffer.data(), &[0xAB, 0xCD, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(app.cursor_offset, 2, "插入会话退出后光标停留在插入内容之后");

        dot(&mut app);
        assert_eq!(
            app.buffer.data(),
            &[0xAB, 0xCD, 0xAB, 0xCD, 0, 0, 0, 0, 0, 0, 0, 0],
            ". 应在光标处重复插入会话字节"
        );
        assert_eq!(
            app.last_change,
            Some(LastChange::Insert { bytes: vec![0xAB, 0xCD] }),
            "重放不应改变 last_change"
        );

        dot(&mut app);
        assert_eq!(
            app.buffer.data(),
            &[0xAB, 0xCD, 0xAB, 0xCD, 0xAB, 0xCD, 0, 0, 0, 0, 0, 0, 0, 0],
            "连续 . 应重复同一修改"
        );
    }

    /// `3x` 后 `.` 再删 1 字节，`2.` 删 2 字节（count 前缀生效）
    #[test]
    fn dot_repeats_delete_with_count() {
        let mut app = app_with_data(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let _ = handle_input(&mut app, key('3'));
        let _ = handle_input(&mut app, key('x'));
        assert_eq!(app.buffer.data(), &[4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(app.last_change, Some(LastChange::Delete { len: 3 }));

        dot(&mut app);
        assert_eq!(app.buffer.data(), &[7, 8, 9, 10], ". 应再删 3 字节");
    }

    /// `2.` 带数字前缀重复删除：删除长度乘以 count
    #[test]
    fn dot_with_count_prefix_multiplies_delete() {
        let mut app = app_with_data(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let _ = handle_input(&mut app, key('3'));
        let _ = handle_input(&mut app, key('x'));
        assert_eq!(app.buffer.data(), &[4, 5, 6, 7, 8, 9, 10]);

        let _ = handle_input(&mut app, key('2'));
        dot(&mut app);
        assert_eq!(app.buffer.data(), &[10], "2. 应删除 3*2=6 字节");
    }

    /// `r` 单字节替换后 `.` 重复替换后续字节（钳制到文件尾）
    #[test]
    fn dot_repeats_single_replace() {
        let mut app = app_with_data(&[0, 0, 0]);
        let _ = handle_input(&mut app, key('r'));
        let _ = handle_input(&mut app, key('f'));
        let _ = handle_input(&mut app, key('f'));
        assert_eq!(app.buffer.data(), &[0xFF, 0, 0]);
        assert_eq!(app.last_change, Some(LastChange::ReplaceByte { value: 0xFF }));

        dot(&mut app);
        assert_eq!(app.buffer.data(), &[0xFF, 0xFF, 0], ". 应用相同值替换光标处字节");
        dot(&mut app);
        assert_eq!(app.buffer.data(), &[0xFF, 0xFF, 0xFF]);
        dot(&mut app); // 已到文件尾，不 panic 且不再变化（钳制）
        assert_eq!(app.buffer.data(), &[0xFF, 0xFF, 0xFF]);
    }

    /// `p` 粘贴后 `.` 再次粘贴（使用当前 yank_buffer）
    #[test]
    fn dot_repeats_paste() {
        let mut app = app_with_data(&[1, 2, 3]);
        app.yank_buffer = vec![9];
        app.cursor_offset = 0;

        let _ = handle_input(&mut app, key('p'));
        assert_eq!(app.buffer.data(), &[1, 9, 2, 3]);

        dot(&mut app);
        assert_eq!(app.buffer.data(), &[1, 9, 9, 2, 3], ". 应在光标后再次粘贴");
    }

    /// 空插入会话（i 后直接 Esc）不记录，`.` 保持上一次修改
    #[test]
    fn empty_insert_session_keeps_last_change() {
        let mut app = app_with_data(&[0u8; 4]);
        insert_session(&mut app, "ab"); // 记录 Insert{[0xAB]}
        assert_eq!(app.last_change, Some(LastChange::Insert { bytes: vec![0xAB] }));

        // 空会话：i 后直接 Esc
        let _ = handle_input(&mut app, key('i'));
        let _ = handle_input(&mut app, esc());
        assert_eq!(
            app.last_change,
            Some(LastChange::Insert { bytes: vec![0xAB] }),
            "空会话不应覆盖 last_change"
        );

        let before = app.buffer.data().to_vec();
        dot(&mut app); // 仍重放上一次的插入（空会话未覆盖记录）
        assert_eq!(app.buffer.data().len(), before.len() + 1);
    }

    /// 文件尾钳制：`2x` 超出剩余长度时只记录实际删除长度；
    /// 清空文件后 `.` 不 panic、无副作用
    #[test]
    fn dot_at_eof_does_not_panic() {
        let mut app = app_with_data(&[7]);
        let _ = handle_input(&mut app, key('2')); // 2x 超出剩余长度，实际只删 1 字节
        let _ = handle_input(&mut app, key('x'));
        assert!(app.buffer.is_empty());
        assert_eq!(app.last_change, Some(LastChange::Delete { len: 1 }));

        dot(&mut app); // 光标已在 EOF，不 panic、无变化
        assert!(app.buffer.is_empty());
    }

    /// 重放整体作为一个 undo 组：一次 `u` 即可完全撤销 `.` 的效果
    #[test]
    fn dot_replay_is_single_undo_group() {
        let mut app = app_with_data(&[0u8; 8]);
        insert_session(&mut app, "abcd");
        let after_session = app.buffer.data().to_vec();

        dot(&mut app);
        assert_ne!(app.buffer.data(), &after_session[..]);

        editor::undo(&mut app);
        assert_eq!(app.buffer.data(), &after_session[..], "一次 undo 应完整撤销重放");
    }

    // -----------------------------------------------------------------------
    // 搜索匹配列表回归测试（Task #20）
    // -----------------------------------------------------------------------

    /// 执行一次搜索并等待异步完成（收集 matches）
    fn search_sync(app: &mut App, query: &str) {
        app.mode = Mode::Search;
        app.search_input = query.to_string();
        let _ = handle_input(app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        while !app.poll_search_result() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// 无搜索结果时 :list / L 只提示消息，不打开浮层
    #[test]
    fn list_without_results_shows_message() {
        let mut app = app_with_data(b"hello world");
        command::execute_command(&mut app, "list").unwrap();
        assert!(!app.match_list_open);
        assert_eq!(app.message.as_ref().unwrap().0, "No search results");

        let _ = handle_input(&mut app, key('L'));
        assert!(!app.match_list_open, "无结果时 L 不应打开浮层");
        assert_eq!(app.message.as_ref().unwrap().0, "No search results");
    }

    /// 搜索后 :list / :matches 打开列表，选中初始化为最接近光标的匹配；L 等价
    #[test]
    fn list_opens_with_selection_near_cursor() {
        let mut app = app_with_data(b"aaa aaa aaa"); // 匹配于 0, 4, 8
        search_sync(&mut app, "aaa");
        app.cursor_offset = 5;
        command::execute_command(&mut app, "list").unwrap();
        assert!(app.match_list_open, ":list 应打开匹配列表");
        assert_eq!(app.match_list_sel, 1, "选中应为最接近光标的匹配（0x4）");

        app.match_list_open = false;
        command::execute_command(&mut app, "matches").unwrap();
        assert!(app.match_list_open, ":matches 别名应同样打开");
        app.match_list_open = false;
        let _ = handle_input(&mut app, key('L'));
        assert!(app.match_list_open, "L 快捷键应等价于 :list");
    }

    /// ↑↓ / jk 导航不越界；列表打开时导航键不移动光标
    #[test]
    fn list_navigation_clamps_at_bounds() {
        let mut app = app_with_data(b"aaa aaa aaa");
        search_sync(&mut app, "aaa");
        app.cursor_offset = 0;
        command::execute_command(&mut app, "list").unwrap();
        assert_eq!(app.match_list_sel, 0);

        for _ in 0..3 {
            let _ = handle_input(&mut app, key('k'));
        }
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.match_list_sel, 0, "顶部连续上移不越界");

        for _ in 0..5 {
            let _ = handle_input(&mut app, key('j'));
        }
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.match_list_sel, 2, "底部连续下移不越界");
        assert_eq!(app.cursor_offset, 0, "列表打开时导航键不移动光标");
    }

    /// Enter 跳转光标到选中匹配偏移并关闭，跳转点记入 jumplist；
    /// q/Esc 只关闭不移动光标；关闭后 L 可再次打开
    #[test]
    fn list_enter_jumps_and_q_esc_close() {
        let mut app = app_with_data(b"aaa aaa aaa");
        search_sync(&mut app, "aaa");
        app.cursor_offset = 2;
        command::execute_command(&mut app, "list").unwrap();

        let _ = handle_input(&mut app, key('j')); // 选中第二个匹配（0x4）
        let _ = handle_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.cursor_offset, 4, "Enter 应跳转到选中匹配偏移");
        assert!(!app.match_list_open, "Enter 后应关闭列表");
        assert_eq!(app.jump_back.last(), Some(&2), "跳转前位置应记入 jumplist");

        // q 关闭：光标不动；L 再次打开后 Esc 同样关闭
        command::execute_command(&mut app, "list").unwrap();
        let _ = handle_input(&mut app, key('q'));
        assert!(!app.match_list_open, "q 应关闭列表");
        assert_eq!(app.cursor_offset, 4);

        let _ = handle_input(&mut app, key('L'));
        assert!(app.match_list_open);
        let _ = handle_input(&mut app, esc());
        assert!(!app.match_list_open, "Esc 应关闭列表");
        assert_eq!(app.cursor_offset, 4);
    }

    /// 新搜索完成后列表刷新：保持打开，选中/滚动重置，显示新结果；
    /// 替换清空结果后下次 :list 提示无结果（不做复杂失效追踪）
    #[test]
    fn list_refreshes_after_new_search() {
        let mut app = app_with_data(b"aa aa aa");
        search_sync(&mut app, "aa"); // 匹配于 0, 3, 6
        command::execute_command(&mut app, "list").unwrap();
        let _ = handle_input(&mut app, key('j'));
        assert_eq!(app.match_list_sel, 1);

        // 新搜索完成（模拟主循环 poll_result 收集），列表应刷新为新结果
        search_sync(&mut app, "a"); // 匹配于 0, 1, 3, 4, 6, 7
        assert!(app.match_list_open, "新搜索后列表应保持打开");
        assert_eq!(app.match_list_sel, 0, "新搜索后选中应重置");
        assert_eq!(app.match_list_scroll, 0);
        assert_eq!(app.search_state.matches.len(), 6);
    }

    // -----------------------------------------------------------------------
    // frame 模式 Visual 回归测试（Task #22）
    // -----------------------------------------------------------------------

    /// 构造定长帧模式的 App（32 字节/帧）
    fn app_with_frames(data: &[u8], frame_len: usize) -> App {
        let mut app = app_with_data(data);
        let index = crate::frame::build_frame_index(
            app.buffer.data(),
            &crate::frame::FrameConfig::FixedLength { length: frame_len },
        );
        app.frame_index = Some(index);
        app.view_mode = ViewMode::Frame;
        app
    }

    /// frame 模式下 Visual 模式 j/k 应按帧行宽（32 字节）移动，而非固定 16 字节
    #[test]
    fn visual_jk_moves_by_frame_width_in_frame_mode() {
        let mut app = app_with_frames(&[0u8; 96], 32);
        app.cursor_offset = 0;

        let _ = handle_input(&mut app, key('v'));
        assert_eq!(app.mode, Mode::Visual);

        let _ = handle_input(&mut app, key('j'));
        assert_eq!(app.cursor_offset, 32, "j 应移动到下一帧同列（步长=帧宽 32）");

        let _ = handle_input(&mut app, key('k'));
        assert_eq!(app.cursor_offset, 0, "k 应回到上一帧同列");
    }

    /// frame 模式下 Visual 移动后 selection_range 应覆盖锚点到光标；
    /// 0/$ 按帧边界定位而非 16 字节行边界
    #[test]
    fn visual_selection_range_in_frame_mode() {
        let mut app = app_with_frames(&[0u8; 96], 32);
        app.cursor_offset = 2;

        let _ = handle_input(&mut app, key('v'));
        let _ = handle_input(&mut app, key('j'));
        assert_eq!(app.cursor_offset, 34);
        assert_eq!(app.selection_range(), Some((2, 34)), "选区应覆盖锚点到光标");

        let _ = handle_input(&mut app, key('$'));
        assert_eq!(app.cursor_offset, 63, "$ 应定位到当前帧最后一字节");
        assert_eq!(app.selection_range(), Some((2, 63)));

        let _ = handle_input(&mut app, key('0'));
        assert_eq!(app.cursor_offset, 32, "0 应定位到当前帧首字节");
        assert_eq!(app.selection_range(), Some((2, 32)));
    }

    // -----------------------------------------------------------------------
    // Visual Line 行选模式回归测试（Task #23）
    // -----------------------------------------------------------------------

    /// V 进入行选，j 扩展整行；selection_range 吸附 16 字节行边界
    #[test]
    fn visual_line_snaps_selection_to_16_byte_rows() {
        let mut app = app_with_data(&[0u8; 48]);
        app.cursor_offset = 5;

        let _ = handle_input(&mut app, key('V'));
        assert_eq!(app.mode, Mode::Visual);
        assert!(app.visual_line, "V 应进入行选模式");
        assert_eq!(app.selection_range(), Some((0, 15)), "单行选区应吸附整行");

        let _ = handle_input(&mut app, key('j'));
        assert_eq!(app.cursor_offset, 21);
        assert_eq!(app.selection_range(), Some((0, 31)), "j 应扩展包含整个第二行");
    }

    /// 帧模式（定长 32）下行选吸附帧边界
    #[test]
    fn visual_line_snaps_to_frame_boundaries() {
        let mut app = app_with_frames(&[0u8; 96], 32);
        app.cursor_offset = 5;

        let _ = handle_input(&mut app, key('V'));
        assert_eq!(app.selection_range(), Some((0, 31)), "单帧应吸附到帧边界");

        let _ = handle_input(&mut app, key('j'));
        assert_eq!(app.cursor_offset, 37);
        assert_eq!(app.selection_range(), Some((0, 63)), "j 应扩展包含整个第二帧");
    }

    /// Visual 内 v/V 互切，锚点不变；切换后选区吸附行为随之变化
    #[test]
    fn visual_v_v_toggle_switches_line_mode() {
        let mut app = app_with_data(&[0u8; 48]);
        app.cursor_offset = 5;

        let _ = handle_input(&mut app, key('V'));
        assert!(app.visual_line);
        let _ = handle_input(&mut app, key('v'));
        assert!(!app.visual_line, "v 应切换为字符选区");
        assert_eq!(app.selection_range(), Some((5, 5)), "切换后选区不再吸附行边界");
        let _ = handle_input(&mut app, key('l'));
        let _ = handle_input(&mut app, key('V'));
        assert!(app.visual_line, "V 应切换回行选");
        assert_eq!(app.selection_range(), Some((0, 15)));
        assert_eq!(app.visual_anchor, Some(5), "切换时锚点不变");
    }

    /// 行选 d 删除整行，光标移到删除位置起始处，并记录 last_change / yank
    #[test]
    fn visual_line_d_deletes_whole_rows() {
        let mut app = app_with_data(&[1u8; 48]);
        app.cursor_offset = 5;

        let _ = handle_input(&mut app, key('V'));
        let _ = handle_input(&mut app, key('j'));
        let _ = handle_input(&mut app, key('d'));
        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.visual_line, "退出 Visual 应清零 visual_line");
        assert_eq!(app.buffer.len(), 16, "应删除前两行（32 字节）");
        assert_eq!(app.cursor_offset, 0, "光标应移到删除位置起始处");
        assert_eq!(app.last_change, Some(LastChange::Delete { len: 32 }));
        assert_eq!(app.yank_buffer.len(), 32);
    }

    /// 行选 y 复制整行；Esc 退出后 visual_line 清零且锚点清空
    #[test]
    fn visual_line_y_and_esc_exit() {
        let mut app = app_with_data(&[7u8; 32]);
        app.cursor_offset = 20;

        let _ = handle_input(&mut app, key('V'));
        let _ = handle_input(&mut app, key('y'));
        assert_eq!(app.yank_buffer, vec![7u8; 16], "y 应复制整个第二行");
        assert_eq!(app.cursor_offset, 16, "复制后光标回到选区起始行首");
        assert!(!app.visual_line);

        let _ = handle_input(&mut app, key('V'));
        assert!(app.visual_line);
        let _ = handle_input(&mut app, esc());
        assert!(!app.visual_line, "Esc 退出后 visual_line 应清零");
        assert_eq!(app.visual_anchor, None);
    }

    /// 行选按 : 进入 Command：pending_range 为吸附后的整行范围，并退出 Visual
    #[test]
    fn visual_line_colon_stashes_snapped_range() {
        let mut app = app_with_data(&[0u8; 48]);
        app.cursor_offset = 20;

        let _ = handle_input(&mut app, key('V'));
        let _ = handle_input(&mut app, key(':'));
        assert_eq!(app.mode, Mode::Command);
        assert_eq!(app.pending_range, Some((16, 31)), "pending_range 应为吸附后的整行范围");
        assert!(!app.visual_line, "退出 Visual 应清零 visual_line");
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
