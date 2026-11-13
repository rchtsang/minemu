//! Shared TUI colors and styles.

use ratatui::style::{Color, Modifier, Style};

/// Highlight color for focused panes, active tabs, and the cursor.
pub const ACCENT: Color = Color::LightYellow;
/// Color for unfocused borders and secondary text.
pub const MUTED: Color = Color::DarkGray;

/// Title and border style of the focused pane or active tab.
pub fn focused() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Style of unfocused borders and secondary text.
pub fn muted() -> Style {
    Style::default().fg(MUTED)
}

/// Style of the selected memory byte or current instruction.
pub fn selected() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}
