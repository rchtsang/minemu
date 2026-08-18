mod command;
mod motion;
mod search;

pub use motion::{DecodeResult, Motion, MotionDecoder};

pub fn parse_hex_address(value: &str) -> Result<u32, String> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if value.is_empty() {
        return Err("address must be hexadecimal".into());
    }
    u32::from_str_radix(value, 16).map_err(|_| "address must be hexadecimal".into())
}

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use minemu_runtime::RuntimeInspectionRequest;

use super::{
    action::Action,
    types::{DialogMessage, InputMode, View, WidgetId},
};

pub struct InputRouter {
    mode: InputMode,
    buffer: String,
    motion: MotionDecoder,
}

impl Default for InputRouter {
    fn default() -> Self {
        Self {
            mode: InputMode::Normal,
            buffer: String::new(),
            motion: MotionDecoder::default(),
        }
    }
}

impl InputRouter {
    pub const fn mode(&self) -> InputMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: InputMode) {
        self.mode = mode;
        self.buffer.clear();
        self.motion.reset();
    }

    pub fn display(&self) -> String {
        let prefix = match self.mode {
            InputMode::Normal => "",
            InputMode::Insert => "-- INSERT -- ",
            InputMode::Command => ":",
            InputMode::Leader => "<space>",
            InputMode::SearchAscii => "/",
            InputMode::SearchBytes => "\\",
            InputMode::Goto => ">",
        };
        format!("{prefix}{}", self.buffer)
    }

    pub fn route(&mut self, event: Event, focused: WidgetId, _view: View) -> Vec<Action> {
        match event {
            Event::Paste(text) if self.mode == InputMode::Insert => {
                vec![Action::InsertText {
                    target: focused,
                    text,
                }]
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => self.route_key(key, focused),
            _ => Vec::new(),
        }
    }

    fn route_key(&mut self, key: KeyEvent, focused: WidgetId) -> Vec<Action> {
        match self.mode {
            InputMode::Normal => self.normal(key, focused),
            InputMode::Insert => {
                if key.code == KeyCode::Esc {
                    vec![Action::SetMode(InputMode::Normal)]
                } else {
                    vec![Action::WidgetKey {
                        target: focused,
                        key,
                    }]
                }
            }
            InputMode::Leader => self.leader(key),
            InputMode::Command
            | InputMode::SearchAscii
            | InputMode::SearchBytes
            | InputMode::Goto => self.text_input(key, focused),
        }
    }

    fn normal(&mut self, key: KeyEvent, focused: WidgetId) -> Vec<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            let target = match key.code {
                KeyCode::Char('c') => Some(WidgetId::Console),
                KeyCode::Char('e') => Some(WidgetId::Events),
                KeyCode::Char('d') => Some(WidgetId::Dialog),
                KeyCode::Char('p') => Some(WidgetId::Primary),
                KeyCode::Char('s') => Some(WidgetId::Secondary),
                _ => None,
            };
            return target.map_or_else(Vec::new, |target| vec![Action::Focus(target)]);
        }
        match key.code {
            KeyCode::Char(':') => vec![Action::SetMode(InputMode::Command)],
            KeyCode::Char('?') => vec![Action::ShowHelp],
            KeyCode::Char(' ') => vec![Action::SetMode(InputMode::Leader)],
            KeyCode::Esc => {
                self.motion.reset();
                self.buffer.clear();
                Vec::new()
            }
            code => match self.motion.push(code) {
                DecodeResult::Pending => {
                    if let KeyCode::Char(character) = code {
                        self.buffer.push(character);
                    }
                    Vec::new()
                }
                DecodeResult::Motion(motion) => {
                    self.buffer.clear();
                    vec![Action::Navigate {
                        target: focused,
                        motion,
                    }]
                }
                DecodeResult::Unhandled => {
                    self.buffer.clear();
                    vec![Action::WidgetKey {
                        target: focused,
                        key,
                    }]
                }
            },
        }
    }

    fn leader(&mut self, key: KeyEvent) -> Vec<Action> {
        let action = match key.code {
            KeyCode::Char('r') => Some(Action::SelectView(View::Runtime)),
            KeyCode::Char('i') => Some(Action::SelectView(View::Inspect)),
            KeyCode::Char('s') => Some(Action::ToggleRun),
            KeyCode::Esc => None,
            _ => {
                return vec![
                    Action::ShowMessage(DialogMessage::error("unknown leader command")),
                    Action::SetMode(InputMode::Normal),
                ];
            }
        };
        action
            .into_iter()
            .chain([Action::SetMode(InputMode::Normal)])
            .collect()
    }

    fn text_input(&mut self, key: KeyEvent, focused: WidgetId) -> Vec<Action> {
        match key.code {
            KeyCode::Esc => vec![Action::SetMode(InputMode::Normal)],
            KeyCode::Backspace => {
                self.buffer.pop();
                Vec::new()
            }
            KeyCode::Char(character) => {
                self.buffer.push(character);
                Vec::new()
            }
            KeyCode::Enter => {
                let input = std::mem::take(&mut self.buffer);
                let result = match self.mode {
                    InputMode::Command => command::parse(&input, focused),
                    InputMode::SearchAscii => search::ascii(&input).map(|pattern| {
                        vec![Action::RequestInspection {
                            target: focused,
                            request: RuntimeInspectionRequest::SearchMemory { pattern },
                        }]
                    }),
                    InputMode::SearchBytes => search::bytes(&input).map(|pattern| {
                        vec![Action::RequestInspection {
                            target: focused,
                            request: RuntimeInspectionRequest::SearchMemory { pattern },
                        }]
                    }),
                    InputMode::Goto => Ok(vec![Action::Goto {
                        target: focused,
                        value: input,
                    }]),
                    _ => unreachable!(),
                };
                let mut actions = result
                    .unwrap_or_else(|error| vec![Action::ShowMessage(DialogMessage::error(error))]);
                actions.push(Action::SetMode(InputMode::Normal));
                actions
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use super::InputRouter;
    use crate::tui::{
        action::Action,
        types::{InputMode, View, WidgetId},
    };

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn routes_leader_and_control_focus_bindings() {
        let mut router = InputRouter::default();
        assert!(matches!(
            router
                .route(key(KeyCode::Char(' ')), WidgetId::Console, View::Runtime)
                .as_slice(),
            [Action::SetMode(InputMode::Leader)]
        ));
        router.set_mode(InputMode::Leader);
        assert!(matches!(
            router
                .route(key(KeyCode::Char('i')), WidgetId::Console, View::Runtime)
                .as_slice(),
            [
                Action::SelectView(View::Inspect),
                Action::SetMode(InputMode::Normal)
            ]
        ));

        router.set_mode(InputMode::Normal);
        let focus = Event::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(matches!(
            router
                .route(focus, WidgetId::Console, View::Runtime)
                .as_slice(),
            [Action::Focus(WidgetId::Dialog)]
        ));
    }

    #[test]
    fn insert_mode_is_target_agnostic() {
        let mut router = InputRouter::default();
        router.set_mode(InputMode::Insert);
        assert!(matches!(
            router
                .route(key(KeyCode::Char('x')), WidgetId::Primary, View::Inspect)
                .as_slice(),
            [Action::WidgetKey {
                target: WidgetId::Primary,
                ..
            }]
        ));
    }
}
