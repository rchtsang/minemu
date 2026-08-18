use std::num::NonZeroU64;

use crossterm::event::KeyEvent;
use minemu_runtime::{RuntimeInspectionRequest, UartPort};

use super::{
    input::Motion,
    types::{DialogMessage, InputMode, PrimarySubview, SecondarySubview, SplitId, View, WidgetId},
};

#[derive(Clone, Debug)]
pub enum Action {
    Quit,
    SelectView(View),
    ToggleView,
    Focus(WidgetId),
    SetMode(InputMode),
    ToggleRun,
    Start(Option<NonZeroU64>),
    Stop,
    Reset,
    SendUart(UartPort, Vec<u8>),
    RequestInspection {
        target: WidgetId,
        request: RuntimeInspectionRequest,
    },
    WidgetKey {
        target: WidgetId,
        key: KeyEvent,
    },
    InsertText {
        target: WidgetId,
        text: String,
    },
    Navigate {
        target: WidgetId,
        motion: Motion,
    },
    Goto {
        target: WidgetId,
        value: String,
    },
    SetUart(UartPort),
    SetPrimary(PrimarySubview),
    SetSecondary(SecondarySubview),
    Refresh(WidgetId),
    ShowMessage(DialogMessage),
    ShowHelp,
    Resize {
        split: SplitId,
        percent: u16,
    },
}
