use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Mode};

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    match app.mode {
        Mode::Command => {
            let line = Line::from(vec![
                Span::styled(":", Style::default().fg(Color::Yellow)),
                Span::raw(&app.command_input),
            ]);
            let paragraph = Paragraph::new(line);
            frame.render_widget(paragraph, area);

            let cursor_x = area.x + 1 + app.command_input.len() as u16;
            let cursor_y = area.y;
            frame.set_cursor_position((cursor_x, cursor_y));
        }
        Mode::Search => {
            let line = Line::from(vec![
                Span::styled("/", Style::default().fg(Color::Yellow)),
                Span::raw(&app.search_input),
            ]);
            let paragraph = Paragraph::new(line);
            frame.render_widget(paragraph, area);

            let cursor_x = area.x + 1 + app.search_input.len() as u16;
            let cursor_y = area.y;
            frame.set_cursor_position((cursor_x, cursor_y));
        }
        Mode::Normal => {
            if let Some((msg, _)) = &app.message {
                let line = Line::from(Span::styled(msg, Style::default().fg(Color::Yellow)));
                let paragraph = Paragraph::new(line);
                frame.render_widget(paragraph, area);
            } else {
                let help = Line::from(vec![
                    Span::styled("h/j/k/l", Style::default().fg(Color::Yellow)),
                    Span::raw(" move | "),
                    Span::styled(":", Style::default().fg(Color::Yellow)),
                    Span::raw(" command | "),
                    Span::styled("Tab", Style::default().fg(Color::Yellow)),
                    Span::raw(" switch panel"),
                ]);
                let paragraph = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
                frame.render_widget(paragraph, area);
            }
        }
        _ => {
            frame.render_widget(Paragraph::new(""), area);
        }
    }
}
