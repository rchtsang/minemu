use minemu_runtime::{RuntimeInspection, RuntimeStatus, UartPort};

use super::{
    input::Motion,
    types::{DialogMessage, PrimarySubview, SecondarySubview, View},
};

#[derive(Clone, Debug)]
pub enum AppEvent {
    Status(RuntimeStatus),
    Inspection(RuntimeInspection),
    ViewChanged(View),
    Pulse,
    Refresh,
    Navigate(Motion),
    Goto(String),
    InsertText(String),
    UartSelected(UartPort),
    PrimarySelected(PrimarySubview),
    SecondarySelected(SecondarySubview),
    Dialog(DialogMessage),
}
