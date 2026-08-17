use std::{path::PathBuf, thread, time::Duration};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use minemu_platform::{InspectionRequest, MemRegion, PhysicalAddress, PhysicalRange};
use minemu_runtime::{
    ExecutionInspection, LifecycleState, RuntimeError, RuntimeHandle, RuntimeInspection,
    RuntimeInspectionRequest, RuntimeStatus, UartPort,
};
use tracing::{debug, error, info, warn};

use crate::{CliError, Result, runner::start_runtime};

use super::input::{Motion, MotionDecoder};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    Runtime,
    Introspection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Console,
    Events,
    Memory,
    Disassembly,
    Cpu,
    Hardware,
}

/// Immutable data assembled from runtime snapshots and TUI-local navigation state.
pub struct App {
    pub runtime: RuntimeHandle,
    pub view: View,
    pub focus: Focus,
    pub active_uart: UartPort,
    pub status: RuntimeStatus,
    pub peripherals: Option<minemu_platform::PeripheralsInspection>,
    pub events: Vec<minemu_platform::ObservableEvent>,
    pub memory_range: PhysicalRange,
    pub memory: Vec<u8>,
    pub execution: Option<ExecutionInspection>,
    pub command: Option<String>,
    pub show_help: bool,
    command_paused: bool,
    motion: MotionDecoder,
    pub event_offset: usize,
}

impl App {
    pub fn start(image: PathBuf, block_media: Option<PathBuf>) -> Result<Self> {
        let runtime = start_runtime(&image, block_media)?;
        let memory_range =
            PhysicalRange::new(MemRegion::Ram.base(), 256).map_err(|_| CliError::RuntimeSetup)?;
        let mut app = Self {
            status: runtime.status(),
            runtime,
            view: View::Runtime,
            focus: Focus::Console,
            active_uart: UartPort::Uart0,
            peripherals: None,
            events: Vec::new(),
            memory_range,
            memory: Vec::new(),
            execution: None,
            command: None,
            show_help: false,
            command_paused: false,
            motion: MotionDecoder::default(),
            event_offset: 0,
        };
        app.refresh_runtime()?;
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.status = self.runtime.status();
        match self.view {
            View::Runtime => self.refresh_runtime(),
            View::Introspection => self.refresh_introspection(),
        }
    }

    pub fn handle_event(&mut self, event: Event) -> Result<bool> {
        match event {
            Event::Paste(text) if self.focus == Focus::Console && self.command.is_none() => {
                self.runtime.send_uart(self.active_uart, text.as_bytes());
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => return self.handle_key(key),
            _ => {}
        }
        Ok(false)
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        if self.show_help {
            debug!(view = ?self.view, focus = ?self.focus, "dismissed TUI help overlay");
            self.show_help = false;
            return Ok(false);
        }
        if let Some(command) = &mut self.command {
            match key.code {
                KeyCode::Esc => {
                    info!(command = %command, "cancelled TUI command");
                    self.command = None;
                    self.resume_after_command()?;
                }
                KeyCode::Enter => {
                    let command = std::mem::take(command);
                    self.command = None;
                    return self.execute_command(&command);
                }
                KeyCode::Backspace => {
                    command.pop();
                }
                KeyCode::Char(character) => command.push(character),
                _ => {}
            }
            return Ok(false);
        }
        if self.focus == Focus::Console {
            match key.code {
                KeyCode::Esc => self.focus = Focus::Events,
                KeyCode::Enter => self.runtime.send_uart(self.active_uart, b"\n"),
                KeyCode::Backspace => self.runtime.send_uart(self.active_uart, &[8]),
                KeyCode::Char(character) => {
                    let mut bytes = [0; 4];
                    self.runtime.send_uart(
                        self.active_uart,
                        character.encode_utf8(&mut bytes).as_bytes(),
                    );
                }
                _ => {}
            }
            return Ok(false);
        }
        match key.code {
            KeyCode::Char(':') => self.begin_command()?,
            KeyCode::Esc => self.motion.reset(),
            KeyCode::Tab => self.next_focus(),
            code => {
                if let Some(motion) = self.motion.push(code) {
                    self.apply_motion(motion)?;
                }
            }
        }
        Ok(false)
    }

    fn execute_command(&mut self, command: &str) -> Result<bool> {
        info!(
            command,
            view = ?self.view,
            focus = ?self.focus,
            lifecycle = ?self.status.lifecycle,
            "executing TUI command"
        );
        let mut words = command.split_whitespace();
        let mut resume_after_command = self.command_paused;
        match words.next() {
            Some("pause") => {
                self.runtime
                    .pause()
                    .map_err(|error| self.runtime_error("pause command", error))?;
                resume_after_command = false;
            }
            Some("resume") => {
                self.runtime
                    .resume()
                    .map_err(|error| self.runtime_error("resume command", error))?;
                resume_after_command = false;
            }
            Some("reset") => {
                self.runtime
                    .reset()
                    .map_err(|error| self.runtime_error("reset command", error))?;
                resume_after_command = false;
            }
            Some("quit") | Some("q") => return Ok(true),
            Some("view") => match words.next() {
                Some("runtime") => self.view = View::Runtime,
                Some("inspect") | Some("introspection") => {
                    self.enter_introspection()?;
                    resume_after_command = false;
                }
                _ => return Err(CliError::Assertion("usage: :view runtime|inspect".into())),
            },
            Some("focus") => self.focus = parse_focus(words.next())?,
            Some("mem") => {
                let address = parse_address(words.next())?;
                self.memory_range = PhysicalRange::new(PhysicalAddress::new(address), 256)
                    .map_err(|_| CliError::Assertion("memory range is invalid".into()))?;
                self.refresh_introspection()?;
            }
            Some("uart") => {
                self.active_uart = match words.next() {
                    Some("0") => UartPort::Uart0,
                    Some("1") => UartPort::Uart1,
                    _ => return Err(CliError::Assertion("usage: :uart 0|1".into())),
                };
            }
            Some("help") => self.show_help = true,
            Some(_) | None => return Err(CliError::Assertion("unknown TUI command".into())),
        }
        if resume_after_command {
            debug!("resuming guest after non-lifecycle TUI command");
            self.runtime
                .resume()
                .map_err(|error| self.runtime_error("resume after command", error))?;
        }
        self.command_paused = false;
        self.refresh()?;
        Ok(false)
    }

    fn begin_command(&mut self) -> Result<()> {
        info!(
            view = ?self.view,
            focus = ?self.focus,
            lifecycle = ?self.status.lifecycle,
            "starting TUI command"
        );
        if self.status.lifecycle == LifecycleState::Running {
            self.pause_and_wait()?;
            self.command_paused = true;
        }
        self.command = Some(String::new());
        Ok(())
    }

    fn resume_after_command(&mut self) -> Result<()> {
        if self.command_paused {
            debug!("resuming guest after cancelled TUI command");
            self.runtime
                .resume()
                .map_err(|error| self.runtime_error("resume after command cancellation", error))?;
            self.command_paused = false;
        }
        self.refresh()
    }

    fn enter_introspection(&mut self) -> Result<()> {
        info!(
            lifecycle = ?self.status.lifecycle,
            tick = self.status.machine.ticks,
            "entering TUI introspection view"
        );
        if self.status.lifecycle == LifecycleState::Running {
            self.pause_and_wait()?;
        }
        self.view = View::Introspection;
        self.focus = Focus::Memory;
        self.refresh_introspection()
    }

    fn pause_and_wait(&mut self) -> Result<()> {
        debug!("requesting guest pause for TUI operation");
        self.runtime
            .pause()
            .map_err(|error| self.runtime_error("pause request", error))?;
        for _ in 0..500 {
            if self.runtime.status().lifecycle == LifecycleState::Paused {
                self.status = self.runtime.status();
                debug!(
                    tick = self.status.machine.ticks,
                    "guest paused for TUI operation"
                );
                return Ok(());
            }
            thread::sleep(Duration::from_millis(2));
        }
        warn!(lifecycle = ?self.runtime.status().lifecycle, "timed out waiting for guest pause");
        Err(CliError::RuntimeSetup)
    }

    fn refresh_runtime(&mut self) -> Result<()> {
        self.status = self.runtime.status();
        debug!(
            request = ?InspectionRequest::Peripherals,
            lifecycle = ?self.status.lifecycle,
            tick = self.status.machine.ticks,
            "requesting TUI peripheral snapshot"
        );
        if let RuntimeInspection::Peripherals(peripherals) = self
            .runtime
            .inspect(InspectionRequest::Peripherals)
            .map_err(|error| self.runtime_error("peripheral inspection", error))?
        {
            self.peripherals = Some(peripherals);
        }
        if let RuntimeInspection::Events(events) =
            self.runtime
                .inspect(InspectionRequest::Events)
                .map_err(|error| self.runtime_error("event inspection", error))?
        {
            self.events = events;
        }
        Ok(())
    }

    fn refresh_introspection(&mut self) -> Result<()> {
        self.status = self.runtime.status();
        if self.status.lifecycle != LifecycleState::Paused {
            return Ok(());
        }
        let memory_request = RuntimeInspectionRequest::LiveMemory(self.memory_range);
        debug!(
            request = ?memory_request,
            tick = self.status.machine.ticks,
            "requesting TUI live-memory snapshot"
        );
        if let RuntimeInspection::LiveMemory(range, bytes) = self
            .runtime
            .request_inspection(memory_request)
            .map_err(|error| self.runtime_error("live-memory inspection request", error))?
            .recv()
            .map_err(|error| {
                self.inspection_receive_error("live-memory inspection response", error)
            })?
            .map_err(|error| self.runtime_error("live-memory inspection", error))?
        {
            self.memory_range = range;
            self.memory = bytes;
        }
        let execution_request = RuntimeInspectionRequest::Execution {
            before: 32,
            after: 96,
        };
        debug!(
            request = ?execution_request,
            tick = self.status.machine.ticks,
            "requesting TUI execution snapshot"
        );
        if let RuntimeInspection::Execution(execution) = self
            .runtime
            .request_inspection(execution_request)
            .map_err(|error| self.runtime_error("execution inspection request", error))?
            .recv()
            .map_err(|error| self.inspection_receive_error("execution inspection response", error))?
            .map_err(|error| self.runtime_error("execution inspection", error))?
        {
            self.execution = Some(execution);
        }
        self.refresh_runtime()
    }

    fn runtime_error(&self, operation: &'static str, source: RuntimeError) -> CliError {
        error!(
            operation,
            view = ?self.view,
            focus = ?self.focus,
            lifecycle = ?self.status.lifecycle,
            tick = self.status.machine.ticks,
            error = %source,
            "TUI runtime operation failed"
        );
        CliError::RuntimeSetup
    }

    fn inspection_receive_error(
        &self,
        operation: &'static str,
        source: std::sync::mpsc::RecvError,
    ) -> CliError {
        error!(
            operation,
            view = ?self.view,
            lifecycle = ?self.status.lifecycle,
            tick = self.status.machine.ticks,
            error = %source,
            "TUI inspection response channel closed"
        );
        CliError::RuntimeSetup
    }

    fn next_focus(&mut self) {
        self.focus = match (self.view, self.focus) {
            (View::Runtime, Focus::Console) => Focus::Events,
            (View::Runtime, _) => Focus::Console,
            (View::Introspection, Focus::Memory) => Focus::Disassembly,
            (View::Introspection, Focus::Disassembly) => Focus::Cpu,
            (View::Introspection, Focus::Cpu) => Focus::Hardware,
            (View::Introspection, _) => Focus::Memory,
        };
    }

    fn apply_motion(&mut self, motion: Motion) -> Result<()> {
        match self.focus {
            Focus::Events => nav_event_offset(&mut self.event_offset, self.events.len(), motion),
            Focus::Memory => nav_memory_range(&mut self.memory_range, motion)?,
            Focus::Disassembly => nav_memory_range(&mut self.memory_range, motion)?,
            Focus::Cpu | Focus::Hardware => {}
            Focus::Console => unreachable!("console motions are handled before decoding"),
        }
        if self.view == View::Introspection {
            self.refresh_introspection()?;
        }
        Ok(())
    }
}

fn parse_focus(value: Option<&str>) -> Result<Focus> {
    match value {
        Some("console") => Ok(Focus::Console),
        Some("events") => Ok(Focus::Events),
        Some("memory") => Ok(Focus::Memory),
        Some("disasm") | Some("disassembly") => Ok(Focus::Disassembly),
        Some("cpu") => Ok(Focus::Cpu),
        Some("hardware") => Ok(Focus::Hardware),
        _ => Err(CliError::Assertion("unknown focus pane".into())),
    }
}

fn parse_address(value: Option<&str>) -> Result<u32> {
    let value =
        value.ok_or_else(|| CliError::Assertion("usage: :mem <physical-address>".into()))?;
    let value = value.strip_prefix("0x").unwrap_or(value);
    u32::from_str_radix(value, 16)
        .map_err(|_| CliError::Assertion("invalid physical address".into()))
}

fn nav_event_offset(offset: &mut usize, length: usize, motion: Motion) {
    let count = match motion {
        Motion::Left(count)
        | Motion::Down(count)
        | Motion::Up(count)
        | Motion::Right(count)
        | Motion::NextItem(count)
        | Motion::EndItem(count) => count,
        Motion::Top => {
            *offset = 0;
            return;
        }
        Motion::Bottom => {
            *offset = length.saturating_sub(1);
            return;
        }
    };
    match motion {
        Motion::Up(_) | Motion::Left(_) => *offset = offset.saturating_sub(count),
        _ => *offset = (*offset + count).min(length.saturating_sub(1)),
    }
}

fn nav_memory_range(range: &mut PhysicalRange, motion: Motion) -> Result<()> {
    let count = match motion {
        Motion::Left(count)
        | Motion::Down(count)
        | Motion::Up(count)
        | Motion::Right(count)
        | Motion::NextItem(count)
        | Motion::EndItem(count) => count as u32,
        Motion::Top => {
            *range = PhysicalRange::new(MemRegion::Ram.base(), range.length())
                .map_err(|_| CliError::RuntimeSetup)?;
            return Ok(());
        }
        Motion::Bottom => {
            let start = MemRegion::Ram.base().get() + MemRegion::Ram.size() - range.length();
            *range = PhysicalRange::new(PhysicalAddress::new(start), range.length())
                .map_err(|_| CliError::RuntimeSetup)?;
            return Ok(());
        }
    };
    let step: u32 = match motion {
        Motion::Left(_) | Motion::Right(_) => 1,
        Motion::Up(_) | Motion::Down(_) => 16,
        Motion::NextItem(_) | Motion::EndItem(_) => 4,
        Motion::Top | Motion::Bottom => unreachable!(),
    };
    let min = MemRegion::Ram.base().get();
    let max = min + MemRegion::Ram.size() - range.length();
    let current = range.start().get();
    let address = match motion {
        Motion::Left(_) | Motion::Up(_) => {
            current.saturating_sub(step.saturating_mul(count)).max(min)
        }
        Motion::EndItem(_) => (current / 4).saturating_mul(4).saturating_add(3).min(max),
        _ => current.saturating_add(step.saturating_mul(count)).min(max),
    };
    *range = PhysicalRange::new(PhysicalAddress::new(address), range.length())
        .map_err(|_| CliError::RuntimeSetup)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use minemu_platform::{MemRegion, PhysicalRange};

    use super::{Focus, Motion, nav_event_offset, nav_memory_range, parse_address, parse_focus};

    #[test]
    fn parses_tui_command_arguments() {
        assert_eq!(parse_focus(Some("disasm")).unwrap(), Focus::Disassembly);
        assert_eq!(parse_address(Some("0x40000010")).unwrap(), 0x4000_0010);
        assert!(parse_focus(Some("unknown")).is_err());
        assert!(parse_address(Some("not-an-address")).is_err());
    }

    #[test]
    fn applies_pane_local_navigation() {
        let mut range = PhysicalRange::new(MemRegion::Ram.base(), 32).unwrap();
        nav_memory_range(&mut range, Motion::Down(2)).unwrap();
        assert_eq!(range.start().get(), MemRegion::Ram.base().get() + 32);
        nav_memory_range(&mut range, Motion::NextItem(3)).unwrap();
        assert_eq!(range.start().get(), MemRegion::Ram.base().get() + 44);

        let mut event_offset = 2;
        nav_event_offset(&mut event_offset, 10, Motion::Top);
        assert_eq!(event_offset, 0);
        nav_event_offset(&mut event_offset, 10, Motion::Bottom);
        assert_eq!(event_offset, 9);
    }
}
