use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::help::{HELP_SECTIONS, find_section_index, section_start_line};

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    // 构建帮助文本行
    let mut lines: Vec<Line> = Vec::new();

    for section in HELP_SECTIONS.iter() {
        // section 标题：粗体黄色
        lines.push(Line::from(Span::styled(
            section.title,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from("")); // 空行

        for entry in section.entries {
            if entry.key.is_empty() {
                lines.push(Line::from(Span::raw(format!("  {}", entry.description))));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {:<20}", entry.key),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(entry.description),
                ]));
            }
        }

        lines.push(Line::from("")); // section 间空行
    }

    // 计算实际滚动偏移：如果 help_topic 有值且 help_scroll == 0，自动跳转
    let mut scroll = app.help_scroll;
    if let Some(ref topic) = app.help_topic {
        if app.help_scroll == 0 {
            if let Some(idx) = find_section_index(topic) {
                scroll = section_start_line(idx);
            }
        }
    }

    let block = Block::default()
        .title(" Help (q/Esc to close) ")
        .borders(Borders::ALL)
        .style(Style::default());

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((scroll as u16, 0));

    frame.render_widget(paragraph, area);
}
