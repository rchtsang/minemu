use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::tui::{
    types::{View, WidgetId},
    widget::{RenderContext, TuiWidget},
};

use super::pane_block;

#[derive(Default)]
pub struct HelpWidget;

impl TuiWidget for HelpWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Help
    }

    fn visible(&self, _view: View) -> bool {
        true
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _context: &RenderContext<'_>) {
        let text = "Global\n  ^c console  ^e events  ^d dialog  ^p primary  ^s secondary\n  <space>r runtime  <space>i inspect  <space>s start/stop\n\nNormal mode\n  [#]hjkl move  gg first  G current/end  i insert  : command\n  / ASCII search  \\ hex-byte search  > goto  <tab> subview\n\nCommands\n  :q  :?  :start  :stop  :s  :reset  :view [r|i]\n  :set uart 0|1  :set primary mem|disasm\n  :set secondary reg|peri|pend  :goto LOCATION\n\nPress any key to close.";
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(Color::LightYellow))
                .wrap(Wrap { trim: false })
                .block(pane_block("help", true)),
            area,
        );
    }
}
