mod console;
mod dialog;
mod events;
mod header;
mod help;
mod hints;
mod input_bar;
mod primary;
mod secondary;

pub use console::ConsoleWidget;
pub use dialog::DialogWidget;
pub use events::EventsWidget;
pub use header::HeaderWidget;
pub use help::HelpWidget;
pub use hints::HintsWidget;
pub use input_bar::InputBarWidget;
pub use primary::PrimaryWidget;
pub use secondary::SecondaryWidget;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

pub fn pane_block(title: impl Into<String>, focused: bool) -> Block<'static> {
    let style = if focused {
        Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .title(title.into())
        .title_style(style)
        .border_style(style)
}

pub fn scroll_offset(total_lines: usize, height: u16, from_bottom: usize) -> u16 {
    let visible = usize::from(height.saturating_sub(2));
    total_lines
        .saturating_sub(visible)
        .saturating_sub(from_bottom)
        .min(usize::from(u16::MAX)) as u16
}

pub fn nav_scroll(from_bottom: &mut usize, motion: crate::tui::input::Motion) {
    use crate::tui::input::Motion;
    match motion {
        Motion::Up(count) | Motion::Left(count) => *from_bottom = from_bottom.saturating_add(count),
        Motion::Down(count)
        | Motion::Right(count)
        | Motion::NextItem(count)
        | Motion::EndItem(count) => *from_bottom = from_bottom.saturating_sub(count),
        Motion::Top => *from_bottom = usize::MAX,
        Motion::Bottom => *from_bottom = 0,
    }
}

pub fn render_scrollbar(
    frame: &mut Frame,
    area: Rect,
    content_length: usize,
    viewport_length: usize,
    position: usize,
) {
    if content_length <= viewport_length || area.width < 3 || area.height < 3 {
        return;
    }
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .track_style(Style::default().fg(Color::DarkGray))
        .thumb_style(Style::default().fg(Color::LightYellow));
    let mut state = ScrollbarState::new(content_length)
        .position(position)
        .viewport_content_length(viewport_length);
    frame.render_stateful_widget(scrollbar, area, &mut state);
}
