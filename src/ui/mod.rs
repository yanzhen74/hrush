use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use crate::app::App;

mod command_line;
mod hex_view;
mod status_bar;
pub mod frame_view;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Hex,
    Ascii,
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    if app.is_frame_mode() {
        frame_view::render_frame_view(frame, layout[0], app);
    } else {
        let buffer = &app.buffer;
        hex_view::draw(
            frame,
            layout[0],
            buffer,
            app.cursor_offset,
            app.active_panel,
            app.scroll_offset,
            &app.search_state,
        );
    }

    status_bar::draw(frame, layout[1], app);
    command_line::draw(frame, layout[2], app);
}
