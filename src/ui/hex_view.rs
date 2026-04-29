use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::buffer::Buffer;
use crate::search::SearchState;
use crate::ui::Panel;

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    buffer: &Buffer,
    cursor_offset: usize,
    active_panel: Panel,
    scroll_offset: usize,
    search_state: &SearchState,
) {
    let block = Block::default()
        .title(" Hex View ")
        .borders(Borders::ALL);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(10), // Offset
            Constraint::Min(30),    // Hex
            Constraint::Length(16), // ASCII
        ])
        .split(inner);

    if buffer.is_empty() {
        let hint = Paragraph::new(Line::from(vec![
            Span::raw("No file loaded. Press "),
            Span::styled(":", Style::default().fg(Color::Yellow)),
            Span::raw(" to enter command mode."),
        ]))
        .style(Style::default().fg(Color::Gray));

        let hint_area = center_rect(inner, 42, 1);
        frame.render_widget(hint, hint_area);
        return;
    }

    let visible_rows = inner.height as usize;
    let total_rows = (buffer.len() + 15) / 16;
    let cursor_row = cursor_offset / 16;

    let start_row = scroll_offset.min(total_rows.saturating_sub(1));
    let end_row = (start_row + visible_rows).min(total_rows);

    let mut offset_lines = Vec::new();
    let mut hex_lines = Vec::new();
    let mut ascii_lines = Vec::new();

    for row in start_row..end_row {
        let base_offset = row * 16;
        let is_cursor_row = row == cursor_row;
        let base_bg = if is_cursor_row {
            Some(Color::Rgb(50, 50, 50))
        } else {
            None
        };
        let default_space_style = Style::default().bg(base_bg.unwrap_or(Color::Reset));
        let default_empty_style = Style::default().bg(base_bg.unwrap_or(Color::Reset));

        // Offset
        let offset_style =
            Style::default().fg(Color::Gray).bg(base_bg.unwrap_or(Color::Reset));
        offset_lines.push(Line::from(Span::styled(
            format!("{:08X}", base_offset),
            offset_style,
        )));

        // Hex and ASCII
        let mut hex_spans = Vec::new();
        let mut ascii_spans = Vec::new();

        for col in 0..16 {
            let offset = base_offset + col;
            if let Some(byte) = buffer.get_byte(offset) {
                let is_modified = buffer.is_modified(offset);
                let is_cursor_byte = cursor_offset == offset;

                let is_search_match = search_state.is_match_byte(offset);
                let is_current_match = search_state.is_current_match_byte(offset);

                let (hex_fg, hex_bg) = if is_cursor_byte && active_panel == Panel::Hex {
                    (Color::Black, Some(Color::White))
                } else if is_current_match {
                    (Color::White, Some(Color::Rgb(255, 165, 0)))
                } else if is_search_match {
                    (Color::White, Some(Color::Rgb(180, 100, 0)))
                } else {
                    let fg = if is_modified { Color::Yellow } else { Color::White };
                    (fg, base_bg)
                };

                let (ascii_fg, ascii_bg) = if is_cursor_byte && active_panel == Panel::Ascii {
                    (Color::Black, Some(Color::White))
                } else if is_current_match {
                    (Color::White, Some(Color::Rgb(255, 165, 0)))
                } else if is_search_match {
                    (Color::White, Some(Color::Rgb(180, 100, 0)))
                } else {
                    let fg = if is_modified { Color::Yellow } else { Color::White };
                    (fg, base_bg)
                };

                let hex_style = Style::default().fg(hex_fg).bg(hex_bg.unwrap_or(Color::Reset));
                let ascii_style =
                    Style::default().fg(ascii_fg).bg(ascii_bg.unwrap_or(Color::Reset));

                hex_spans.push(Span::styled(format!("{:02X}", byte), hex_style));
                if col < 15 {
                    hex_spans.push(Span::styled(" ", default_space_style));
                }

                let ch = if byte.is_ascii_graphic() || byte == b' ' {
                    byte as char
                } else {
                    '.'
                };
                ascii_spans.push(Span::styled(ch.to_string(), ascii_style));
            } else {
                hex_spans.push(Span::styled("  ", default_empty_style));
                if col < 15 {
                    hex_spans.push(Span::styled(" ", default_empty_style));
                }
                ascii_spans.push(Span::styled(" ", default_empty_style));
            }
        }

        hex_lines.push(Line::from(hex_spans));
        ascii_lines.push(Line::from(ascii_spans));
    }

    // Offset column
    let offset_block = Block::default()
        .title(" Offset ")
        .borders(Borders::RIGHT);
    let offset_para = Paragraph::new(offset_lines).block(offset_block);
    frame.render_widget(offset_para, columns[0]);

    // Hex column
    let hex_block = Block::default()
        .title(" Hex ")
        .borders(Borders::RIGHT);
    let hex_para = Paragraph::new(hex_lines).block(hex_block);
    frame.render_widget(hex_para, columns[1]);

    // ASCII column
    let ascii_block = Block::default()
        .title(" ASCII ")
        .borders(Borders::NONE);
    let ascii_para = Paragraph::new(ascii_lines).block(ascii_block);
    frame.render_widget(ascii_para, columns[2]);
}

fn center_rect(rect: Rect, width: u16, height: u16) -> Rect {
    let x = rect.x + (rect.width.saturating_sub(width)) / 2;
    let y = rect.y + (rect.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(rect.width), height.min(rect.height))
}
