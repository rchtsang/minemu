use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Row, Table, block::Padding},
};

use crate::tui::{
    types::{View, WidgetId},
    widget::{RenderContext, TuiWidget},
};

#[derive(Default)]
pub struct HelpWidget;

enum Entry {
    Section(&'static str),
    Command(&'static str, &'static str),
}

use Entry::{Command, Section};

/// Help rows; `tests::every_command_is_documented` keeps this in step with the
/// command parser.
const HELP: &[Entry] = &[
    Section("VIEWS / EMULATION"),
    Command("Space r", "Select runtime view"),
    Command("Space i", "Select inspect view and pause"),
    Command("Space s", "Toggle emulation start/stop"),
    Command(
        ":start [COUNT]",
        "Run continuously or for COUNT instructions",
    ),
    Command(":s [COUNT]", "Toggle, or run COUNT instructions"),
    Command(":stop", "Stop emulation"),
    Command(":reset", "Request an emulated power cycle"),
    Section("FOCUS AND MODES"),
    Command("Ctrl+c/e/d", "Focus console, events, or dialog"),
    Command("Ctrl+p/s", "Focus primary or secondary inspector"),
    Command("i / Esc", "Console insert mode / return to normal mode"),
    Command("? / :help", "Open this help window (also :?)"),
    Section("NAVIGATION"),
    Command("[#] arrows / hjkl", "Move by pane-local units"),
    Command(
        "[#] w / e",
        "Memory: next / end of 4-byte word; elsewhere like j",
    ),
    Command("gg / G", "Move to first / last item"),
    Command("Tab", "Switch the focused inspector subview"),
    Command("/text", "Physical memory: search RAM for ASCII text"),
    Command("\\de ad be ef", "Physical memory: search RAM for hex bytes"),
    Command(
        ">location",
        "Primary or registers: goto an address or register",
    ),
    Section("COMMANDS"),
    Command(":view [r|i]", "Toggle or select a view (alias :v)"),
    Command(":set uart 0|1", "Select console input UART"),
    Command(":set primary", "Select pmem, vmem, or disasm"),
    Command(":set secondary", "Select reg, sys, peri, or pend"),
    Command(
        ":goto LOCATION",
        "Goto in the focused pane, like > (alias :g)",
    ),
    Command(
        ":translate ADDR",
        "Translate a virtual address (alias :xlate)",
    ),
    Command(":q / :quit", "Quit minemu"),
];

impl TuiWidget for HelpWidget {
    fn id(&self) -> WidgetId {
        WidgetId::Help
    }

    fn visible(&self, _view: View) -> bool {
        true
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _context: &RenderContext<'_>) {
        const COMMAND_WIDTH: u16 = 18;
        const COLUMN_SPACING: u16 = 2;
        let description_width = area
            .width
            .saturating_sub(2 + 1 + COMMAND_WIDTH + COLUMN_SPACING)
            .max(1) as usize;
        let rows = HELP
            .iter()
            .map(|entry| match *entry {
                Entry::Section(title) => section(title),
                Entry::Command(input, description) => {
                    command(input, description, description_width)
                }
            })
            .collect::<Vec<_>>();
        let border = Style::default().fg(Color::Green);
        let block = Block::default()
            .borders(Borders::ALL)
            .padding(Padding::right(1))
            .border_style(border)
            .title_top(Line::styled(
                " help ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(
                Line::styled(
                    " press any key to close ",
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::ITALIC),
                )
                .right_aligned(),
            );
        let table = Table::new(
            rows,
            [Constraint::Length(COMMAND_WIDTH), Constraint::Min(1)],
        )
        .column_spacing(COLUMN_SPACING)
        .style(Style::default().fg(Color::White))
        .block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(table, area);
    }
}

fn section(title: &'static str) -> Row<'static> {
    Row::new([
        Cell::from(title).style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from(""),
    ])
}

fn command(input: &'static str, description: &'static str, width: usize) -> Row<'static> {
    let lines = wrap_words(description, width);
    Row::new([Cell::from(input), Cell::from(lines.join("\n"))]).height(lines.len() as u16)
}

fn wrap_words(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{Entry, HELP, wrap_words};
    use crate::tui::{input::command, types::WidgetId};

    #[test]
    fn every_command_is_documented() {
        let help = HELP
            .iter()
            .filter_map(|entry| match entry {
                Entry::Command(input, description) => Some(format!("{input} {description}")),
                Entry::Section(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for name in command::NAMES {
            assert!(
                !command::parse(name, WidgetId::Dialog)
                    .is_err_and(|error| error.starts_with("unknown command")),
                "{name} is not a parser command"
            );
            let documented = help
                .split(|c: char| c.is_whitespace() || c == '/' || c == '(' || c == ')')
                .any(|word| word == format!(":{name}"));
            assert!(documented, ":{name} is missing from the help table");
        }
    }

    #[test]
    fn descriptions_wrap_at_word_boundaries() {
        assert_eq!(
            wrap_words("one two three four", 9),
            ["one two", "three", "four"]
        );
    }
}
