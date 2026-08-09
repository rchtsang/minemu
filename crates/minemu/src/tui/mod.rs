mod app;
mod input;
mod render;
mod terminal;

use std::{path::PathBuf, time::Duration};

use crossterm::event;

use crate::{CliError, Result};

use app::App;
use terminal::TerminalSession;

/// Runs the interactive terminal UI for one system image.
pub fn run_tui(image: PathBuf, block_media: Option<PathBuf>) -> Result<()> {
    let mut app = App::start(image, block_media)?;
    let result = run_loop(&mut app);
    let _ = app.runtime.shutdown();
    result
}

fn run_loop(app: &mut App) -> Result<()> {
    let mut terminal = TerminalSession::new()?;
    loop {
        app.refresh()?;
        if app.status.lifecycle == minemu_runtime::LifecycleState::Failed {
            return Err(CliError::RuntimeSetup);
        }
        terminal.terminal().draw(|frame| render::draw(frame, app))?;
        if event::poll(Duration::from_millis(50))? && app.handle_event(event::read()?)? {
            break;
        }
    }
    terminal.restore()?;
    Ok(())
}
