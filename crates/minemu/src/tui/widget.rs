use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use super::{
    action::Action,
    event::AppEvent,
    types::{InputMode, View, WidgetId},
};

pub struct InputContext {
    pub mode: InputMode,
}

pub struct RenderContext<'a> {
    pub view: View,
    pub mode: InputMode,
    pub focused: WidgetId,
    pub input: &'a str,
    pub ticks: u64,
}

pub trait TuiWidget {
    fn id(&self) -> WidgetId;
    fn visible(&self, view: View) -> bool;
    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>);
    fn handle_key(&mut self, _key: KeyEvent, _context: &InputContext) -> Vec<Action> {
        Vec::new()
    }
    fn update(&mut self, _event: &AppEvent) -> Vec<Action> {
        Vec::new()
    }
}
