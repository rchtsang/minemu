use crossterm::event::{KeyCode, KeyEvent};
use minemu_platform::PeripheralsInspection;
use minemu_runtime::{RuntimeInspection, UartPort};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Paragraph, Wrap},
};

use crate::tui::{
    action::Action,
    event::AppEvent,
    input::Motion,
    types::{InputMode, View, WidgetId},
    widget::{InputContext, RenderContext, TuiWidget},
};

use super::{pane_block, render_scrollbar, scroll_offset};

pub struct ConsoleWidget {
    uart: UartPort,
    peripherals: Option<PeripheralsInspection>,
    from_bottom: usize,
    active: bool,
}

impl Default for ConsoleWidget {
    fn default() -> Self {
        Self {
            uart: UartPort::Uart0,
            peripherals: None,
            from_bottom: 0,
            active: true,
        }
    }
}

impl ConsoleWidget {
    fn output(&self) -> String {
        self.peripherals
            .as_ref()
            .map(|peripherals| match self.uart {
                UartPort::Uart0 => &peripherals.uart0.tx_history,
                UartPort::Uart1 => &peripherals.uart1.tx_history,
            })
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default()
    }
}

impl TuiWidget for ConsoleWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Console
    }

    fn visible(&self, view: View) -> bool {
        view == View::Runtime
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, context: &RenderContext<'_>) {
        let output = self.output();
        let lines = output.lines().count();
        let viewport = usize::from(area.height.saturating_sub(2));
        let offset = scroll_offset(lines, area.height, self.from_bottom);
        frame.render_widget(
            Paragraph::new(output)
                .wrap(Wrap { trim: false })
                .scroll((offset, 0))
                .block(pane_block(
                    format!(
                        "[^c] console (uart {})",
                        if self.uart == UartPort::Uart0 { 0 } else { 1 }
                    ),
                    context.focused == self.id(),
                    area.width,
                )),
            area,
        );
        render_scrollbar(frame, area, lines, viewport, usize::from(offset));
    }

    fn handle_key(&mut self, key: KeyEvent, context: &InputContext) -> Vec<Action> {
        match context.mode {
            InputMode::Normal if key.code == KeyCode::Char('i') => {
                vec![Action::SetMode(InputMode::Insert)]
            }
            InputMode::Insert => match key.code {
                KeyCode::Enter => vec![Action::SendUart(self.uart, b"\n".to_vec())],
                KeyCode::Backspace => vec![Action::SendUart(self.uart, vec![8])],
                KeyCode::Char(character) => {
                    let mut bytes = [0; 4];
                    vec![Action::SendUart(
                        self.uart,
                        character.encode_utf8(&mut bytes).as_bytes().to_vec(),
                    )]
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        match event {
            AppEvent::Inspection(RuntimeInspection::Peripherals(peripherals)) => {
                self.peripherals = Some(peripherals.clone());
            }
            AppEvent::ViewChanged(view) => self.active = *view == View::Runtime,
            AppEvent::Pulse if self.active => {
                return vec![Action::RequestInspection {
                    target: self.id(),
                    request: minemu_runtime::RuntimeInspectionRequest::Machine(
                        minemu_platform::InspectionRequest::Peripherals,
                    ),
                }];
            }
            AppEvent::UartSelected(uart) => self.uart = *uart,
            AppEvent::Navigate(motion) => match motion {
                Motion::Up(count) | Motion::Left(count) => {
                    self.from_bottom = self.from_bottom.saturating_add(*count)
                }
                Motion::Down(count) | Motion::Right(count) => {
                    self.from_bottom = self.from_bottom.saturating_sub(*count)
                }
                Motion::Top => self.from_bottom = usize::MAX,
                Motion::Bottom => self.from_bottom = 0,
                _ => {}
            },
            AppEvent::InsertText(text) => {
                return vec![Action::SendUart(self.uart, text.as_bytes().to_vec())];
            }
            _ => {}
        }
        Vec::new()
    }
}
