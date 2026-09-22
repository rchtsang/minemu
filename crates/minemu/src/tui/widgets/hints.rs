use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::{
    types::{InputMode, View, WidgetId},
    widget::{RenderContext, TuiWidget},
};

#[derive(Default)]
pub struct HintsWidget;

impl TuiWidget for HintsWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Hints
    }

    fn visible(&self, _view: View) -> bool {
        true
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(28)])
            .split(area);
        let hints = match context.mode {
            InputMode::Insert => "Normal: <esc>",
            InputMode::Command => "Execute: <enter>  Cancel: <esc>",
            InputMode::Leader => "Runtime: r  Inspect: i  Start/Stop: s",
            InputMode::SearchAscii | InputMode::SearchBytes => "Search: <enter>  Cancel: <esc>",
            InputMode::Goto => "Goto: <enter>  Cancel: <esc>",
            InputMode::Normal => match context.focused {
                WidgetId::Console => "Insert: i  Scroll: [#]jk/arrows  Leader: <space>  Help: ?",
                WidgetId::Primary => "Move: [#]hjkl/arrows  Subview: <tab>  Search: / \\  Goto: >",
                WidgetId::Secondary => "Move: [#]hjkl/arrows  Subview: <tab>  Goto: >",
                _ => "Scroll: [#]jk/arrows  Current: G  Leader: <space>  Help: ?",
            },
        };
        let style = Style::default().fg(Color::DarkGray);
        let hint_area = Rect {
            width: columns[0].width.saturating_sub(1),
            ..columns[0]
        };
        frame.render_widget(Paragraph::new(hints).style(style), hint_area);
        frame.render_widget(
            Paragraph::new(format!(" ticks: 0x{:016x}", context.ticks))
                .style(style)
                .block(Block::default().borders(Borders::LEFT).border_style(style)),
            columns[1],
        );
    }
}
