use std::{
    collections::VecDeque,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use crossterm::event::{
    Event, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use minemu_runtime::{LifecycleState, RuntimeStatus};
use ratatui::{Frame, layout::Rect};
use tracing::{debug, error, info, warn};

use crate::{
    CliError, Result,
    runner::{SystemSpec, start_runtime},
};

use super::{
    action::Action,
    event::AppEvent,
    input::InputRouter,
    layout::SplitLayout,
    runtime::{PendingResult, RuntimeController},
    types::{DialogMessage, InputMode, SplitId, View, WidgetId},
    widget::{InputContext, RenderContext, TuiWidget},
    widgets::{
        ConsoleWidget, DialogWidget, EventsWidget, HeaderWidget, HelpWidget, HintsWidget,
        InputBarWidget, PrimaryWidget, SecondaryWidget,
    },
};

/// Whether opening the command bar paused a running emulator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandPause {
    /// The command bar did not pause emulation.
    None,
    /// Emulation resumes when the command bar closes.
    ResumeOnExit,
    /// A command took explicit run control, so emulation stays paused on close.
    StayPaused,
}

pub struct App {
    runtime: RuntimeController,
    widgets: Vec<Box<dyn TuiWidget>>,
    view: View,
    focused: WidgetId,
    input: InputRouter,
    layout: SplitLayout,
    actions: VecDeque<Action>,
    quit: bool,
    command_pause: CommandPause,
    last_pulse: Instant,
    terminal_area: Rect,
    dragging: Option<SplitId>,
    help_visible: bool,
}

impl App {
    pub fn start(
        image: PathBuf,
        boot_rom: PathBuf,
        block0_media: Option<PathBuf>,
        block1_media: Option<PathBuf>,
    ) -> Result<Self> {
        let handle = start_runtime(SystemSpec {
            image: &image,
            boot_rom: &boot_rom,
            block_media: [block0_media, block1_media],
            instruction_batch: None,
        })?;
        let mut runtime = RuntimeController::new(handle);
        for _ in 0..500 {
            runtime.refresh_status();
            if runtime.status().lifecycle == LifecycleState::Paused {
                break;
            }
            if runtime.status().lifecycle == LifecycleState::Failed {
                return Err(CliError::Runtime(failure_reason(runtime.status())));
            }
            thread::sleep(Duration::from_millis(2));
        }
        if runtime.status().lifecycle != LifecycleState::Paused {
            error!(
                lifecycle = ?runtime.status().lifecycle,
                "timed out waiting for initial TUI pause"
            );
            return Err(CliError::Runtime(format!(
                "timed out waiting for the initial pause (lifecycle {:?})",
                runtime.status().lifecycle
            )));
        }
        let mut app = Self {
            runtime,
            widgets: vec![
                Box::new(HeaderWidget::default()),
                Box::new(ConsoleWidget::default()),
                Box::new(EventsWidget::default()),
                Box::new(PrimaryWidget::default()),
                Box::new(SecondaryWidget::default()),
                Box::new(DialogWidget::default()),
                Box::new(InputBarWidget),
                Box::new(HintsWidget),
                Box::new(HelpWidget),
            ],
            view: View::Runtime,
            focused: WidgetId::Console,
            input: InputRouter::default(),
            layout: SplitLayout::default(),
            actions: VecDeque::new(),
            quit: false,
            command_pause: CommandPause::None,
            last_pulse: Instant::now(),
            terminal_area: Rect::default(),
            dragging: None,
            help_visible: false,
        };
        app.broadcast(AppEvent::Status(app.runtime.status().clone()));
        app.broadcast(AppEvent::ViewChanged(View::Runtime));
        app.process_actions();
        Ok(app)
    }

    pub fn tick(&mut self) -> Result<()> {
        if let Some(status) = self.runtime.refresh_status() {
            let paused = status.lifecycle == LifecycleState::Paused;
            let breakpoint = paused
                .then(|| status.last_stop.clone())
                .flatten()
                .filter(|stop| stop.starts_with("breakpoint "));
            self.broadcast(AppEvent::Status(status));
            if paused {
                if let Some(stop) = breakpoint {
                    self.show_message(DialogMessage::info(stop));
                }
                self.refresh_inspectors();
            }
        }
        for result in self.runtime.poll() {
            match result {
                PendingResult::Ready { target, inspection } => {
                    self.target(target, AppEvent::Inspection(inspection));
                }
                PendingResult::Failed {
                    target,
                    request,
                    error: detail,
                } => {
                    warn!(?target, ?request, error = %detail, "TUI inspection failed");
                    if target == WidgetId::Primary {
                        self.target(target, AppEvent::InspectionFailed(request, detail));
                    } else {
                        self.show_message(DialogMessage::error(format!(
                            "{request:?} failed: {detail}"
                        )));
                    }
                }
            }
        }
        if self.last_pulse.elapsed() >= Duration::from_millis(100) {
            self.last_pulse = Instant::now();
            self.broadcast(AppEvent::Pulse);
        }
        self.process_actions();
        if self.runtime.status().lifecycle == LifecycleState::Failed {
            return Err(CliError::RuntimeFailed(failure_reason(
                self.runtime.status(),
            )));
        }
        Ok(())
    }

    pub fn handle_event(&mut self, event: Event) {
        if self.help_visible && matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press)
        {
            self.help_visible = false;
            return;
        }
        if let Event::Mouse(mouse) = event {
            self.handle_mouse(mouse);
            self.process_actions();
            return;
        }
        self.actions.extend(self.input.route(event, self.focused));
        self.process_actions();
    }

    pub fn render(&mut self, frame: &mut Frame) {
        self.terminal_area = frame.area();
        let areas = self.layout.areas(frame.area(), self.view, self.focused);
        let input = self.input.display();
        let context = RenderContext {
            view: self.view,
            mode: self.input.mode(),
            focused: self.focused,
            input: &input,
            ticks: self.runtime.status().machine.ticks,
        };
        for widget in &mut self.widgets {
            if widget.id() == WidgetId::Help && !self.help_visible {
                continue;
            }
            if widget.visible(self.view)
                && let Some(area) = areas.get(&widget.id())
            {
                widget.render(frame, *area, &context);
            }
        }
    }

    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn shutdown(&self) -> Result<()> {
        self.runtime.shutdown().map_err(|error| {
            error!(error = %error, "failed to shut down TUI runtime");
            CliError::RuntimeFailed(format!("shutdown: {error}"))
        })
    }

    fn process_actions(&mut self) {
        while let Some(action) = self.actions.pop_front() {
            debug!(?action, "processing TUI action");
            self.process_action(action);
        }
    }

    fn process_action(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.keep_paused_after_command();
                self.quit = true;
            }
            Action::SelectView(view) => self.select_view(view),
            Action::ToggleView => self.select_view(self.view.toggled()),
            Action::Focus(target) => {
                if self.visible_focus(target) {
                    self.focused = target;
                }
            }
            Action::SetMode(mode) => self.set_mode(mode),
            Action::ToggleRun => {
                self.keep_paused_after_command();
                if self.command_pause != CommandPause::None {
                    self.stop();
                } else {
                    match self.runtime.status().lifecycle {
                        LifecycleState::Running => self.stop(),
                        LifecycleState::Paused => self.start_emulation(None),
                        state => self.show_message(DialogMessage::error(format!(
                            "cannot toggle emulation while {state:?}"
                        ))),
                    }
                }
            }
            Action::Start(instruction_limit) => {
                self.keep_paused_after_command();
                self.start_emulation(instruction_limit);
            }
            Action::Stop => {
                self.keep_paused_after_command();
                self.stop();
            }
            Action::Reset => {
                self.keep_paused_after_command();
                if let Err(error) = self.runtime.reset() {
                    self.runtime_error("reset", error.to_string());
                } else {
                    self.broadcast(AppEvent::Reset);
                    self.show_message(DialogMessage::info("emulator reset requested"));
                    self.refresh_inspectors();
                }
            }
            Action::SendUart(port, bytes) => self.runtime.send_uart(port, &bytes),
            Action::RequestInspection { target, request } => {
                if let Err(error) = self.runtime.request(target, request.clone()) {
                    self.runtime_error(&format!("{request:?}"), error.to_string());
                }
            }
            Action::WidgetKey { target, key } => {
                let context = InputContext {
                    mode: self.input.mode(),
                };
                if let Some(widget) = self.widget_mut(target) {
                    let actions = widget.handle_key(key, &context);
                    self.actions.extend(actions);
                }
            }
            Action::InsertText { target, text } => self.target(target, AppEvent::InsertText(text)),
            Action::Navigate { target, motion } => {
                self.target(target, AppEvent::Navigate(motion));
            }
            Action::Goto { target, value } => self.target(target, AppEvent::Goto(value)),
            Action::SetUart(port) => {
                self.broadcast(AppEvent::UartSelected(port));
                self.show_message(DialogMessage::info(format!(
                    "console input set to UART{}",
                    port.index()
                )));
            }
            Action::SetPrimary(subview) => self.broadcast(AppEvent::PrimarySelected(subview)),
            Action::SetSecondary(subview) => {
                self.broadcast(AppEvent::SecondarySelected(subview));
            }
            Action::Refresh(target) => self.target(target, AppEvent::Refresh),
            Action::ShowMessage(message) => self.show_message(message),
            Action::ShowHelp => self.show_help(),
            Action::Resize { split, percent } => self.layout.resize(split, percent),
        }
    }

    fn select_view(&mut self, view: View) {
        if view == View::Inspect {
            self.keep_paused_after_command();
        }
        if self.view == view {
            return;
        }
        info!(?view, "selecting TUI view");
        self.view = view;
        self.focused = match view {
            View::Runtime => WidgetId::Console,
            View::Inspect => WidgetId::Primary,
        };
        if view == View::Inspect && self.runtime.status().lifecycle == LifecycleState::Running {
            self.stop();
        }
        self.broadcast(AppEvent::ViewChanged(view));
    }

    fn set_mode(&mut self, mode: InputMode) {
        let previous = self.input.mode();
        if previous == mode {
            return;
        }
        if mode == InputMode::Command
            && self.runtime.status().lifecycle == LifecycleState::Running
            && self.runtime.pause().is_ok()
        {
            self.command_pause = CommandPause::ResumeOnExit;
        }
        self.input.set_mode(mode);
        if previous == InputMode::Command && mode == InputMode::Normal {
            if self.command_pause == CommandPause::ResumeOnExit {
                self.start_emulation(None);
            }
            self.command_pause = CommandPause::None;
        }
    }

    /// Records that a command took explicit run control, so closing the
    /// command bar must not resume emulation it paused.
    fn keep_paused_after_command(&mut self) {
        if self.command_pause == CommandPause::ResumeOnExit {
            self.command_pause = CommandPause::StayPaused;
        }
    }

    fn start_emulation(&mut self, instruction_limit: Option<std::num::NonZeroU64>) {
        if self.runtime.status().lifecycle != LifecycleState::Paused
            && self.command_pause == CommandPause::None
        {
            self.show_message(DialogMessage::error("emulation is not paused"));
            return;
        }
        if let Err(error) = self.runtime.resume(instruction_limit) {
            self.runtime_error("start", error.to_string());
        } else {
            let message = instruction_limit.map_or_else(
                || "emulation started".into(),
                |count| format!("emulation started for {} instructions", count.get()),
            );
            self.show_message(DialogMessage::info(message));
        }
    }

    fn stop(&mut self) {
        if self.runtime.status().lifecycle != LifecycleState::Running {
            if self.runtime.status().lifecycle == LifecycleState::Paused {
                self.refresh_inspectors();
            } else {
                self.show_message(DialogMessage::error("emulation is not running"));
            }
            return;
        }
        if let Err(error) = self.runtime.pause() {
            self.runtime_error("stop", error.to_string());
        } else {
            self.show_message(DialogMessage::info("emulation stopped"));
            self.refresh_inspectors();
        }
    }

    fn widget_mut(&mut self, id: WidgetId) -> Option<&mut Box<dyn TuiWidget>> {
        self.widgets.iter_mut().find(|widget| widget.id() == id)
    }

    /// Re-requests inspection data for both inspect-view panes.
    fn refresh_inspectors(&mut self) {
        self.actions.push_back(Action::Refresh(WidgetId::Primary));
        self.actions.push_back(Action::Refresh(WidgetId::Secondary));
    }

    fn target(&mut self, target: WidgetId, event: AppEvent) {
        if let Some(widget) = self.widget_mut(target) {
            let actions = widget.update(&event);
            self.actions.extend(actions);
        }
    }

    fn broadcast(&mut self, event: AppEvent) {
        for widget in &mut self.widgets {
            self.actions.extend(widget.update(&event));
        }
    }

    fn show_message(&mut self, message: DialogMessage) {
        self.target(WidgetId::Dialog, AppEvent::Dialog(message));
    }

    fn show_help(&mut self) {
        self.help_visible = true;
    }

    fn runtime_error(&mut self, operation: &str, detail: String) {
        error!(operation, error = %detail, "recoverable TUI runtime operation failed");
        self.show_message(DialogMessage::error(format!(
            "{operation} failed: {detail}"
        )));
    }

    fn visible_focus(&self, target: WidgetId) -> bool {
        matches!(
            (self.view, target),
            (
                View::Runtime,
                WidgetId::Console | WidgetId::Events | WidgetId::Dialog
            ) | (
                View::Inspect,
                WidgetId::Primary | WidgetId::Secondary | WidgetId::Dialog
            )
        )
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if mouse.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let split = match self.view {
                    View::Runtime => SplitId::RuntimeMain,
                    View::Inspect => SplitId::InspectMain,
                };
                let column = self.layout.split_column(self.terminal_area, self.view);
                if mouse.column.abs_diff(column) <= 2 {
                    self.dragging = Some(split);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(action) = plain_click_action(
                    &self.layout,
                    self.terminal_area,
                    self.view,
                    self.focused,
                    self.input.mode(),
                    mouse.column,
                    mouse.row,
                ) {
                    self.actions.push_back(action);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging.is_some() => {
                let relative = mouse.column.saturating_sub(self.terminal_area.x);
                if let Some(percent) = relative
                    .saturating_mul(100)
                    .checked_div(self.terminal_area.width)
                {
                    self.actions.push_back(Action::Resize {
                        split: self.dragging.expect("checked drag split"),
                        percent,
                    });
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.dragging = None,
            _ => {}
        }
    }
}

fn plain_click_action(
    layout: &SplitLayout,
    terminal_area: Rect,
    view: View,
    focused: WidgetId,
    mode: InputMode,
    column: u16,
    row: u16,
) -> Option<Action> {
    if mode == InputMode::Insert {
        return None;
    }
    let areas = layout.areas(terminal_area, view, focused);
    if let Some(header) = areas.get(&WidgetId::Header)
        && let Some(view) = HeaderWidget::view_at(*header, column, row)
    {
        return Some(Action::SelectView(view));
    }
    layout
        .focus_at(terminal_area, view, focused, column, row)
        .map(Action::Focus)
}

/// Describes why a failed runtime stopped, for the error printed after the TUI exits.
fn failure_reason(status: &RuntimeStatus) -> String {
    match (&status.last_error, &status.last_stop) {
        (Some(error), _) => error.clone(),
        (None, Some(stop)) => format!("stopped after {stop}"),
        (None, None) => "no reason was reported".into(),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{failure_reason, plain_click_action};
    use crate::tui::{
        action::Action,
        layout::SplitLayout,
        types::{InputMode, View, WidgetId},
    };

    #[test]
    fn plain_clicks_use_exact_tab_and_pane_geometry() {
        let layout = SplitLayout::default();
        let area = Rect::new(4, 2, 100, 30);

        assert!(matches!(
            plain_click_action(
                &layout,
                area,
                View::Runtime,
                WidgetId::Console,
                InputMode::Normal,
                16,
                3,
            ),
            Some(Action::SelectView(View::Inspect))
        ));
        assert!(matches!(
            plain_click_action(
                &layout,
                area,
                View::Runtime,
                WidgetId::Console,
                InputMode::Normal,
                16,
                5,
            ),
            Some(Action::Focus(WidgetId::Console))
        ));
        assert!(
            plain_click_action(
                &layout,
                area,
                View::Runtime,
                WidgetId::Console,
                InputMode::Normal,
                16,
                2,
            )
            .is_none()
        );
    }

    #[test]
    fn insert_mode_blocks_tab_and_pane_clicks() {
        let layout = SplitLayout::default();
        let area = Rect::new(4, 2, 100, 30);

        for (column, row) in [(16, 3), (80, 5)] {
            assert!(
                plain_click_action(
                    &layout,
                    area,
                    View::Runtime,
                    WidgetId::Console,
                    InputMode::Insert,
                    column,
                    row,
                )
                .is_none()
            );
        }
    }

    #[test]
    fn failure_reason_prefers_the_runtime_error() {
        let mut status = minemu_runtime::RuntimeStatus {
            lifecycle: minemu_runtime::LifecycleState::Failed,
            machine: minemu_core::Machine::default().status(),
            last_stop: Some("Instructions".into()),
            last_error: Some("backend READ_UNMAPPED".into()),
        };
        assert_eq!(failure_reason(&status), "backend READ_UNMAPPED");
        status.last_error = None;
        assert_eq!(failure_reason(&status), "stopped after Instructions");
        status.last_stop = None;
        assert_eq!(failure_reason(&status), "no reason was reported");
    }
}
