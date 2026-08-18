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
        let rows = vec![
            section("VIEWS / EMULATION"),
            command("Space r", "Select runtime view", description_width),
            command(
                "Space i",
                "Select inspect view and pause",
                description_width,
            ),
            command("Space s", "Toggle emulation start/stop", description_width),
            command(
                ":start / :stop",
                "Start or stop emulation",
                description_width,
            ),
            command(
                ":reset",
                "Request an emulated power cycle",
                description_width,
            ),
            section("FOCUS AND MODES"),
            command(
                "Ctrl+c/e/d",
                "Focus console, events, or dialog",
                description_width,
            ),
            command(
                "Ctrl+p/s",
                "Focus primary or secondary inspector",
                description_width,
            ),
            command(
                "i / Esc",
                "Enter insert mode / return to normal mode",
                description_width,
            ),
            command("? / :help", "Open this help window", description_width),
            section("NAVIGATION"),
            command("[#] h/j/k/l", "Move by pane-local units", description_width),
            command(
                "gg / G",
                "Move to first / current or last item",
                description_width,
            ),
            command(
                "Tab",
                "Switch the focused inspector subview",
                description_width,
            ),
            command(
                "/text",
                "Search physical RAM for ASCII text",
                description_width,
            ),
            command(
                "\\de ad be ef",
                "Search physical RAM for hex bytes",
                description_width,
            ),
            command(
                ">location",
                "Goto an address or register",
                description_width,
            ),
            section("COMMANDS"),
            command(":view [r|i]", "Toggle or select a view", description_width),
            command(
                ":set uart 0|1",
                "Select console input UART",
                description_width,
            ),
            command(":set primary", "Select mem or disasm", description_width),
            command(
                ":set secondary",
                "Select reg, peri, or pend",
                description_width,
            ),
            command(":q", "Quit minemu", description_width),
        ];
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
    use super::wrap_words;

    #[test]
    fn descriptions_wrap_at_word_boundaries() {
        assert_eq!(
            wrap_words("one two three four", 9),
            ["one two", "three", "four"]
        );
    }
}
