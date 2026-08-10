use capstone::prelude::*;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
};

use super::app::{App, Focus, View};

pub fn draw(frame: &mut Frame, app: &App) {
    match app.view {
        View::Runtime => draw_runtime(frame, app),
        View::Introspection => draw_introspection(frame, app),
    }
    if let Some(command) = &app.command {
        let area = centered(frame.area(), 80, 3);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(format!(":{command}")).block(block("command", true)),
            area,
        );
    }
    if app.show_help {
        let area = centered(frame.area(), 80, 8);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(":pause  :resume  :reset  :quit\n:view runtime|inspect  :focus <pane>\n:mem <physical-address>  :uart 0|1\n\nPress any key to close.")
                .block(block("help", true)),
            area,
        );
    }
}

fn draw_runtime(frame: &mut Frame, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(frame.area());
    header(frame, areas[0], app);
    if areas[1].width < 50 || areas[1].height < 10 {
        frame.render_widget(console(app), areas[1]);
    } else {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(areas[1]);
        frame.render_widget(console(app), panes[0]);
        frame.render_widget(events(app), panes[1]);
    }
    frame.render_widget(Paragraph::new(footer(app)), areas[2]);
}

fn draw_introspection(frame: &mut Frame, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(frame.area());
    header(frame, areas[0], app);
    if areas[1].width < 80 || areas[1].height < 18 {
        frame.render_widget(focused_inspection(app), areas[1]);
    } else {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(areas[1]);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(columns[0]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(columns[1]);
        frame.render_widget(memory(app), left[0]);
        frame.render_widget(cpu(app), left[1]);
        frame.render_widget(disassembly(app), right[0]);
        frame.render_widget(hardware(app), right[1]);
    }
    frame.render_widget(Paragraph::new(footer(app)), areas[2]);
}

fn focused_inspection(app: &App) -> Paragraph<'static> {
    match app.focus {
        Focus::Disassembly => disassembly(app),
        Focus::Cpu => cpu(app),
        Focus::Hardware => hardware(app),
        _ => memory(app),
    }
}

fn console(app: &App) -> Paragraph<'static> {
    let output = app
        .peripherals
        .as_ref()
        .map(|peripherals| match app.active_uart {
            minemu_runtime::UartPort::Uart0 => &peripherals.uart0.tx_history,
            minemu_runtime::UartPort::Uart1 => &peripherals.uart1.tx_history,
        })
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default();
    Paragraph::new(output)
        .wrap(Wrap { trim: false })
        .block(block("console", app.focus == Focus::Console))
}

fn events(app: &App) -> Paragraph<'static> {
    let text = app
        .events
        .iter()
        .skip(app.event_offset)
        .map(|event| format!("{event:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    Paragraph::new(text).block(block("events", app.focus == Focus::Events))
}

fn memory(app: &App) -> Paragraph<'static> {
    let mut lines = Vec::new();
    for (line, bytes) in app.memory.chunks(16).enumerate() {
        let address = app.memory_range.start().get() + (line * 16) as u32;
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii = bytes
            .iter()
            .map(|byte| {
                if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '.'
                }
            })
            .collect::<String>();
        lines.push(format!("{address:08x}  {hex:<47}  {ascii}"));
    }
    Paragraph::new(lines.join("\n")).block(block("memory hex/ascii", app.focus == Focus::Memory))
}

fn disassembly(app: &App) -> Paragraph<'static> {
    let Some(execution) = &app.execution else {
        return Paragraph::new("paused execution snapshot unavailable")
            .block(block("disassembly", false));
    };
    let text = if let Ok(capstone) = Capstone::new()
        .arm()
        .mode(capstone::arch::arm::ArchMode::Arm)
        .build()
    {
        capstone
            .disasm_all(
                &execution.instruction_bytes,
                u64::from(execution.instruction_address.get()),
            )
            .map(|instructions| {
                instructions
                    .iter()
                    .map(|instruction| {
                        let bytes = instruction
                            .bytes()
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        format!(
                            "{:08x}: {:<11} {:<8} {}",
                            instruction.address(),
                            bytes,
                            instruction.mnemonic().unwrap_or("?"),
                            instruction.op_str().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let text = if text.is_empty() {
        execution
            .instruction_bytes
            .chunks(4)
            .enumerate()
            .map(|(index, bytes)| {
                let bytes = bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(
                    "{:08x}: {bytes}",
                    execution.instruction_address.get() + (index * 4) as u32,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text
    };
    Paragraph::new(text).block(block("A32 disassembly", app.focus == Focus::Disassembly))
}

fn cpu(app: &App) -> Paragraph<'static> {
    let Some(execution) = &app.execution else {
        return Paragraph::new("paused execution snapshot unavailable").block(block("CPU", false));
    };
    let mut lines = execution
        .registers
        .iter()
        .enumerate()
        .map(|(index, value)| format!("r{index:02} {value:08x}"))
        .collect::<Vec<_>>();
    lines.push(format!(
        "cpsr {:08x}  spsr {:08x}",
        execution.cpsr, execution.spsr
    ));
    Paragraph::new(lines.join("\n")).block(block("CPU registers", app.focus == Focus::Cpu))
}

fn hardware(app: &App) -> Paragraph<'static> {
    let text = app.peripherals.as_ref().map(|peripherals| format!(
        "IRQ pending {:08x} enabled {:08x} claim {:?}\nUART0 status {:08x} control {:08x}\nUART1 status {:08x} control {:08x}\nSysTick period {} control {:08x} status {:08x}\nBlock lba {} sectors {} dma {:08x} status {:08x} error {}\nRNG state {:08x}",
        peripherals.interrupts.pending,
        peripherals.interrupts.enabled,
        peripherals.interrupts.claim,
        peripherals.uart0.status,
        peripherals.uart0.control,
        peripherals.uart1.status,
        peripherals.uart1.control,
        peripherals.systick.period,
        peripherals.systick.control,
        peripherals.systick.status,
        peripherals.block.lba,
        peripherals.block.sector_count,
        peripherals.block.dma_address,
        peripherals.block.status,
        peripherals.block.error,
        peripherals.rng.state,
    )).unwrap_or_else(|| "peripheral snapshot unavailable".into());
    Paragraph::new(text).block(block(
        "interrupts and peripherals",
        app.focus == Focus::Hardware,
    ))
}

fn header(frame: &mut Frame, area: Rect, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(1)])
        .split(area);
    let selected = match app.view {
        View::Runtime => 0,
        View::Introspection => 1,
    };
    frame.render_widget(
        Tabs::new(["runtime", "inspect"])
            .select(selected)
            .style(yellow())
            .highlight_style(yellow().add_modifier(Modifier::BOLD | Modifier::REVERSED))
            .divider(" | "),
        areas[0],
    );
    frame.render_widget(Paragraph::new(status_line(app)), areas[1]);
}

fn status_line(app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:?} ", app.status.lifecycle),
            yellow().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("tick {}  focus {:?}", app.status.machine.ticks, app.focus),
            yellow(),
        ),
    ])
}

fn footer(app: &App) -> Line<'static> {
    let prompt = if app.focus == Focus::Console {
        "Esc: leave console"
    } else {
        "hjkl we gg G | :help"
    };
    Line::styled(prompt, yellow())
}

fn block(title: &str, focused: bool) -> Block<'static> {
    let style = if focused {
        yellow().add_modifier(Modifier::BOLD)
    } else {
        yellow()
    };
    Block::default()
        .borders(Borders::ALL)
        .title(title.to_owned())
        .title_style(style)
        .border_style(style)
}

fn yellow() -> Style {
    Style::default().fg(Color::Yellow)
}

fn centered(area: Rect, width_percent: u16, height: u16) -> Rect {
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(area);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(height),
            Constraint::Percentage(60),
        ])
        .split(horizontal[1]);
    vertical[1]
}
