use ratatui::Frame as RatatuiFrame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

pub fn render_frame_view(f: &mut RatatuiFrame, area: Rect, app: &App) {
    let block = Block::default()
        .title(" Frame View ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.buffer.is_empty() {
        let hint = Paragraph::new(Line::from(vec![
            Span::raw("No file loaded. Press "),
            Span::styled(":", Style::default().fg(Color::Yellow)),
            Span::raw(" to enter command mode."),
        ]))
        .style(Style::default().fg(Color::Gray));
        let hint_area = center_rect(inner, 42, 1);
        f.render_widget(hint, hint_area);
        return;
    }

    let frame_index = match &app.frame_index {
        Some(fi) => fi,
        None => {
            let hint = Paragraph::new("No frame index. Use :frame command.")
                .style(Style::default().fg(Color::Gray));
            let hint_area = center_rect(inner, 30, 1);
            f.render_widget(hint, hint_area);
            return;
        }
    };

    let total_frames = frame_index.frames.len();
    if total_frames == 0 {
        return;
    }

    // 计算行头固定宽度: "#NNNN @XXXXXXXX LXXXX | "
    let max_len = frame_index.frames.iter().map(|fr| fr.length).max().unwrap_or(0);
    let len_digits = if max_len == 0 { 1 } else { max_len.to_string().len() };
    let header_width = (20 + len_digits) as u16;

    let visible_rows = inner.height as usize;
    let start_frame = app.scroll_offset.min(total_frames.saturating_sub(1));
    let end_frame = (start_frame + visible_rows.saturating_sub(2)).min(total_frames);

    // 光标所在帧索引
    let cursor_frame_idx = app.current_frame_number();

    let data_width = inner.width.saturating_sub(header_width);
    let visible_bytes = (data_width as usize) / 3;

    let mut lines = Vec::new();

    let data_chars_len = if visible_bytes == 0 { 0 } else { visible_bytes * 3 - 1 };

    // 第一行：刻度线
    let mut tick_line = " ".repeat(header_width as usize);
    for i in 0..visible_bytes {
        let byte_idx = app.h_scroll_offset + i;
        let mark = if byte_idx % 10 == 0 {
            "|"
        } else if byte_idx % 5 == 0 {
            ":"
        } else {
            "."
        };
        tick_line.push_str(mark);
        if i + 1 < visible_bytes {
            tick_line.push_str("  ");
        }
    }
    lines.push(Line::from(Span::styled(
        tick_line,
        Style::default().fg(Color::Gray),
    )));

    // 第二行：数字
    let mut num_chars = vec![' '; data_chars_len];
    for i in 0..visible_bytes {
        let byte_idx = app.h_scroll_offset + i;
        if byte_idx % 10 == 0 {
            let start_pos = i * 3;
            let num_str = byte_idx.to_string();
            for (j, c) in num_str.chars().enumerate() {
                if start_pos + j < data_chars_len {
                    num_chars[start_pos + j] = c;
                }
            }
        }
    }
    let num_line = " ".repeat(header_width as usize) + &num_chars.into_iter().collect::<String>();
    lines.push(Line::from(Span::styled(
        num_line,
        Style::default().fg(Color::Gray),
    )));

    // Frame rows
    for frame_idx in start_frame..end_frame {
        let frame = &frame_index.frames[frame_idx];
        let is_cursor_row = cursor_frame_idx == Some(frame_idx);
        let base_bg = if is_cursor_row {
            Some(Color::Indexed(236))
        } else {
            None
        };

        // 行头
        let header_text = format!(
            "#{:04} @{:08X} L{:>width$} | ",
            frame_idx + 1,
            frame.offset,
            frame.length,
            width = len_digits
        );
        let mut spans = vec![Span::styled(
            header_text,
            Style::default().fg(Color::Gray).bg(base_bg.unwrap_or(Color::Reset)),
        )];

        for i in 0..visible_bytes {
            let byte_offset = frame.offset + app.h_scroll_offset + i;
            let space_style =
                Style::default().fg(Color::White).bg(base_bg.unwrap_or(Color::Reset));

            if byte_offset >= frame.offset + frame.length {
                // 超出当前帧长度，显示空白占位
                spans.push(Span::styled("  ", space_style));
                if i + 1 < visible_bytes {
                    spans.push(Span::styled(" ", space_style));
                }
                continue;
            }

            match app.buffer.get_byte(byte_offset) {
                Some(byte) => {
                    let is_modified = app.buffer.is_modified(byte_offset);
                    let is_cursor_byte = app.cursor_offset == byte_offset;
                    let is_search_match = app.search_state.is_match_byte(byte_offset);
                    let is_current_match = app.search_state.is_current_match_byte(byte_offset);

                    let (fg, bg) = if is_cursor_byte {
                        (Color::Black, Some(Color::White))
                    } else if is_current_match {
                        (Color::White, Some(Color::Indexed(214)))
                    } else if is_search_match {
                        (Color::White, Some(Color::Indexed(130)))
                    } else {
                        let fg = if is_modified { Color::Yellow } else { Color::White };
                        (fg, base_bg)
                    };

                    let style = Style::default().fg(fg).bg(bg.unwrap_or(Color::Reset));
                    spans.push(Span::styled(format!("{:02X}", byte), style));
                    if i + 1 < visible_bytes {
                        spans.push(Span::styled(" ", space_style));
                    }
                }
                None => {
                    // 超出缓冲区
                    spans.push(Span::styled("  ", space_style));
                    if i + 1 < visible_bytes {
                        spans.push(Span::styled(" ", space_style));
                    }
                }
            }
        }

        lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn center_rect(rect: Rect, width: u16, height: u16) -> Rect {
    let x = rect.x + (rect.width.saturating_sub(width)) / 2;
    let y = rect.y + (rect.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(rect.width), height.min(rect.height))
}
