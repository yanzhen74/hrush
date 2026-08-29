use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::buffer::Buffer;
use crate::checksum::ChecksumInfo;
use crate::search::SearchState;
use crate::ui::{self, Panel};
use crate::undo::UndoManager;
use crate::frame::{ViewMode, FrameIndex, Frame, frame_at_offset};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Replace,
    Command,
    Search,
    Visual,
    Help,
}

/// 上一次修改的类型与内容（用于 `.` 命令重放）
#[derive(Clone, Debug, PartialEq)]
pub enum LastChange {
    Insert { bytes: Vec<u8> },      // i/a 会话插入的字节序列
    Overwrite { bytes: Vec<u8> },   // R 会话覆盖写入的字节序列
    ReplaceByte { value: u8 },      // r 单字节替换
    Delete { len: usize },          // x / dd / Visual d,x 删除的长度
    Paste,                          // p 粘贴（重放时用当前 yank_buffer）
}

pub struct App {
    pub running: bool,
    pub mode: Mode,
    pub command_input: String,
    pub buffer: Buffer,
    pub cursor_offset: usize,
    pub active_panel: Panel,
    pub scroll_offset: usize,
    pub message: Option<(String, Instant)>,
    pub undo_manager: UndoManager,
    pub nibble_input: Option<u8>,
    pub pending_key: Option<char>,
    pub visible_rows: usize,
    pub search_state: SearchState,
    pub search_input: String,
    pub view_mode: ViewMode,
    pub frame_index: Option<FrameIndex>,
    pub h_scroll_offset: usize,
    pub frame_original_len: Option<usize>,
    pub visible_bytes: usize,
    pub insert_after: bool,
    pub count_prefix: Option<usize>,
    pub visual_anchor: Option<usize>,
    pub yank_buffer: Vec<u8>,
    pub help_scroll: usize,
    pub help_topic: Option<String>,
    pub jump_back: Vec<usize>,
    pub jump_forward: Vec<usize>,
    pub type_panel_open: bool,
    pub type_endian_le: bool,
    pub command_history: Vec<String>,
    pub search_history: Vec<String>,
    pub history_index: Option<usize>,
    pub last_change: Option<LastChange>,
    pub change_start: Option<usize>,
    pub pending_range: Option<(usize, usize)>,
    pub sum_open: bool,
    pub sum_info: Option<ChecksumInfo>,
    pub match_list_open: bool,
    pub match_list_sel: usize,    // 匹配列表当前选中行
    pub match_list_scroll: usize, // 匹配列表滚动偏移
}

impl App {
    pub fn new() -> Self {
        Self {
            running: true,
            mode: Mode::Normal,
            command_input: String::new(),
            buffer: Buffer::new(),
            cursor_offset: 0,
            active_panel: Panel::Hex,
            scroll_offset: 0,
            message: None,
            undo_manager: UndoManager::new(),
            nibble_input: None,
            pending_key: None,
            visible_rows: 1,
            search_state: SearchState::new(),
            search_input: String::new(),
            view_mode: ViewMode::default(),
            frame_index: None,
            h_scroll_offset: 0,
            frame_original_len: None,
            visible_bytes: 0,
            insert_after: false,
            count_prefix: None,
            visual_anchor: None,
            yank_buffer: Vec::new(),
            help_scroll: 0,
            help_topic: None,
            jump_back: Vec::new(),
            jump_forward: Vec::new(),
            type_panel_open: false,
            type_endian_le: true,
            command_history: Vec::new(),
            search_history: Vec::new(),
            history_index: None,
            last_change: None,
            change_start: None,
            pending_range: None,
            sum_open: false,
            sum_info: None,
            match_list_open: false,
            match_list_sel: 0,
            match_list_scroll: 0,
        }
    }

    /// 在执行大跳跃（:goto、搜索跳转、gg/G 等）前调用：
    /// 把当前光标位置压入后退栈，并清空前进栈（避免连续重复入栈）
    pub fn push_jump(&mut self) {
        if let Some(&last) = self.jump_back.last() {
            if last == self.cursor_offset {
                return;
            }
        }
        self.jump_back.push(self.cursor_offset);
        self.jump_forward.clear();
    }

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.visual_anchor.map(|anchor| (anchor.min(self.cursor_offset), anchor.max(self.cursor_offset)))
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        while self.running {
            if let Some((_, instant)) = self.message.as_ref() {
                if instant.elapsed() > Duration::from_secs(3) {
                    self.message = None;
                }
            }

            let size = terminal.size()?;
            self.visible_rows = size.height.saturating_sub(4).max(1) as usize;

            // 帧模式下计算每行可见字节数（用于水平滚动同步）
            if self.is_frame_mode() {
                if let Some(fi) = &self.frame_index {
                    let max_len = fi.frames.iter().map(|f| f.length).max().unwrap_or(0);
                    let len_digits = if max_len == 0 { 1 } else { max_len.to_string().len() };
                    let header_width = (20 + len_digits) as u16;
                    let data_width = (size.width.saturating_sub(2)).saturating_sub(header_width);
                    self.visible_bytes = (data_width as usize) / 3;
                }
            }

            // 非 Insert 模式下光标不能超出缓冲区末尾
            if self.mode != Mode::Insert && !self.buffer.is_empty() && self.cursor_offset >= self.buffer.len() {
                self.cursor_offset = self.buffer.len().saturating_sub(1);
            }

            // 检查异步搜索是否完成（收集结果后刷新打开中的匹配列表）
            if self.poll_search_result() {
                // 搜索完成，自动跳转到第一个匹配（跳转前记录原位置到 jumplist）
                if let Some(offset) = self.search_state.first_match() {
                    if offset != self.cursor_offset {
                        self.push_jump();
                    }
                    self.cursor_offset = offset;
                } else if !self.search_state.matches.is_empty() {
                    if let Some(offset) = self.search_state.next_match(self.cursor_offset) {
                        self.push_jump();
                        self.cursor_offset = offset;
                    }
                }
            }
            
            terminal.draw(|frame| ui::draw(frame, self))?;
            self.handle_events()?;

            // 切入 Insert/Command 等非 Normal 模式时自动关闭浮层面板（类型解读/校验和）
            if self.mode != Mode::Normal && self.type_panel_open {
                self.type_panel_open = false;
            }
            if self.mode != Mode::Normal && self.sum_open {
                self.sum_open = false;
            }
            if self.mode != Mode::Normal && self.match_list_open {
                self.match_list_open = false;
            }

            // 事件处理后再更新 scroll_offset，确保 cursor_offset 变化后立即同步
            if self.is_frame_mode() {
                // 帧模式：垂直滚动确保当前帧可见
                if let Some(frame_num) = self.current_frame_number() {
                    if frame_num < self.scroll_offset {
                        self.scroll_offset = frame_num;
                    } else if frame_num >= self.scroll_offset + (self.visible_rows.saturating_sub(2).max(1)).saturating_sub(1).max(1) {
                        self.scroll_offset = frame_num.saturating_sub((self.visible_rows.saturating_sub(2).max(1)).saturating_sub(1).max(1));
                    }
                }
                // 帧模式：水平滚动确保光标字节可见
                if let Some(frame) = self.current_frame() {
                    let frame_col = self.cursor_offset.saturating_sub(frame.offset);
                    let visible_bytes = self.visible_bytes.max(1);
                    if frame_col < self.h_scroll_offset {
                        self.h_scroll_offset = frame_col;
                    } else if frame_col >= self.h_scroll_offset + visible_bytes {
                        self.h_scroll_offset = frame_col.saturating_sub(visible_bytes.saturating_sub(1));
                    }
                }
            } else {
                let cursor_row = self.cursor_offset / 16;
                if cursor_row < self.scroll_offset {
                    self.scroll_offset = cursor_row;
                } else if cursor_row >= self.scroll_offset + self.visible_rows.saturating_sub(1).max(1) {
                    self.scroll_offset = cursor_row.saturating_sub(self.visible_rows.saturating_sub(2).max(1));
                }
            }
        }
        Ok(())
    }

    /// 是否有异步搜索正在执行
    pub fn is_searching(&self) -> bool {
        self.search_state.is_searching()
    }

    /// 检查异步搜索是否完成并收集结果；
    /// 新搜索完成时重置匹配列表选中/滚动（列表打开时刷新显示新结果）
    pub fn poll_search_result(&mut self) -> bool {
        if self.search_state.poll_result() {
            if self.match_list_open {
                self.match_list_sel = 0;
                self.match_list_scroll = 0;
            }
            true
        } else {
            false
        }
    }

    /// 是否处于帧模式
    pub fn is_frame_mode(&self) -> bool {
        matches!(self.view_mode, ViewMode::Frame)
    }

    /// 获取光标所在帧的索引号
    pub fn current_frame_number(&self) -> Option<usize> {
        self.frame_index.as_ref().and_then(|fi| frame_at_offset(fi, self.cursor_offset))
    }

    /// 获取光标所在帧的引用
    pub fn current_frame(&self) -> Option<&Frame> {
        self.current_frame_number().and_then(|idx| {
            self.frame_index.as_ref().map(|fi| &fi.frames[idx])
        })
    }

    fn handle_events(&mut self) -> Result<()> {
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                let _ = crate::input::handle_input(self, key);
            }
        }
        Ok(())
    }
}

pub fn run(file: Option<String>, import: Option<String>) -> Result<()> {
    let mut app = App::new();

    if let Some(path) = file {
        app.buffer = Buffer::from_file(Path::new(&path))?;
    } else if let Some(path) = import {
        app.buffer = Buffer::from_hex_import(Path::new(&path))?;
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app.run(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}
