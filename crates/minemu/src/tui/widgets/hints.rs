use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::Paragraph,
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

    fn render(&self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(27)])
            .split(area);
        let hints = match context.mode {
            InputMode::Insert => "Normal: <esc>",
            InputMode::Command => "Execute: <enter>  Cancel: <esc>",
            InputMode::Leader => "Runtime: r  Inspect: i  Start/Stop: s",
            InputMode::SearchAscii | InputMode::SearchBytes => "Search: <enter>  Cancel: <esc>",
            InputMode::Goto => "Goto: <enter>  Cancel: <esc>",
            InputMode::Normal => match context.focused {
                WidgetId::Console => "Insert: i  Scroll: [#]jk  Leader: <space>  Help: ?",
                WidgetId::Primary => "Move: [#]hjkl  Subview: <tab>  Search: / \\  Goto: >",
                WidgetId::Secondary => "Move: [#]hjkl  Subview: <tab>  Goto: >",
                _ => "Scroll: [#]jk  Current: G  Leader: <space>  Help: ?",
            },
        };
        let style = Style::default().fg(Color::DarkGray);
        frame.render_widget(Paragraph::new(hints).style(style), columns[0]);
        frame.render_widget(
            Paragraph::new(format!("ticks: 0x{:016x}", context.ticks)).style(style),
            columns[1],
        );
    }
}
