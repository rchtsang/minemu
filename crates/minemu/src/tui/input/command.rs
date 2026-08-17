use minemu_runtime::UartPort;

use crate::tui::{
    action::Action,
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
        "start" => vec![Action::Start],
        "stop" => vec![Action::Stop],
        "s" => vec![Action::ToggleRun],
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
        value => return Err(format!("unknown command: {value}")),
    };
    if words.next().is_some() {
        return Err("too many command arguments".into());
    }
    Ok(actions)
}

fn parse_set(key: Option<&str>, value: Option<&str>) -> Result<Vec<Action>, String> {
    let action = match (key, value) {
        (Some("uart"), Some("0")) => Action::SetUart(UartPort::Uart0),
        (Some("uart"), Some("1")) => Action::SetUart(UartPort::Uart1),
        (Some("primary"), Some("mem" | "memory")) => Action::SetPrimary(PrimarySubview::Memory),
        (Some("primary"), Some("disasm" | "disassembly")) => {
            Action::SetPrimary(PrimarySubview::Disassembly)
        }
        (Some("secondary"), Some("reg" | "registers")) => {
            Action::SetSecondary(SecondarySubview::Registers)
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
    use super::parse;
    use crate::tui::{action::Action, types::WidgetId};

    #[test]
    fn parses_aliases_and_view_toggle() {
        assert!(matches!(
            parse("s", WidgetId::Dialog).unwrap().as_slice(),
            [Action::ToggleRun]
        ));
        assert!(matches!(
            parse("v", WidgetId::Dialog).unwrap().as_slice(),
            [Action::ToggleView]
        ));
        assert!(parse("set secondary pend", WidgetId::Dialog).is_ok());
    }
}
