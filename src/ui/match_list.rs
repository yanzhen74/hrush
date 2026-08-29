use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::command::pattern_summary;

/// 搜索匹配列表浮层：居中大面板，逐行展示匹配偏移与字节预览；
/// 选中行高亮，列表超出可视高度时滚动（始终保持选中行可见）。
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let matches = &app.search_state.matches;
    let count = matches.len();

    // 居中大浮层：约 70 宽 × 终端高的 60%，小窗口下 min 钳制
    let panel_w = 70u16.min(area.width);
    let panel_h = ((area.height as u32 * 6) / 10).max(4).min(area.height.max(4) as u32) as u16;
    let x = area.x + area.width.saturating_sub(panel_w) / 2;
    let y = area.y + area.height.saturating_sub(panel_h) / 2;
    let popup = Rect::new(x, y, panel_w, panel_h);

    // 先清除底层内容，再绘制浮层
    frame.render_widget(Clear, popup);

    // 边框 2 + 首行摘要 + 末行按键提示 = 3，其余为列表可视行数
    let page = panel_h.saturating_sub(3) as usize;

    // 滚动保持选中行可见
    let mut scroll = app.match_list_scroll.min(count.saturating_sub(1));
    if app.match_list_sel < scroll {
        scroll = app.match_list_sel;
    } else if page > 0 && app.match_list_sel >= scroll + page {
        scroll = app.match_list_sel + 1 - page;
    }

    // 首行：模式摘要 + 匹配数
    let header = match app.search_state.pattern.as_ref() {
        Some(p) => format!(" Matches: {} ({})", pattern_summary(p), count),
        None => format!(" Matches ({})", count),
    };

    // 间距列：与下一个匹配偏移的差值（右对齐整列），末尾匹配显示 "--"
    let gap_w = (0..count)
        .map(|i| format_gap(match_gap(matches, i)).len())
        .max()
        .unwrap_or(2);
    // 预览字节数：浮层宽度扣除前缀后按每字节 3 列计算，上限 16、下限 8 字节
    let inner_w = panel_w.saturating_sub(2) as usize;
    let prefix_w = 1 + 1 + 10 + 1 + gap_w + 2; // marker + 空格 + 地址 + 空格 + 间距 + 两个空格
    let max_preview = if inner_w > prefix_w {
        (inner_w - prefix_w + 1) / 3
    } else {
        8
    };
    let preview_bytes = max_preview.clamp(8, 16);

    let mut lines: Vec<Line> = Vec::with_capacity(page + 2);
    lines.push(Line::from(Span::styled(
        header,
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));

    if count == 0 {
        lines.push(Line::from(Span::styled(
            "  No matches",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (idx, &offset) in matches[scroll..].iter().enumerate().take(page) {
            let i = scroll + idx;
            // 匹配处起若干字节预览（列宽不足时缩减，但至少 8 字节）
            let n = preview_bytes.min(app.buffer.len().saturating_sub(offset));
            let preview = if n == 0 {
                "--".to_string()
            } else {
                app.buffer
                    .get_range(offset, n)
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let sel_style = if i == app.match_list_sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let marker = if i == app.match_list_sel { ">" } else { " " };
            let gap_str = format!(
                "{:>width$}",
                format_gap(match_gap(matches, i)),
                width = gap_w
            );
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!(" 0x{:08X} {}  {}", offset, gap_str, preview),
                    sel_style,
                ),
            ]));
        }
    }

    // 末行：按键提示
    lines.push(Line::from(Span::styled(
        " [Enter] goto  [↑↓] browse  [q/Esc] close",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(" Match List ")
        .borders(Borders::ALL)
        .style(Style::default());

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup);
}

/// 当前匹配与下一个匹配的偏移差值；最后一个匹配返回 None
fn match_gap(matches: &[usize], idx: usize) -> Option<usize> {
    matches.get(idx + 1).map(|&next| next - matches[idx])
}

/// 间距格式化：`+0x` 前缀大写十六进制（至少 4 位）；无下一个匹配显示 "--"
fn format_gap(gap: Option<usize>) -> String {
    match gap {
        Some(g) => format!("+0x{:04X}", g),
        None => "--".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_between_two_matches_is_offset_difference() {
        let matches = vec![0x1234usize, 0x1A34];
        assert_eq!(match_gap(&matches, 0), Some(0x0800));
        assert_eq!(format_gap(match_gap(&matches, 0)), "+0x0800");
    }

    #[test]
    fn single_match_gap_shows_placeholder() {
        let matches = vec![0x1234usize];
        assert_eq!(match_gap(&matches, 0), None);
        assert_eq!(format_gap(match_gap(&matches, 0)), "--");
    }
}
