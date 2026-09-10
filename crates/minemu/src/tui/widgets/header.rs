use minemu_runtime::RuntimeStatus;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Tabs},
};

use crate::tui::{
    action::Action,
    event::AppEvent,
    types::{View, WidgetId},
    widget::{RenderContext, TuiWidget},
};

#[derive(Default)]
pub struct HeaderWidget {
    status: Option<RuntimeStatus>,
}

impl TuiWidget for HeaderWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Header
    }

    fn visible(&self, _view: View) -> bool {
        true
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::styled(
                " minemu ",
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(27), Constraint::Min(1)])
            .split(inner);
        let selected = usize::from(context.view == View::Inspect);
        let titles = match context.view {
            View::Runtime => ["[ runtime ]", "  inspect  "],
            View::Inspect => ["  runtime  ", "[ inspect ]"],
        };
        frame.render_widget(
            Tabs::new(titles)
                .select(selected)
                .style(Style::default().fg(Color::DarkGray))
                .highlight_style(
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD),
                )
                .padding("", "")
                .divider(" "),
            columns[0],
        );
        let status = self.status.as_ref().map_or_else(
            || "starting".into(),
            |status| format!("{:?}", status.lifecycle).to_lowercase(),
        );
        let status_area = Rect {
            width: columns[1].width.saturating_sub(1),
            ..columns[1]
        };
        frame.render_widget(
            Paragraph::new(Line::styled(
                status,
                Style::default().fg(Color::LightYellow),
            )),
            status_area,
        );
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        if let AppEvent::Status(status) = event {
            self.status = Some(status.clone());
        }
        Vec::new()
    }
}

impl HeaderWidget {
    pub fn view_at(area: Rect, column: u16, row: u16) -> Option<View> {
        let inner = Block::default().borders(Borders::TOP).inner(area);
        if row < inner.y || row >= inner.y.saturating_add(inner.height) {
            return None;
        }
        let offset = column.checked_sub(inner.x)?;
        match offset {
            0..=10 => Some(View::Runtime),
            12..=22 => Some(View::Inspect),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::HeaderWidget;
    use crate::tui::types::View;

    #[test]
    fn view_tabs_have_clickable_hit_areas() {
        let area = Rect::new(4, 2, 80, 2);
        assert_eq!(HeaderWidget::view_at(area, 4, 3), Some(View::Runtime));
        assert_eq!(HeaderWidget::view_at(area, 16, 3), Some(View::Inspect));
        assert_eq!(HeaderWidget::view_at(area, 15, 3), None);
        assert_eq!(HeaderWidget::view_at(area, 4, 2), None);
        assert_eq!(HeaderWidget::view_at(area, 30, 3), None);
    }
}
