use minemu_platform::ObservableEvent;
use minemu_runtime::RuntimeInspection;
use ratatui::{Frame, layout::Rect, widgets::Paragraph};

use crate::tui::{
    action::Action,
    event::AppEvent,
    types::{View, WidgetId},
    widget::{RenderContext, TuiWidget},
};

use super::{nav_scroll, pane_block, render_scrollbar, scroll_offset};

#[derive(Default)]
pub struct EventsWidget {
    events: Vec<ObservableEvent>,
    from_bottom: usize,
    active: bool,
}

impl TuiWidget for EventsWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Events
    }

    fn visible(&self, view: View) -> bool {
        view == View::Runtime
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let lines = self
            .events
            .iter()
            .map(|event| format!("{event:?}"))
            .collect::<Vec<_>>();
        let viewport = usize::from(area.height.saturating_sub(2));
        let offset = scroll_offset(lines.len(), area.height, self.from_bottom);
        frame.render_widget(
            Paragraph::new(lines.join("\n"))
                .scroll((offset, 0))
                .block(pane_block("[^e] events", context.focused == self.id())),
            area,
        );
        render_scrollbar(frame, area, lines.len(), viewport, usize::from(offset));
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        match event {
            AppEvent::Inspection(RuntimeInspection::Events(events)) => self.events = events.clone(),
            AppEvent::ViewChanged(view) => self.active = *view == View::Runtime,
            AppEvent::Pulse if self.active => {
                return vec![Action::RequestInspection {
                    target: self.id(),
                    request: minemu_runtime::RuntimeInspectionRequest::Machine(
                        minemu_platform::InspectionRequest::Events,
                    ),
                }];
            }
            AppEvent::Navigate(motion) => nav_scroll(&mut self.from_bottom, *motion),
            _ => {}
        }
        Vec::new()
    }
}
