use std::collections::HashMap;

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use super::types::{SplitId, View, WidgetId};

pub struct SplitLayout {
    runtime_main: u16,
    inspect_main: u16,
}

impl Default for SplitLayout {
    fn default() -> Self {
        Self {
            runtime_main: 65,
            inspect_main: 65,
        }
    }
}

impl SplitLayout {
    pub fn resize(&mut self, split: SplitId, percent: u16) {
        let percent = percent.clamp(25, 80);
        match split {
            SplitId::RuntimeMain => self.runtime_main = percent,
            SplitId::InspectMain => self.inspect_main = percent,
        }
    }

    pub fn percent(&self, view: View) -> u16 {
        match view {
            View::Runtime => self.runtime_main,
            View::Inspect => self.inspect_main,
        }
    }

    pub fn split_column(&self, area: Rect, view: View) -> u16 {
        area.x + area.width.saturating_mul(self.percent(view)) / 100
    }

    pub fn areas(&self, area: Rect, view: View, focused: WidgetId) -> HashMap<WidgetId, Rect> {
        let mut result = HashMap::new();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(4),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);
        result.insert(WidgetId::Header, rows[0]);
        result.insert(WidgetId::InputBar, rows[2]);
        result.insert(WidgetId::Hints, rows[3]);
        result.insert(WidgetId::Help, centered(area, 90, 34));

        if rows[1].width < 70 || rows[1].height < 12 {
            let visible = match (view, focused) {
                (View::Runtime, WidgetId::Events) => WidgetId::Events,
                (View::Runtime, WidgetId::Dialog) => WidgetId::Dialog,
                (View::Runtime, _) => WidgetId::Console,
                (View::Inspect, WidgetId::Secondary) => WidgetId::Secondary,
                (View::Inspect, WidgetId::Dialog) => WidgetId::Dialog,
                (View::Inspect, _) => WidgetId::Primary,
            };
            result.insert(visible, rows[1]);
            return result;
        }

        let percent = self.percent(view);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(percent),
                Constraint::Percentage(100 - percent),
            ])
            .split(rows[1]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(columns[1]);
        match view {
            View::Runtime => {
                result.insert(WidgetId::Console, columns[0]);
                result.insert(WidgetId::Events, right[0]);
            }
            View::Inspect => {
                result.insert(WidgetId::Primary, columns[0]);
                result.insert(WidgetId::Secondary, right[0]);
            }
        }
        result.insert(WidgetId::Dialog, right[1]);
        result
    }
}

fn centered(area: Rect, width_percent: u16, maximum_height: u16) -> Rect {
    let height = maximum_height.min(area.height.saturating_sub(2));
    let vertical = Rect {
        y: area.y + area.height.saturating_sub(height) / 2,
        height,
        ..area
    };
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical)[1]
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::SplitLayout;
    use crate::tui::types::{SplitId, View, WidgetId};

    #[test]
    fn resize_is_clamped_and_changes_the_main_split() {
        let mut layout = SplitLayout::default();
        layout.resize(SplitId::RuntimeMain, 90);
        assert_eq!(layout.percent(View::Runtime), 80);
        let areas = layout.areas(Rect::new(0, 0, 100, 30), View::Runtime, WidgetId::Console);
        assert!(areas[&WidgetId::Console].width > areas[&WidgetId::Events].width);
    }
}
