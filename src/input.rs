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

    match app.mode {
        Mode::Normal => handle_normal_mode(app, key),
        Mode::Insert => handle_insert_mode(app, key),
        Mode::Replace => handle_replace_mode(app, key),
        Mode::Command => handle_command_mode(app, key),
        Mode::Search => handle_search_mode(app, key),
    }

    Ok(())
}

fn handle_pending_key(app: &mut App, key: KeyEvent) {
    let pending = app.pending_key.take().unwrap();

    match pending {
        'g' => {
            if key.code == KeyCode::Char('g') {
                if app.is_frame_mode() {
                    if let Some(fi) = &app.frame_index {
                        if !fi.frames.is_empty() {
                            let current_frame_num = app.current_frame_number().unwrap_or(0);
                            let current_frame = &fi.frames[current_frame_num];
                            let col = app.cursor_offset.saturating_sub(current_frame.offset);
                            let first_frame = &fi.frames[0];
                            let target_col = col.min(first_frame.length.saturating_sub(1));
                            app.cursor_offset = first_frame.offset + target_col;
                            app.scroll_offset = 0;
                        }
                    }
                } else {
                    app.cursor_offset = 0;
                }
            } else {
                // 不是 gg，将当前键作为普通键处理
                handle_normal_mode(app, key);
            }
        }
        'd' => {
            if key.code == KeyCode::Char('d') {
                delete_line(app);
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

        // 模式切换
        KeyCode::Char('i') => {
            app.mode = Mode::Insert;
            app.nibble_input = None;
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
                app.cursor_offset = offset;
            }
        }
        KeyCode::Char('N') => {
            let start = app.cursor_offset.saturating_sub(1);
            if let Some(offset) = app.search_state.prev_match(start) {
                app.cursor_offset = offset;
            }
        }

        // 移动
        KeyCode::Char('h') | KeyCode::Left => {
            move_cursor_left(app);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            move_cursor_right(app);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            move_cursor_up(app);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            move_cursor_down(app);
        }

        // 快速移动
        KeyCode::Char('G') => {
            if !app.buffer.is_empty() {
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
                editor::remove_byte(app, app.cursor_offset);
                clamp_cursor(app);
            }
        }

        // 撤销 / 重做
        KeyCode::Char('u') => {
            editor::undo(app);
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor::redo(app);
        }

        // 面板切换
        KeyCode::Tab => {
            app.active_panel = match app.active_panel {
                Panel::Hex => Panel::Ascii,
                Panel::Ascii => Panel::Hex,
            };
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
}

fn handle_insert_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
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
                    app.search_state.search(&app.buffer, pattern);
                    if let Some(offset) = app.search_state.next_match(app.cursor_offset) {
                        app.cursor_offset = offset;
                    }
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
            app.mode = Mode::Normal;
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

fn move_cursor_left(app: &mut App) {
    if app.cursor_offset > 0 {
        app.cursor_offset -= 1;
    }
}

fn move_cursor_right(app: &mut App) {
    if !app.buffer.is_empty() && app.cursor_offset < app.buffer.len().saturating_sub(1) {
        app.cursor_offset += 1;
    }
}

fn move_cursor_up(app: &mut App) {
    if app.cursor_offset >= 16 {
        app.cursor_offset -= 16;
    }
}

fn move_cursor_down(app: &mut App) {
    if app.cursor_offset + 16 < app.buffer.len() {
        app.cursor_offset += 16;
    }
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
            true
        }
        KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let visible_bytes = app.visible_bytes.max(1);
            app.h_scroll_offset = app.h_scroll_offset.saturating_sub(visible_bytes);
            true
        }
        KeyCode::Char('h') | KeyCode::Left => {
            if app.cursor_offset > current_frame.offset {
                app.cursor_offset -= 1;
                sync_h_scroll(app);
            }
            true
        }
        KeyCode::Char('l') | KeyCode::Right => {
            let frame_end = current_frame.offset + current_frame.length.saturating_sub(1);
            if app.cursor_offset < frame_end {
                app.cursor_offset += 1;
                sync_h_scroll(app);
            }
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if current_frame_num + 1 < frame_index.frames.len() {
                let col = app.cursor_offset.saturating_sub(current_frame.offset);
                let next_frame = &frame_index.frames[current_frame_num + 1];
                let next_col = col.min(next_frame.length.saturating_sub(1));
                app.cursor_offset = next_frame.offset + next_col;
                sync_v_scroll(app);
            }
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if current_frame_num > 0 {
                let col = app.cursor_offset.saturating_sub(current_frame.offset);
                let prev_frame = &frame_index.frames[current_frame_num - 1];
                let prev_col = col.min(prev_frame.length.saturating_sub(1));
                app.cursor_offset = prev_frame.offset + prev_col;
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
                app.cursor_offset = last_frame.offset + target_col;
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
        } else if frame_num >= app.scroll_offset + app.visible_rows.saturating_sub(2).max(1) {
            app.scroll_offset = frame_num.saturating_sub(app.visible_rows.saturating_sub(2).max(1));
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
