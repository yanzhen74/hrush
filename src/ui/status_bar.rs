use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Mode};

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let mode_text = match app.mode {
        Mode::Normal => (" NORMAL ", Color::Blue),
        Mode::Insert => (" INSERT ", Color::Green),
        Mode::Replace => (" REPLACE ", Color::Red),
        Mode::Command => (" COMMAND ", Color::Yellow),
        Mode::Search => (" SEARCH ", Color::Magenta),
    };

    let file_name = app
        .buffer
        .file_name()
        .unwrap_or_else(|| "[No Name]".to_string());
    let size_str = format_size(app.buffer.len());
    let offset_hex = format!("0x{:08X}", app.cursor_offset);
    let offset_dec = format!("{}", app.cursor_offset);
    let offset_str = format!("{} ({})", offset_hex, offset_dec);

    let mut spans = vec![
        Span::styled(
            mode_text.0,
            Style::default()
                .bg(mode_text.1)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::raw(file_name),
        Span::raw(" | "),
        Span::raw(size_str),
        Span::raw(" | "),
        Span::raw(offset_str),
    ];

    if app.buffer.is_dirty() {
        spans.push(Span::styled(" [+]", Style::default().fg(Color::Yellow)));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn format_size(size: usize) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
