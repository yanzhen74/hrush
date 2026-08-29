use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;

/// 校验和浮层：居中小面板，展示打开 :sum 时快照的计算范围与各类校验和。
/// 结果在打开时一次性计算（sum_info），绘制不重复计算，大文件选区也流畅。
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let info = match app.sum_info.as_ref() {
        Some(info) => info,
        None => return,
    };

    let (start, end) = info.range;

    // 宽度 89 = 边框 2 + 缩进 2 + 标签列 19 + ": " 2 + 64 位 hex，保证最长一行
    // （SHA256）不折行；标签列按最长的 "CRC16(CCITT-FALSE)" 对齐；小窗口下 min 钳制
    let panel_w = 89u16.min(area.width);
    let panel_h = 11u16.min(area.height);
    let x = area.x + area.width.saturating_sub(panel_w) / 2;
    let y = area.y + area.height.saturating_sub(panel_h) / 2;
    let popup = Rect::new(x, y, panel_w, panel_h);

    // 先清除底层内容，再绘制浮层
    frame.render_widget(Clear, popup);

    let label_style = Style::default().fg(Color::Cyan);
    let value_style = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
    let endian = if info.sum_le { "LE" } else { "BE" };

    let lines = vec![
        Line::from(Span::styled(
            format!(" Checksum — 0x{:X}..0x{:X} ({} bytes)", start, end, info.len),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(format!("  {:<19}: ", "CRC16(CCITT-FALSE)"), label_style),
            Span::styled(info.crc16.clone(), value_style),
        ]),
        Line::from(vec![
            Span::styled(format!("  {:<19}: ", "CRC32(IEEE)"), label_style),
            Span::styled(info.crc32.clone(), value_style),
        ]),
        Line::from(vec![
            Span::styled(format!("  {:<19}: ", "MD5"), label_style),
            Span::styled(info.md5.clone(), value_style),
        ]),
        Line::from(vec![
            Span::styled(format!("  {:<19}: ", "SHA256"), label_style),
            Span::styled(info.sha256.clone(), value_style),
        ]),
        Line::from(vec![
            Span::styled(format!("  {:<19}: ", "SUM8"), label_style),
            Span::styled(info.sum8.clone(), value_style),
        ]),
        Line::from(vec![
            Span::styled(format!("  {:<19}: ", format!("SUM16({})", endian)), label_style),
            Span::styled(info.sum16.clone(), value_style),
        ]),
        Line::from(vec![
            Span::styled(format!("  {:<19}: ", format!("SUM32({})", endian)), label_style),
            Span::styled(info.sum32.clone(), value_style),
        ]),
        Line::from(Span::styled(
            " [q/Esc close]",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .title(" Checksum ")
        .borders(Borders::ALL)
        .style(Style::default());

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup);
}
