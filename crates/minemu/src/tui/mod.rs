mod action;
mod app;
mod event;
mod input;
mod layout;
mod runtime;
mod terminal;
mod types;
mod widget;
mod widgets;

use std::{path::PathBuf, time::Duration};

use crossterm::event as terminal_event;

use crate::Result;

use app::App;
use terminal::TerminalSession;

/// Runs the interactive terminal UI for one system image.
pub fn run_tui(image: PathBuf, block_media: Option<PathBuf>) -> Result<()> {
    let mut app = App::start(image, block_media)?;
    let result = run_loop(&mut app);
    let _ = app.shutdown();
    result
}

fn run_loop(app: &mut App) -> Result<()> {
    let mut terminal = TerminalSession::new()?;
    loop {
        app.tick()?;
        terminal.terminal().draw(|frame| app.render(frame))?;
        if terminal_event::poll(Duration::from_millis(50))? {
            app.handle_event(terminal_event::read()?);
        }
        if app.should_quit() {
            break;
        }
    }
    terminal.restore()?;
    Ok(())
}
