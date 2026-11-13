mod console;
mod dialog;
mod events;
mod header;
mod help;
mod hints;
mod input_bar;
mod primary;
mod secondary;
mod theme;

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
    layout::{Margin, Rect},
    style::Style,
    widgets::{Block, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState, block::Padding},
};

pub fn pane_block(title: impl Into<String>, focused: bool, width: u16) -> Block<'static> {
    let mut title = title.into();
    let maximum = width.saturating_sub(4) as usize;
    if title.chars().count() > maximum {
        title = title.chars().take(maximum).collect();
        title.push(' ');
    }
    let style = if focused {
        theme::focused()
    } else {
        theme::muted()
    };
    Block::default()
        .borders(Borders::ALL)
        .padding(Padding::right(1))
        .title(title)
        .title_style(style)
        .border_style(style)
}

/// Returns the first visible line of a bottom-anchored pane with `height` rows
/// including borders. `from_bottom` is clamped to the scrollable range so that
/// motions after `gg` or past either end take effect immediately.
pub fn scroll_offset(total_lines: usize, height: u16, from_bottom: &mut usize) -> u16 {
    let visible = usize::from(height.saturating_sub(2));
    let maximum = total_lines.saturating_sub(visible);
    *from_bottom = (*from_bottom).min(maximum);
    (maximum - *from_bottom).min(usize::from(u16::MAX)) as u16
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
        .track_style(theme::muted())
        .thumb_style(Style::default().fg(theme::ACCENT));
    let maximum_position = content_length.saturating_sub(viewport_length);
    let mut state = ScrollbarState::new(maximum_position + 1)
        .position(position.min(maximum_position))
        .viewport_content_length(viewport_length);
    let track_area = area.inner(Margin {
        horizontal: 0,
        vertical: 1,
    });
    frame.render_stateful_widget(scrollbar, track_area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::{nav_scroll, scroll_offset};
    use crate::tui::input::Motion;

    #[test]
    fn scrolling_down_after_top_moves_immediately() {
        // 20 lines in a pane showing 8 (10 rows minus borders).
        let mut from_bottom = 0;
        nav_scroll(&mut from_bottom, Motion::Top);
        assert_eq!(scroll_offset(20, 10, &mut from_bottom), 0);
        nav_scroll(&mut from_bottom, Motion::Down(1));
        assert_eq!(scroll_offset(20, 10, &mut from_bottom), 1);
    }

    #[test]
    fn scrolling_up_past_the_top_does_not_accumulate() {
        let mut from_bottom = 0;
        nav_scroll(&mut from_bottom, Motion::Up(100));
        assert_eq!(scroll_offset(20, 10, &mut from_bottom), 0);
        nav_scroll(&mut from_bottom, Motion::Down(2));
        assert_eq!(scroll_offset(20, 10, &mut from_bottom), 2);
    }
}
