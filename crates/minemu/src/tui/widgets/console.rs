use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent};
use minemu_platform::UartInspection;
use minemu_runtime::{RuntimeInspection, UartPort};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Text},
    widgets::Paragraph,
};

use crate::tui::{
    action::Action,
    event::AppEvent,
    types::{InputMode, View, WidgetId},
    widget::{InputContext, RenderContext, TuiWidget},
};

use super::{hard_wrap, nav_scroll, pane_block, render_scrollbar, scroll_offset};

const TRANSCRIPT_CAPACITY: usize = 16 * 1024;

#[derive(Default)]
struct ConsoleStream {
    transcript: VecDeque<u8>,
    input_line: Vec<u8>,
    observed_tx: Vec<u8>,
    initialized: bool,
}

impl ConsoleStream {
    fn observe(&mut self, uart: &UartInspection) {
        if !self.initialized {
            let input = self.transcript.drain(..).collect::<Vec<_>>();
            self.append(&uart.tx_history);
            self.append(&input);
            self.initialized = true;
        } else {
            let overlap = suffix_prefix_overlap(&self.observed_tx, &uart.tx_history);
            self.append(&uart.tx_history[overlap..]);
        }
        self.observed_tx.clone_from(&uart.tx_history);
    }

    fn echo_input(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match byte {
                b'\n' => {
                    self.append(&[byte]);
                    self.input_line.clear();
                }
                8 | 127 => self.backspace(),
                _ => {
                    self.append(&[byte]);
                    self.input_line.push(byte);
                }
            }
        }
    }

    fn backspace(&mut self) {
        let Some(mut start) = self.input_line.len().checked_sub(1) else {
            return;
        };
        while start > 0 && self.input_line[start] & 0xc0 == 0x80 {
            start -= 1;
        }
        let removed = self.input_line[start..].to_vec();
        self.input_line.truncate(start);
        if self.transcript.len() >= removed.len()
            && self
                .transcript
                .iter()
                .skip(self.transcript.len() - removed.len())
                .copied()
                .eq(removed.iter().copied())
        {
            self.transcript
                .truncate(self.transcript.len() - removed.len());
        }
    }

    fn append(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.transcript.len() == TRANSCRIPT_CAPACITY {
                self.transcript.pop_front();
            }
            self.transcript.push_back(byte);
        }
    }

    fn output(&self) -> String {
        let bytes = self.transcript.iter().copied().collect::<Vec<_>>();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

fn suffix_prefix_overlap(previous: &[u8], current: &[u8]) -> usize {
    if previous.is_empty() || current.is_empty() {
        return 0;
    }

    let mut prefix = vec![0; current.len()];
    for index in 1..current.len() {
        let mut matched = prefix[index - 1];
        while matched > 0 && current[index] != current[matched] {
            matched = prefix[matched - 1];
        }
        if current[index] == current[matched] {
            matched += 1;
        }
        prefix[index] = matched;
    }

    let mut matched = 0;
    for (index, &byte) in previous.iter().enumerate() {
        while matched > 0 && byte != current[matched] {
            matched = prefix[matched - 1];
        }
        if byte == current[matched] {
            matched += 1;
        }
        if matched == current.len() && index + 1 != previous.len() {
            matched = prefix[matched - 1];
        }
    }
    matched
}

pub struct ConsoleWidget {
    uart: UartPort,
    uart0: ConsoleStream,
    uart1: ConsoleStream,
    from_bottom: usize,
    active: bool,
}

impl Default for ConsoleWidget {
    fn default() -> Self {
        Self {
            uart: UartPort::Uart0,
            uart0: ConsoleStream::default(),
            uart1: ConsoleStream::default(),
            from_bottom: 0,
            active: true,
        }
    }
}

impl ConsoleWidget {
    const fn stream(&self) -> &ConsoleStream {
        match self.uart {
            UartPort::Uart0 => &self.uart0,
            UartPort::Uart1 => &self.uart1,
        }
    }

    const fn stream_mut(&mut self) -> &mut ConsoleStream {
        match self.uart {
            UartPort::Uart0 => &mut self.uart0,
            UartPort::Uart1 => &mut self.uart1,
        }
    }

    fn send(&mut self, bytes: Vec<u8>) -> Vec<Action> {
        self.stream_mut().echo_input(&bytes);
        vec![Action::SendUart(self.uart, bytes)]
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
        // Wrap here rather than in the paragraph so scrolling counts screen rows.
        let inner_width = usize::from(area.width.saturating_sub(3));
        let output = self.stream().output();
        let lines = output
            .lines()
            .flat_map(|line| hard_wrap(line, inner_width))
            .map(Line::raw)
            .collect::<Vec<_>>();
        let total = lines.len();
        let viewport = usize::from(area.height.saturating_sub(2));
        let offset = scroll_offset(total, area.height, &mut self.from_bottom);
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .scroll((offset, 0))
                .block(pane_block(
                    format!("[^c] console (uart {})", self.uart.index()),
                    context.focused == self.id(),
                    area.width,
                )),
            area,
        );
        render_scrollbar(frame, area, total, viewport, usize::from(offset));
    }

    fn handle_key(&mut self, key: KeyEvent, context: &InputContext) -> Vec<Action> {
        match context.mode {
            InputMode::Normal if key.code == KeyCode::Char('i') => {
                vec![Action::SetMode(InputMode::Insert)]
            }
            InputMode::Insert => match key.code {
                KeyCode::Enter => self.send(b"\n".to_vec()),
                KeyCode::Backspace => self.send(vec![8]),
                KeyCode::Char(character) => {
                    let mut bytes = [0; 4];
                    self.send(character.encode_utf8(&mut bytes).as_bytes().to_vec())
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn update(&mut self, event: &AppEvent) -> Vec<Action> {
        match event {
            AppEvent::Inspection(RuntimeInspection::Peripherals(peripherals)) => {
                self.uart0.observe(&peripherals.uart0);
                self.uart1.observe(&peripherals.uart1);
            }
            AppEvent::ViewChanged(view) => {
                self.active = *view == View::Runtime;
                if self.active {
                    return vec![Action::RequestInspection {
                        target: self.id(),
                        request: minemu_runtime::RuntimeInspectionRequest::Machine(
                            minemu_platform::InspectionRequest::Peripherals,
                        ),
                    }];
                }
            }
            AppEvent::Reset => {
                self.uart0.reset();
                self.uart1.reset();
                self.from_bottom = 0;
            }
            AppEvent::Pulse if self.active => {
                return vec![Action::RequestInspection {
                    target: self.id(),
                    request: minemu_runtime::RuntimeInspectionRequest::Machine(
                        minemu_platform::InspectionRequest::Peripherals,
                    ),
                }];
            }
            AppEvent::UartSelected(uart) => self.uart = *uart,
            AppEvent::Navigate(motion) => nav_scroll(&mut self.from_bottom, *motion),
            AppEvent::InsertText(text) => {
                return self.send(text.as_bytes().to_vec());
            }
            _ => {}
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use minemu_platform::UartInspection;

    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    use super::{ConsoleStream, ConsoleWidget};
    use crate::tui::{
        event::AppEvent,
        input::Motion,
        types::{InputMode, View, WidgetId},
        widget::{RenderContext, TuiWidget},
    };

    /// Renders `widget` into a 20x6 pane and returns its rows as text.
    fn render(widget: &mut ConsoleWidget) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        let context = RenderContext {
            view: View::Runtime,
            mode: InputMode::Normal,
            focused: WidgetId::Console,
            input: "",
            ticks: 0,
        };
        terminal
            .draw(|frame| widget.render(frame, Rect::new(0, 0, 20, 6), &context))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..6)
            .map(|y| (0..20).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    fn inspection(history: &[u8]) -> UartInspection {
        UartInspection {
            status: 0,
            control: 0,
            rx_queued: 0,
            rx_irq_enabled: false,
            tx_history: history.to_vec(),
        }
    }

    #[test]
    fn merges_local_input_with_incremental_guest_output() {
        let mut stream = ConsoleStream::default();
        stream.observe(&inspection(b"msh> "));
        stream.echo_input(b"echo hi\n");
        stream.observe(&inspection(b"msh> hi\nmsh> "));
        assert_eq!(stream.output(), "msh> echo hi\nhi\nmsh> ");

        stream.observe(&inspection(b"msh> hi\nmsh> "));
        assert_eq!(stream.output(), "msh> echo hi\nhi\nmsh> ");
    }

    #[test]
    fn backspace_removes_the_last_locally_echoed_character() {
        let mut stream = ConsoleStream::default();
        stream.echo_input("aé".as_bytes());
        stream.echo_input(&[8]);
        assert_eq!(stream.output(), "a");
    }

    #[test]
    fn finds_new_output_after_bounded_history_rollover() {
        let mut stream = ConsoleStream::default();
        stream.observe(&inspection(b"abcdefgh"));
        stream.observe(&inspection(b"efghijkl"));
        assert_eq!(stream.output(), "abcdefghijkl");
    }

    #[test]
    fn wrapped_output_keeps_the_newest_line_visible() {
        let mut widget = ConsoleWidget::default();
        // Each 40-character line wraps onto several rows of the 17-column pane.
        let history = (0..3)
            .map(|line| format!("{line}{}", "x".repeat(39)))
            .chain(["last".to_string()])
            .collect::<Vec<_>>()
            .join("\n");
        widget.uart0.observe(&inspection(history.as_bytes()));
        let rows = render(&mut widget);
        assert!(rows[4].contains("last"), "{rows:#?}");

        // Scrolling moves by wrapped screen rows, and w scrolls down like j.
        widget.update(&AppEvent::Navigate(Motion::Up(1)));
        assert!(!render(&mut widget)[4].contains("last"));
        widget.update(&AppEvent::Navigate(Motion::NextItem(1)));
        assert!(render(&mut widget)[4].contains("last"));
    }
}
