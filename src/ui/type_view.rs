use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::interpret;

/// 数据类型解读浮层：居中显示光标处字节按各类型的解读结果
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let popup = center_rect(area, 46, 16);

    // 先清除底层内容，再绘制浮层
    frame.render_widget(Clear, popup);

    let endian = if app.type_endian_le { "LE" } else { "BE" };
    let title = format!(
        " Type Inspector [{}] @ 0x{:08X}  e:toggle q/Esc:close ",
        endian, app.cursor_offset
    );

    let rows = interpret::interpret(app.buffer.data(), app.cursor_offset, app.type_endian_le);
    let lines: Vec<Line> = rows
        .into_iter()
        .map(|(label, value)| {
            if value == "--" {
                Line::from(vec![
                    Span::styled(
                        format!("  {:<5}", label),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(value, Style::default().fg(Color::DarkGray)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!("  {:<5}", label),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        value,
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ])
            }
        })
        .collect();

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default());

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup);
}

fn center_rect(rect: Rect, width: u16, height: u16) -> Rect {
    let x = rect.x + (rect.width.saturating_sub(width)) / 2;
    let y = rect.y + (rect.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(rect.width), height.min(rect.height))
}
