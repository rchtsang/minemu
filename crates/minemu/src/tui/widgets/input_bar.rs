use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
};

use crate::tui::{
    types::{View, WidgetId},
    widget::{RenderContext, TuiWidget},
};

#[derive(Default)]
pub struct InputBarWidget;

impl TuiWidget for InputBarWidget {
    fn id(&self) -> WidgetId {
        WidgetId::InputBar
    }

    fn visible(&self, _view: View) -> bool {
        true
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        frame.render_widget(
            Paragraph::new(format!(" {}", context.input))
                .style(Style::default().fg(Color::White).bg(Color::Rgb(35, 35, 50))),
            area,
        );
    }
}
