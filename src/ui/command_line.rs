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
                    Span::styled("?", Style::default().fg(Color::Yellow)),
                    Span::raw(":Help  "),
                    Span::styled("h/j/k/l", Style::default().fg(Color::Yellow)),
                    Span::raw(":Move  "),
                    Span::styled("i", Style::default().fg(Color::Yellow)),
                    Span::raw(":Insert  "),
                    Span::styled("/", Style::default().fg(Color::Yellow)),
                    Span::raw(":Search  "),
                    Span::styled(":", Style::default().fg(Color::Yellow)),
                    Span::raw(":Command  "),
                    Span::styled("v", Style::default().fg(Color::Yellow)),
                    Span::raw(":Visual  "),
                    Span::styled("t", Style::default().fg(Color::Yellow)),
                    Span::raw(":Types  "),
                    Span::styled("Ctrl+W", Style::default().fg(Color::Yellow)),
                    Span::raw(":Panel  "),
                    Span::styled("Ctrl+O/Tab", Style::default().fg(Color::Yellow)),
                    Span::raw(":Back/Fwd"),
                ]);
                let paragraph = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
                frame.render_widget(paragraph, area);
            }
        }
        Mode::Insert => {
            if let Some((msg, _)) = &app.message {
                let line = Line::from(Span::styled(msg, Style::default().fg(Color::Yellow)));
                let paragraph = Paragraph::new(line);
                frame.render_widget(paragraph, area);
            } else {
                let help = Line::from(vec![
                    Span::styled("Esc", Style::default().fg(Color::Yellow)),
                    Span::raw(":Exit  Type hex bytes or ascii chars"),
                ]);
                let paragraph = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
                frame.render_widget(paragraph, area);
            }
        }
        Mode::Replace => {
            if let Some((msg, _)) = &app.message {
                let line = Line::from(Span::styled(msg, Style::default().fg(Color::Yellow)));
                let paragraph = Paragraph::new(line);
                frame.render_widget(paragraph, area);
            } else {
                let help = Line::from(vec![
                    Span::styled("Esc", Style::default().fg(Color::Yellow)),
                    Span::raw(":Exit  Overwrite bytes"),
                ]);
                let paragraph = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
                frame.render_widget(paragraph, area);
            }
        }
        Mode::Visual => {
            if let Some((msg, _)) = &app.message {
                let line = Line::from(Span::styled(msg, Style::default().fg(Color::Yellow)));
                let paragraph = Paragraph::new(line);
                frame.render_widget(paragraph, area);
            } else {
                let help = Line::from(vec![
                    Span::styled("Esc", Style::default().fg(Color::Yellow)),
                    Span::raw(":Cancel  "),
                    Span::styled("y", Style::default().fg(Color::Yellow)),
                    Span::raw(":Yank  "),
                    Span::styled("d", Style::default().fg(Color::Yellow)),
                    Span::raw(":Delete  "),
                    Span::styled("p", Style::default().fg(Color::Yellow)),
                    Span::raw(":Paste"),
                ]);
                let paragraph = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
                frame.render_widget(paragraph, area);
            }
        }
        Mode::Help => {
            let help = Line::from(vec![
                Span::styled("q/Esc", Style::default().fg(Color::Yellow)),
                Span::raw(":Close  "),
                Span::styled("j/k", Style::default().fg(Color::Yellow)),
                Span::raw(":Scroll  "),
                Span::styled("Ctrl+F/B", Style::default().fg(Color::Yellow)),
                Span::raw(":Page  "),
                Span::styled("G", Style::default().fg(Color::Yellow)),
                Span::raw(":Bottom"),
            ]);
            let paragraph = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
            frame.render_widget(paragraph, area);
        }
    }
}
