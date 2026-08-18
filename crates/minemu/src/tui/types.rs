use ratatui::style::Color;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum View {
    Runtime,
    Inspect,
}

impl View {
    pub const fn toggled(self) -> Self {
        match self {
            Self::Runtime => Self::Inspect,
            Self::Inspect => Self::Runtime,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WidgetId {
    Header,
    Console,
    Events,
    Dialog,
    Primary,
    Secondary,
    InputBar,
    Hints,
    Help,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMode {
    Normal,
    Insert,
    Command,
    Leader,
    SearchAscii,
    SearchBytes,
    Goto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimarySubview {
    Memory,
    Disassembly,
}

impl PrimarySubview {
    pub const fn toggled(self) -> Self {
        match self {
            Self::Memory => Self::Disassembly,
            Self::Disassembly => Self::Memory,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondarySubview {
    Registers,
    Peripherals,
    Pending,
}

impl SecondarySubview {
    pub const fn next(self) -> Self {
        match self {
            Self::Registers => Self::Peripherals,
            Self::Peripherals => Self::Pending,
            Self::Pending => Self::Registers,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogLevel {
    Info,
    Error,
}

impl DialogLevel {
    pub const fn color(self) -> Color {
        match self {
            Self::Info => Color::Yellow,
            Self::Error => Color::Red,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogMessage {
    pub level: DialogLevel,
    pub text: String,
}

impl DialogMessage {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            level: DialogLevel::Info,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            level: DialogLevel::Error,
            text: text.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitId {
    RuntimeMain,
    InspectMain,
}
