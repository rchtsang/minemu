//! Thread-owned runtime service for the concrete Unicorn machine.

use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use minemu_core::{BlockUpdate, CoreError, Machine, MachineStatus, UartUpdate};
use minemu_platform::{
    InspectionRequest, InspectionResponse, MmuInspection, ObservableEvent, Peripheral,
    PeripheralsInspection,
};
use minemu_unicorn::{BackendError, BackendStop, UnicornBackend};
use thiserror::Error;

/// Lifecycle state published by the emulator service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleState {
    Starting,
    Running,
    Paused,
    Stopping,
    Stopped,
    Failed,
}

/// One independently addressed UART ingress path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UartPort {
    Uart0,
    Uart1,
}

impl UartPort {
    const fn index(self) -> usize {
        match self {
            Self::Uart0 => 0,
            Self::Uart1 => 1,
        }
    }
}

/// Lightweight immutable state continuously available to observers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeStatus {
    pub lifecycle: LifecycleState,
    pub machine: MachineStatus,
    pub last_stop: Option<String>,
    pub last_error: Option<String>,
}

/// Owned response to an on-demand machine inspection request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeInspection {
    Memory(Vec<u8>),
    Mmu(MmuInspection),
    Peripherals(PeripheralsInspection),
    Events(Vec<ObservableEvent>),
}

/// Runtime configuration supplied before the emulator thread starts.
pub struct RuntimeConfig {
    pub machine: Machine,
    pub entry: u32,
    pub end: u32,
    pub instruction_batch: usize,
    pub status_period: Duration,
    pub command_capacity: usize,
    pub uart_capacity: usize,
    pub block_media_path: Option<PathBuf>,
}

impl RuntimeConfig {
    pub fn new(machine: Machine, entry: u32) -> Self {
        Self {
            machine,
            entry,
            end: u32::MAX,
            instruction_batch: 1024,
            status_period: Duration::from_millis(16),
            command_capacity: 32,
            uart_capacity: 4096,
            block_media_path: None,
        }
    }
}

/// Runtime operation failures visible to host callers.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("runtime command queue is full")]
    CommandQueueFull,
    #[error("runtime service has stopped")]
    Stopped,
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, RuntimeError>;
type InspectionResult = std::result::Result<RuntimeInspection, RuntimeError>;

enum Command {
    Pause,
    Resume,
    Reset,
    Shutdown,
    Inspect(InspectionRequest, SyncSender<InspectionResult>),
}

/// Thread-safe host handle. It never exposes the concrete guest machine.
pub struct RuntimeHandle {
    commands: SyncSender<Command>,
    status: Arc<Mutex<RuntimeStatus>>,
    uart: Arc<Mutex<UartInbox>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl RuntimeHandle {
    /// Starts a concrete machine on its dedicated emulator thread.
    pub fn spawn(mut config: RuntimeConfig) -> Result<Self> {
        let machine_status = config.machine.status();
        let status = Arc::new(Mutex::new(RuntimeStatus {
            lifecycle: LifecycleState::Starting,
            machine: machine_status,
            last_stop: None,
            last_error: None,
        }));
        let command_capacity = config.command_capacity.max(1);
        let (command_sender, command_receiver) = mpsc::sync_channel(command_capacity);
        let uart = Arc::new(Mutex::new(UartInbox::new(config.uart_capacity)));
        let service_status = Arc::clone(&status);
        let service_uart = Arc::clone(&uart);
        config.instruction_batch = config.instruction_batch.max(1);
        let thread = thread::Builder::new()
            .name("minemu-emulator".into())
            .spawn(move || {
                Service::new(config, command_receiver, service_status, service_uart).run();
            })
            .map_err(RuntimeError::Io)?;
        Ok(Self {
            commands: command_sender,
            status,
            uart,
            thread: Mutex::new(Some(thread)),
        })
    }

    /// Returns the latest status without queueing work on the emulator thread.
    pub fn status(&self) -> RuntimeStatus {
        self.status
            .lock()
            .expect("runtime status mutex poisoned")
            .clone()
    }

    pub fn pause(&self) -> Result<()> {
        self.send(Command::Pause)
    }

    pub fn resume(&self) -> Result<()> {
        self.send(Command::Resume)
    }

    pub fn reset(&self) -> Result<()> {
        self.send(Command::Reset)
    }

    /// Queues shutdown and waits for the emulator thread to stop.
    pub fn shutdown(&self) -> Result<()> {
        let _ = self.send(Command::Shutdown);
        if let Some(thread) = self
            .thread
            .lock()
            .expect("runtime thread mutex poisoned")
            .take()
        {
            let _ = thread.join();
        }
        Ok(())
    }

    /// Appends bytes to the bounded ingress buffer for one UART.
    ///
    /// Input is coalesced into batches on the emulator thread. When full, the
    /// oldest queued bytes are discarded so producer speed cannot grow state.
    pub fn send_uart(&self, port: UartPort, bytes: &[u8]) {
        self.uart
            .lock()
            .expect("UART ingress mutex poisoned")
            .push(port, bytes);
    }

    /// Requests a larger inspection without letting a slow receiver block guest execution.
    pub fn request_inspection(
        &self,
        request: InspectionRequest,
    ) -> Result<Receiver<InspectionResult>> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(Command::Inspect(request, sender))?;
        Ok(receiver)
    }

    pub fn inspect(&self, request: InspectionRequest) -> Result<RuntimeInspection> {
        self.request_inspection(request)?
            .recv()
            .map_err(|_| RuntimeError::Stopped)?
    }

    fn send(&self, command: Command) -> Result<()> {
        match self.commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(RuntimeError::CommandQueueFull),
            Err(TrySendError::Disconnected(_)) => Err(RuntimeError::Stopped),
        }
    }
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        let _ = self.commands.try_send(Command::Shutdown);
    }
}

struct UartInbox {
    bytes: [VecDeque<u8>; 2],
    capacity: usize,
}

impl UartInbox {
    fn new(capacity: usize) -> Self {
        Self {
            bytes: std::array::from_fn(|_| VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    fn push(&mut self, port: UartPort, bytes: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        let queue = &mut self.bytes[port.index()];
        for byte in bytes {
            if queue.len() == self.capacity {
                queue.pop_front();
            }
            queue.push_back(*byte);
        }
    }

    fn drain(&mut self, port: UartPort) -> Vec<u8> {
        self.bytes[port.index()].drain(..).collect()
    }
}

struct Emulator {
    backend: UnicornBackend,
    entry: u32,
    end: u32,
    instruction_batch: usize,
    block_media_path: Option<PathBuf>,
}

impl Emulator {
    fn new(config: &RuntimeConfig) -> Result<Self> {
        let mut machine = config.machine.reset_clone()?;
        if let Some(path) = &config.block_media_path {
            machine
                .bus
                .block
                .update(BlockUpdate::Attach(fs::read(path)?))?;
        }
        let mut backend = UnicornBackend::new(machine)?;
        backend.set_program_counter(config.entry)?;
        Ok(Self {
            backend,
            entry: config.entry,
            end: config.end,
            instruction_batch: config.instruction_batch,
            block_media_path: config.block_media_path.clone(),
        })
    }

    fn run_batch(&mut self) -> Result<BackendStop> {
        let start = self.backend.program_counter().unwrap_or(self.entry);
        Ok(self.backend.run(start, self.end, self.instruction_batch))
    }

    fn reset(&mut self) -> Result<()> {
        self.flush()?;
        let machine = self.backend.machine().reset_clone()?;
        self.backend = UnicornBackend::new(machine)?;
        self.backend.set_program_counter(self.entry)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let Some(path) = &self.block_media_path else {
            return Ok(());
        };
        self.backend
            .machine_mut()
            .bus
            .block
            .flush(|media| fs::write(path, media))?;
        Ok(())
    }

    fn drain_uart(&mut self, inbox: &Arc<Mutex<UartInbox>>) {
        let uart0 = inbox
            .lock()
            .expect("UART ingress mutex poisoned")
            .drain(UartPort::Uart0);
        let uart1 = inbox
            .lock()
            .expect("UART ingress mutex poisoned")
            .drain(UartPort::Uart1);
        for byte in uart0 {
            let _ = self
                .backend
                .machine_mut()
                .bus
                .uart0
                .update(UartUpdate::Receive(byte));
        }
        for byte in uart1 {
            let _ = self
                .backend
                .machine_mut()
                .bus
                .uart1
                .update(UartUpdate::Receive(byte));
        }
    }

    fn status(&mut self) -> MachineStatus {
        self.backend.machine_mut().status()
    }

    fn inspect(&mut self, request: InspectionRequest) -> Result<RuntimeInspection> {
        let response = self.backend.machine_mut().inspect(request)?;
        Ok(match response {
            InspectionResponse::Memory(bytes) => RuntimeInspection::Memory(bytes.to_vec()),
            InspectionResponse::Mmu(mmu) => RuntimeInspection::Mmu(mmu),
            InspectionResponse::Peripherals(peripherals) => {
                RuntimeInspection::Peripherals(peripherals)
            }
            InspectionResponse::Events(events) => RuntimeInspection::Events(events),
        })
    }
}

struct Service {
    config: RuntimeConfig,
    commands: Receiver<Command>,
    status: Arc<Mutex<RuntimeStatus>>,
    uart: Arc<Mutex<UartInbox>>,
}

impl Service {
    fn new(
        config: RuntimeConfig,
        commands: Receiver<Command>,
        status: Arc<Mutex<RuntimeStatus>>,
        uart: Arc<Mutex<UartInbox>>,
    ) -> Self {
        Self {
            config,
            commands,
            status,
            uart,
        }
    }

    fn run(mut self) {
        let mut emulator = match Emulator::new(&self.config) {
            Ok(emulator) => emulator,
            Err(error) => {
                self.publish(LifecycleState::Failed, None, Some(error.to_string()), None);
                return;
            }
        };
        let mut lifecycle = LifecycleState::Running;
        let mut last_publish = Instant::now();
        self.publish(lifecycle, None, None, Some(&mut emulator));

        loop {
            if !self.process_commands(&mut emulator, &mut lifecycle) {
                break;
            }
            if lifecycle == LifecycleState::Running {
                emulator.drain_uart(&self.uart);
                match emulator.run_batch() {
                    Ok(BackendStop::Unicorn(error)) => {
                        let flush_error = emulator.flush().err();
                        let detail = flush_error
                            .map(|flush| format!("backend {error:?}; flush {flush}"))
                            .unwrap_or_else(|| format!("backend {error:?}"));
                        self.publish(
                            LifecycleState::Failed,
                            Some(format!("{error:?}")),
                            Some(detail),
                            Some(&mut emulator),
                        );
                        break;
                    }
                    Ok(stop) => {
                        if last_publish.elapsed() >= self.config.status_period {
                            self.publish(
                                lifecycle,
                                Some(format!("{stop:?}")),
                                None,
                                Some(&mut emulator),
                            );
                            last_publish = Instant::now();
                        }
                    }
                    Err(error) => {
                        let flush_error = emulator.flush().err();
                        let detail = flush_error
                            .map(|flush| format!("{error}; flush {flush}"))
                            .unwrap_or_else(|| error.to_string());
                        self.publish(
                            LifecycleState::Failed,
                            None,
                            Some(detail),
                            Some(&mut emulator),
                        );
                        break;
                    }
                }
            } else if lifecycle == LifecycleState::Paused {
                match self.commands.recv_timeout(Duration::from_millis(10)) {
                    Ok(command) => {
                        if !self.process_command(command, &mut emulator, &mut lifecycle) {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        self.stop(&mut emulator, None);
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }
    }

    fn process_commands(
        &mut self,
        emulator: &mut Emulator,
        lifecycle: &mut LifecycleState,
    ) -> bool {
        loop {
            match self.commands.try_recv() {
                Ok(command) => {
                    if !self.process_command(command, emulator, lifecycle) {
                        return false;
                    }
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => {
                    self.stop(emulator, None);
                    return false;
                }
            }
        }
    }

    fn process_command(
        &mut self,
        command: Command,
        emulator: &mut Emulator,
        lifecycle: &mut LifecycleState,
    ) -> bool {
        match command {
            Command::Pause if *lifecycle == LifecycleState::Running => {
                *lifecycle = LifecycleState::Paused;
                let error = emulator.flush().err().map(|error| error.to_string());
                self.publish(*lifecycle, None, error, Some(emulator));
            }
            Command::Resume if *lifecycle == LifecycleState::Paused => {
                *lifecycle = LifecycleState::Running;
                self.publish(*lifecycle, None, None, Some(emulator));
            }
            Command::Reset
                if matches!(*lifecycle, LifecycleState::Running | LifecycleState::Paused) =>
            {
                let previous = *lifecycle;
                match emulator.reset() {
                    Ok(()) => self.publish(previous, None, None, Some(emulator)),
                    Err(error) => {
                        self.publish(previous, None, Some(error.to_string()), Some(emulator))
                    }
                }
            }
            Command::Shutdown => {
                self.stop(emulator, None);
                return false;
            }
            Command::Inspect(request, response) => {
                let _ = response.try_send(emulator.inspect(request));
            }
            _ => {}
        }
        true
    }

    fn stop(&mut self, emulator: &mut Emulator, error: Option<String>) {
        self.publish(LifecycleState::Stopping, None, error, Some(emulator));
        match emulator.flush() {
            Ok(()) => self.publish(LifecycleState::Stopped, None, None, Some(emulator)),
            Err(flush_error) => self.publish(
                LifecycleState::Failed,
                None,
                Some(flush_error.to_string()),
                Some(emulator),
            ),
        }
    }

    fn publish(
        &self,
        lifecycle: LifecycleState,
        last_stop: Option<String>,
        last_error: Option<String>,
        emulator: Option<&mut Emulator>,
    ) {
        let mut status = self.status.lock().expect("runtime status mutex poisoned");
        status.lifecycle = lifecycle;
        if let Some(stop) = last_stop {
            status.last_stop = Some(stop);
        }
        if let Some(error) = last_error {
            status.last_error = Some(error);
        }
        if let Some(emulator) = emulator {
            status.machine = emulator.status();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use minemu_core::PhysicalMemoryAccess;
    use minemu_platform::{InspectionRequest, MemRegion, PhysicalAddress, PhysicalRange};

    use super::{LifecycleState, Machine, RuntimeConfig, RuntimeHandle, UartPort};

    fn running_runtime() -> RuntimeHandle {
        let mut machine = Machine::default();
        let entry = MemRegion::Ram.base().get();
        machine
            .memory
            .write_range(PhysicalAddress::new(entry), &[0xfe, 0xff, 0xff, 0xea])
            .unwrap(); // b .
        RuntimeHandle::spawn(RuntimeConfig::new(machine, entry)).unwrap()
    }

    fn wait_for(runtime: &RuntimeHandle, state: LifecycleState) {
        for _ in 0..100 {
            if runtime.status().lifecycle == state {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("runtime did not reach {state:?}");
    }

    #[test]
    fn lifecycle_commands_run_on_the_emulator_thread() {
        let runtime = running_runtime();
        wait_for(&runtime, LifecycleState::Running);
        runtime.pause().unwrap();
        wait_for(&runtime, LifecycleState::Paused);
        runtime.reset().unwrap();
        runtime.resume().unwrap();
        wait_for(&runtime, LifecycleState::Running);
        runtime.shutdown().unwrap();
        assert_eq!(runtime.status().lifecycle, LifecycleState::Stopped);
    }

    #[test]
    fn dropped_inspection_receivers_do_not_stop_execution() {
        let runtime = running_runtime();
        wait_for(&runtime, LifecycleState::Running);
        let receiver = runtime
            .request_inspection(InspectionRequest::Memory(
                PhysicalRange::new(MemRegion::Ram.base(), 4096).unwrap(),
            ))
            .unwrap();
        drop(receiver);
        runtime.send_uart(UartPort::Uart0, b"coalesced input");
        for _ in 0..100 {
            if runtime.status().machine.ticks > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(runtime.status().machine.ticks > 0);
        runtime.shutdown().unwrap();
    }
}
