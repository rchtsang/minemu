use std::collections::VecDeque;

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Text},
    widgets::Paragraph,
};

use crate::tui::{
    action::Action,
    event::AppEvent,
    types::{DialogMessage, View, WidgetId},
    widget::{RenderContext, TuiWidget},
};

use super::{nav_scroll, pane_block, scroll_offset};

pub struct DialogWidget {
    messages: VecDeque<DialogMessage>,
    from_bottom: usize,
    capacity: usize,
}

impl Default for DialogWidget {
    fn default() -> Self {
        Self {
            messages: VecDeque::new(),
            from_bottom: 0,
            capacity: 256,
        }
    }
}

impl TuiWidget for DialogWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Dialog
    }

    fn visible(&self, _view: View) -> bool {
        true
    }

    fn render(&self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let lines = self
            .messages
            .iter()
            .map(|message| Line::styled(message.text.clone(), message.level.color()))
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Text::from(lines.clone()))
                .scroll((scroll_offset(lines.len(), area.height, self.from_bottom), 0))
                .block(pane_block("[^d] dialog", context.focused == self.id())),
            area,
        );
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        match event {
            AppEvent::Dialog(message) => {
                if self.messages.len() == self.capacity {
                    self.messages.pop_front();
                }
                self.messages.push_back(message.clone());
                self.from_bottom = 0;
            }
            AppEvent::Navigate(motion) => nav_scroll(&mut self.from_bottom, *motion),
            _ => {}
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::{
        event::AppEvent,
        types::{DialogLevel, DialogMessage},
        widget::TuiWidget,
    };

    use super::DialogWidget;

    #[test]
    fn retains_recoverable_error_messages() {
        let mut dialog = DialogWidget::default();
        dialog.update(&AppEvent::Dialog(DialogMessage::error("read failed")));
        assert_eq!(dialog.messages.len(), 1);
        assert_eq!(dialog.messages[0].level, DialogLevel::Error);
    }
}
