use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use crate::app::{App, Mode};

mod command_line;
mod hex_view;
mod help_view;
mod status_bar;
mod sum_view;
mod type_view;
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

    if app.mode == Mode::Help {
        help_view::draw(frame, layout[0], app);
    } else if app.is_frame_mode() {
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
            app.selection_range(),
        );
    }

    // 类型解读面板浮层（仅 Normal 相关视图下显示）
    if app.mode != Mode::Help && app.type_panel_open {
        type_view::draw(frame, layout[0], app);
    }

    // 校验和浮层（仅 Normal 相关视图下显示）
    if app.mode != Mode::Help && app.sum_open {
        sum_view::draw(frame, layout[0], app);
    }

    status_bar::draw(frame, layout[1], app);
    command_line::draw(frame, layout[2], app);
}
