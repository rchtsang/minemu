use std::collections::VecDeque;

use minemu_runtime::RuntimeInspection;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Text},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthChar;

use crate::tui::{
    action::Action,
    event::AppEvent,
    types::{DialogMessage, View, WidgetId},
    widget::{RenderContext, TuiWidget},
};

use super::{nav_scroll, pane_block, render_scrollbar, scroll_offset};

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

impl DialogWidget {
    fn push_message(&mut self, message: DialogMessage) {
        if self.messages.len() == self.capacity {
            self.messages.pop_front();
        }
        self.messages.push_back(message);
        self.from_bottom = 0;
    }

    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        self.messages
            .iter()
            .flat_map(|message| {
                message
                    .text
                    .lines()
                    .enumerate()
                    .flat_map(|(index, line)| {
                        let prefix = if index == 0 { "> " } else { "  " };
                        hard_wrap(&format!("{prefix}{line}"), width)
                            .into_iter()
                            .map(|line| Line::styled(line, message.level.color()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

impl TuiWidget for DialogWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Dialog
    }

    fn visible(&self, _view: View) -> bool {
        true
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let inner_width = usize::from(area.width.saturating_sub(3)).max(1);
        let lines = self.lines(inner_width);
        let wrapped_lines = lines.len();
        let viewport = usize::from(area.height.saturating_sub(2));
        let offset = scroll_offset(wrapped_lines, area.height, self.from_bottom);
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .scroll((offset, 0))
                .block(pane_block(
                    "[^d] dialog",
                    context.focused == self.id(),
                    area.width,
                )),
            area,
        );
        render_scrollbar(frame, area, wrapped_lines, viewport, usize::from(offset));
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        match event {
            AppEvent::Dialog(message) => self.push_message(message.clone()),
            AppEvent::Inspection(RuntimeInspection::Translation(virtual_address, physical)) => {
                self.push_message(DialogMessage::info(format!(
                    "0x{:08x} -> 0x{:08x}",
                    virtual_address.get(),
                    physical.get()
                )));
            }
            AppEvent::Navigate(motion) => nav_scroll(&mut self.from_bottom, *motion),
            _ => {}
        }
        Vec::new()
    }
}

fn hard_wrap(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if !line.is_empty() && line_width + character_width > width {
            lines.push(std::mem::take(&mut line));
            line_width = 0;
        }
        line.push(character);
        line_width += character_width;
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use crate::tui::{
        event::AppEvent,
        types::{DialogLevel, DialogMessage},
        widget::TuiWidget,
    };

    use super::{DialogWidget, hard_wrap};

    #[test]
    fn retains_recoverable_error_messages() {
        let mut dialog = DialogWidget::default();
        dialog.update(&AppEvent::Dialog(DialogMessage::error("read failed")));
        assert_eq!(dialog.messages.len(), 1);
        assert_eq!(dialog.messages[0].level, DialogLevel::Error);
        assert_eq!(dialog.lines(80)[0].spans[0].content, "> read failed");
    }

    #[test]
    fn hard_wrapping_preserves_the_complete_message() {
        let message = "> VirtualMemory { address: VirtualAddress(0), length: 288 } failed: Unicorn operation failed: READ_PROT.";
        let lines = hard_wrap(message, 24);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), message);
        assert!(lines.iter().all(|line| line.width() <= 24));
    }
}
