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
use crate::search::SearchState;
use crate::ui::{self, Panel};
use crate::undo::UndoManager;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Replace,
    Command,
    Search,
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
        }
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

            // 非 Insert 模式下光标不能超出缓冲区末尾
            if self.mode != Mode::Insert && !self.buffer.is_empty() && self.cursor_offset >= self.buffer.len() {
                self.cursor_offset = self.buffer.len().saturating_sub(1);
            }

            terminal.draw(|frame| ui::draw(frame, self))?;
            self.handle_events()?;

            // 事件处理后再更新 scroll_offset，确保 cursor_offset 变化后立即同步
            let cursor_row = self.cursor_offset / 16;
            if cursor_row < self.scroll_offset {
                self.scroll_offset = cursor_row;
            } else if cursor_row >= self.scroll_offset + self.visible_rows.saturating_sub(1).max(1) {
                self.scroll_offset = cursor_row.saturating_sub(self.visible_rows.saturating_sub(2).max(1));
            }
        }
        Ok(())
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
