use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::interpret;

/// 数据类型解读浮层：跟随光标行智能定位（优先光标行下方，不足时上方），
/// 水平靠右放置，确保不遮挡光标行及行首内容。
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let endian = if app.type_endian_le { "LE" } else { "BE" };
    let title = format!(
        " Type Inspector [{}] @ 0x{:08X}  e:toggle q/Esc:close ",
        endian, app.cursor_offset
    );

    let rows = interpret::interpret(app.buffer.data(), app.cursor_offset, app.type_endian_le);

    // 面板高度 = 内容行数 + 上下边框；宽度固定 46，受视图宽度钳制。
    // 定位基于 hex 视图的内部区域（去掉外边框一行），与内容实际渲染位置一致。
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let popup = position_popup(inner, app, rows.len() as u16 + 2);

    // 先清除底层内容，再绘制浮层
    frame.render_widget(Clear, popup);

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

/// 计算浮层位置：水平靠右（右边距 1）；垂直优先放在光标行下方（顶边 = 光标行 + 1），
/// 下方空间不足时放到光标行上方（底边 = 光标行 - 1），绝不覆盖光标行。
/// scroll_offset 语义与 hex_view 一致，为行偏移（每行 16 字节）。
fn position_popup(inner: Rect, app: &App, panel_h: u16) -> Rect {
    let panel_w = 46u16.min(inner.width);
    let panel_h = panel_h.min(inner.height);

    let bytes_per_row = 16usize;
    let total_rows = (app.buffer.len() + bytes_per_row - 1) / bytes_per_row;
    let start_row = app.scroll_offset.min(total_rows.saturating_sub(1));
    let cursor_row = (app.cursor_offset / bytes_per_row).saturating_sub(start_row);

    // 水平：靠右，右边距 1，避免遮挡 offset 列和行首字节。
    let x = inner.width.saturating_sub(panel_w + 1);

    // 垂直：优先下方，放不下则上方；再钳制到视图范围内。
    let y = if cursor_row as u16 + 1 + panel_h <= inner.height {
        cursor_row as u16 + 1
    } else {
        (cursor_row as u16).saturating_sub(panel_h + 1)
    }
    .min(inner.height.saturating_sub(panel_h));

    Rect::new(inner.x + x, inner.y + y, panel_w, panel_h)
}
