use std::num::NonZeroU64;

use minemu_platform::VirtualAddress;
use minemu_runtime::{RuntimeInspectionRequest, UartPort};

use crate::tui::{
    action::Action,
    input::parse_hex_address,
    types::{PrimarySubview, SecondarySubview, View, WidgetId},
};

pub fn parse(command: &str, target: WidgetId) -> Result<Vec<Action>, String> {
    let mut words = command.split_whitespace();
    let Some(name) = words.next() else {
        return Ok(Vec::new());
    };
    let actions = match name {
        "q" | "quit" => vec![Action::Quit],
        "?" | "help" => vec![Action::ShowHelp],
        "start" => vec![Action::Start(parse_instruction_limit(words.next())?)],
        "stop" => vec![Action::Stop],
        "s" => match words.next() {
            None => vec![Action::ToggleRun],
            Some(value) => vec![Action::Start(parse_instruction_limit(Some(value))?)],
        },
        "reset" => vec![Action::Reset],
        "v" | "view" => match words.next() {
            None => vec![Action::ToggleView],
            Some("r" | "runtime") => vec![Action::SelectView(View::Runtime)],
            Some("i" | "inspect") => vec![Action::SelectView(View::Inspect)],
            Some(value) => return Err(format!("unknown view: {value}")),
        },
        "set" => parse_set(words.next(), words.next())?,
        "g" | "goto" => {
            let value = words
                .next()
                .ok_or_else(|| "usage: :goto <location>".to_string())?;
            vec![Action::Goto {
                target,
                value: value.into(),
            }]
        }
        "translate" | "xlate" => {
            let value = words
                .next()
                .ok_or_else(|| "usage: :translate <virtual-address>".to_string())?;
            vec![Action::RequestInspection {
                target: WidgetId::Dialog,
                request: RuntimeInspectionRequest::Translate(VirtualAddress::new(
                    parse_hex_address(value)?,
                )),
            }]
        }
        value => return Err(format!("unknown command: {value}")),
    };
    if words.next().is_some() {
        return Err("too many command arguments".into());
    }
    Ok(actions)
}

fn parse_instruction_limit(value: Option<&str>) -> Result<Option<NonZeroU64>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    value
        .parse::<u64>()
        .ok()
        .and_then(NonZeroU64::new)
        .map(Some)
        .ok_or_else(|| "instruction count must be a positive decimal integer".into())
}

fn parse_set(key: Option<&str>, value: Option<&str>) -> Result<Vec<Action>, String> {
    let action = match (key, value) {
        (Some("uart"), Some("0")) => Action::SetUart(UartPort::Uart0),
        (Some("uart"), Some("1")) => Action::SetUart(UartPort::Uart1),
        (Some("primary"), Some("pmem" | "physical" | "mem" | "memory")) => {
            Action::SetPrimary(PrimarySubview::PhysicalMemory)
        }
        (Some("primary"), Some("vmem" | "virtual")) => {
            Action::SetPrimary(PrimarySubview::VirtualMemory)
        }
        (Some("primary"), Some("disasm" | "disassembly")) => {
            Action::SetPrimary(PrimarySubview::Disassembly)
        }
        (Some("secondary"), Some("reg" | "registers")) => {
            Action::SetSecondary(SecondarySubview::Registers)
        }
        (Some("secondary"), Some("sys" | "system")) => {
            Action::SetSecondary(SecondarySubview::System)
        }
        (Some("secondary"), Some("peri" | "peripherals")) => {
            Action::SetSecondary(SecondarySubview::Peripherals)
        }
        (Some("secondary"), Some("pend" | "pending")) => {
            Action::SetSecondary(SecondarySubview::Pending)
        }
        _ => return Err("usage: :set uart|primary|secondary <value>".into()),
    };
    Ok(vec![action])
}

#[cfg(test)]
mod tests {
    use minemu_runtime::RuntimeInspectionRequest;

    use super::parse;
    use crate::tui::{action::Action, types::WidgetId};

    #[test]
    fn parses_aliases_and_view_toggle() {
        assert!(matches!(
            parse("s", WidgetId::Dialog).unwrap().as_slice(),
            [Action::ToggleRun]
        ));
        assert!(matches!(
            parse("start", WidgetId::Dialog).unwrap().as_slice(),
            [Action::Start(None)]
        ));
        assert!(matches!(
            parse("start 25", WidgetId::Dialog).unwrap().as_slice(),
            [Action::Start(Some(count))] if count.get() == 25
        ));
        assert!(matches!(
            parse("s 7", WidgetId::Dialog).unwrap().as_slice(),
            [Action::Start(Some(count))] if count.get() == 7
        ));
        assert!(parse("start 0", WidgetId::Dialog).is_err());
        assert!(matches!(
            parse("v", WidgetId::Dialog).unwrap().as_slice(),
            [Action::ToggleView]
        ));
        assert!(parse("set secondary pend", WidgetId::Dialog).is_ok());
        assert!(parse("set primary vmem", WidgetId::Dialog).is_ok());
        assert!(matches!(
            parse("translate 0xc0030264", WidgetId::Primary)
                .unwrap()
                .as_slice(),
            [Action::RequestInspection {
                target: WidgetId::Dialog,
                request: RuntimeInspectionRequest::Translate(address),
            }] if address.get() == 0xc003_0264
        ));
    }
}
