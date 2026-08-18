use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Clear, Row, Table},
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
        let rows = vec![
            section("VIEWS AND EMULATION"),
            command("Space r", "Select runtime view"),
            command("Space i", "Select inspect view and pause"),
            command("Space s", "Toggle emulation start/stop"),
            command(":start / :stop", "Start or stop emulation"),
            command(":reset", "Request an emulated power cycle"),
            section("FOCUS AND MODES"),
            command("Ctrl+c/e/d", "Focus console, events, or dialog"),
            command("Ctrl+p/s", "Focus primary or secondary inspector"),
            command("i / Esc", "Enter insert mode / return to normal mode"),
            command("? / :help", "Open this help window"),
            section("NAVIGATION"),
            command("[#] h/j/k/l", "Move by pane-local units"),
            command("gg / G", "Move to first / current or last item"),
            command("Tab", "Switch the focused inspector subview"),
            command("/text", "Search physical RAM for ASCII text"),
            command("\\de ad be ef", "Search physical RAM for hex bytes"),
            command(">location", "Goto an address or register"),
            section("COMMANDS"),
            command(":view [r|i]", "Toggle or select a view"),
            command(":set uart 0|1", "Select console input UART"),
            command(":set primary", "Select mem or disasm"),
            command(":set secondary", "Select reg, peri, or pend"),
            command(":q", "Quit minemu"),
            Row::new([Cell::from(""), Cell::from("Press any key to close")]),
        ];
        let border = Style::default().fg(Color::Green);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title(" help ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        let table = Table::new(rows, [Constraint::Length(22), Constraint::Min(24)])
            .column_spacing(2)
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

fn command(input: &'static str, description: &'static str) -> Row<'static> {
    Row::new([Cell::from(input), Cell::from(description)])
}
