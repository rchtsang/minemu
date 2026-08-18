use std::{
    collections::VecDeque,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use crossterm::event::{
    Event, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use minemu_runtime::LifecycleState;
use ratatui::{Frame, layout::Rect};
use tracing::{debug, error, info, warn};

use crate::{CliError, Result, runner::start_runtime};

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

pub struct App {
    runtime: RuntimeController,
    widgets: Vec<Box<dyn TuiWidget>>,
    view: View,
    focused: WidgetId,
    input: InputRouter,
    layout: SplitLayout,
    actions: VecDeque<Action>,
    quit: bool,
    resume_after_input: bool,
    suppress_input_resume: bool,
    last_pulse: Instant,
    terminal_area: Rect,
    dragging: Option<SplitId>,
    help_visible: bool,
}

impl App {
    pub fn start(image: PathBuf, block_media: Option<PathBuf>) -> Result<Self> {
        let handle = start_runtime(&image, block_media)?;
        let mut runtime = RuntimeController::new(handle);
        runtime.pause().map_err(|error| {
            error!(error = %error, "failed to request initial TUI pause");
            CliError::RuntimeSetup
        })?;
        for _ in 0..500 {
            runtime.refresh_status();
            if runtime.status().lifecycle == LifecycleState::Paused {
                break;
            }
            if runtime.status().lifecycle == LifecycleState::Failed {
                return Err(CliError::RuntimeSetup);
            }
            thread::sleep(Duration::from_millis(2));
        }
        if runtime.status().lifecycle != LifecycleState::Paused {
            error!(
                lifecycle = ?runtime.status().lifecycle,
                "timed out waiting for initial TUI pause"
            );
            return Err(CliError::RuntimeSetup);
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
            resume_after_input: false,
            suppress_input_resume: false,
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
            self.broadcast(AppEvent::Status(status));
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
                    self.show_message(DialogMessage::error(format!(
                        "{request:?} failed: {detail}"
                    )));
                }
            }
        }
        if self.last_pulse.elapsed() >= Duration::from_millis(100) {
            self.last_pulse = Instant::now();
            self.broadcast(AppEvent::Pulse);
        }
        self.process_actions();
        if self.runtime.status().lifecycle == LifecycleState::Failed {
            return Err(CliError::RuntimeSetup);
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
        self.actions
            .extend(self.input.route(event, self.focused, self.view));
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
            CliError::RuntimeSetup
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
                self.suppress_input_resume = true;
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
                self.suppress_input_resume = true;
                if self.resume_after_input {
                    self.stop();
                } else {
                    match self.runtime.status().lifecycle {
                        LifecycleState::Running => self.stop(),
                        LifecycleState::Paused => self.start_emulation(),
                        state => self.show_message(DialogMessage::error(format!(
                            "cannot toggle emulation while {state:?}"
                        ))),
                    }
                }
            }
            Action::Start => {
                self.suppress_input_resume = true;
                self.start_emulation();
            }
            Action::Stop => {
                self.suppress_input_resume = true;
                self.stop();
            }
            Action::Reset => {
                self.suppress_input_resume = true;
                if let Err(error) = self.runtime.reset() {
                    self.runtime_error("reset", error.to_string());
                } else {
                    self.show_message(DialogMessage::info("emulator reset requested"));
                    self.actions.push_back(Action::Refresh(WidgetId::Primary));
                    self.actions.push_back(Action::Refresh(WidgetId::Secondary));
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
                if let Some(widget) = self.widgets.iter_mut().find(|widget| widget.id() == target) {
                    self.actions.extend(widget.handle_key(key, &context));
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
                    if port == minemu_runtime::UartPort::Uart0 {
                        0
                    } else {
                        1
                    }
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
            self.suppress_input_resume = true;
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
            self.suppress_input_resume = true;
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
            self.resume_after_input = true;
            self.suppress_input_resume = false;
        }
        self.input.set_mode(mode);
        if previous == InputMode::Command && mode == InputMode::Normal {
            if self.resume_after_input && !self.suppress_input_resume {
                self.start_emulation();
            }
            self.resume_after_input = false;
            self.suppress_input_resume = false;
        }
    }

    fn start_emulation(&mut self) {
        if self.runtime.status().lifecycle != LifecycleState::Paused && !self.resume_after_input {
            self.show_message(DialogMessage::error("emulation is not paused"));
            return;
        }
        if let Err(error) = self.runtime.resume() {
            self.runtime_error("start", error.to_string());
        } else {
            self.show_message(DialogMessage::info("emulation started"));
        }
    }

    fn stop(&mut self) {
        if self.runtime.status().lifecycle != LifecycleState::Running {
            if self.runtime.status().lifecycle != LifecycleState::Paused {
                self.show_message(DialogMessage::error("emulation is not running"));
            }
            return;
        }
        if let Err(error) = self.runtime.pause() {
            self.runtime_error("stop", error.to_string());
        } else {
            self.show_message(DialogMessage::info("emulation stopped"));
        }
    }

    fn target(&mut self, target: WidgetId, event: AppEvent) {
        if let Some(widget) = self.widgets.iter_mut().find(|widget| widget.id() == target) {
            self.actions.extend(widget.update(&event));
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
