use std::{
    collections::VecDeque,
    fs,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use minemu_core::{BlockUpdate, MachineStatus, PhysicalMemoryAccess, UartUpdate};
use minemu_platform::{
    InspectionRequest, InspectionResponse, Peripheral, PhysicalRange, VirtualAddress,
};
use minemu_unicorn::{BackendStop, UnicornBackend};

use crate::{
    LifecycleState, RuntimeConfig, RuntimeError, RuntimeInspection, RuntimeStatus, UartPort,
    types::{ExecutionInspection, InspectionResult, Result},
};

enum Command {
    Pause,
    Resume,
    Reset,
    Shutdown,
    Inspect(InspectionRequest, SyncSender<InspectionResult>),
    LiveMemory(PhysicalRange, SyncSender<InspectionResult>),
    Execution {
        before: usize,
        after: usize,
        response: SyncSender<InspectionResult>,
    },
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
        let (command_sender, command_receiver) = mpsc::sync_channel(config.command_capacity.max(1));
        let uart = Arc::new(Mutex::new(UartInbox::new(config.uart_capacity)));
        let service_status = Arc::clone(&status);
        let service_uart = Arc::clone(&uart);
        config.instruction_batch = config.instruction_batch.max(1);
        let thread = thread::Builder::new()
            .name("minemu-emulator".into())
            .spawn(move || {
                Service::new(config, command_receiver, service_status, service_uart).run()
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

    /// Reads current physical bytes from Unicorn rather than the core RAM mirror.
    pub fn inspect_live_memory(&self, range: PhysicalRange) -> Result<RuntimeInspection> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(Command::LiveMemory(range, sender))?;
        receiver.recv().map_err(|_| RuntimeError::Stopped)?
    }

    /// Captures CPU state and virtual instruction bytes at one emulator-thread boundary.
    pub fn inspect_execution(&self, before: usize, after: usize) -> Result<RuntimeInspection> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(Command::Execution {
            before,
            after,
            response: sender,
        })?;
        receiver.recv().map_err(|_| RuntimeError::Stopped)?
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
    block_media_path: Option<std::path::PathBuf>,
    initial_ram_writes: Vec<(minemu_platform::PhysicalAddress, Vec<u8>)>,
}

impl Emulator {
    fn new(config: &RuntimeConfig) -> Result<Self> {
        let machine = configured_machine(config, &config.machine)?;
        let mut backend = UnicornBackend::new(machine)?;
        backend.set_program_counter(config.entry)?;
        Ok(Self {
            backend,
            entry: config.entry,
            end: config.end,
            instruction_batch: config.instruction_batch,
            block_media_path: config.block_media_path.clone(),
            initial_ram_writes: config.initial_ram_writes.clone(),
        })
    }

    fn run_batch(&mut self) -> Result<BackendStop> {
        let start = self.backend.program_counter().unwrap_or(self.entry);
        Ok(self.backend.run(start, self.end, self.instruction_batch))
    }

    fn reset(&mut self) -> Result<()> {
        self.flush()?;
        let config = RuntimeConfig {
            machine: self.backend.machine().reset_clone()?,
            entry: self.entry,
            end: self.end,
            instruction_batch: self.instruction_batch,
            status_period: Duration::ZERO,
            command_capacity: 1,
            uart_capacity: 0,
            block_media_path: self.block_media_path.clone(),
            initial_ram_writes: self.initial_ram_writes.clone(),
        };
        let machine = configured_machine(&config, &config.machine)?;
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
        let mut inbox = inbox.lock().expect("UART ingress mutex poisoned");
        let uart0 = inbox.drain(UartPort::Uart0);
        let uart1 = inbox.drain(UartPort::Uart1);
        drop(inbox);
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
        Ok(match self.backend.machine_mut().inspect(request)? {
            InspectionResponse::Memory(bytes) => RuntimeInspection::Memory(bytes.to_vec()),
            InspectionResponse::Mmu(mmu) => RuntimeInspection::Mmu(mmu),
            InspectionResponse::Peripherals(peripherals) => {
                RuntimeInspection::Peripherals(peripherals)
            }
            InspectionResponse::Events(events) => RuntimeInspection::Events(events),
        })
    }

    fn inspect_live_memory(&self, range: PhysicalRange) -> Result<RuntimeInspection> {
        Ok(RuntimeInspection::LiveMemory(
            range,
            self.backend
                .read_physical_memory(range.start(), range.length() as usize)?,
        ))
    }

    fn inspect_execution(&mut self, before: usize, after: usize) -> Result<RuntimeInspection> {
        let cpu = self.backend.cpu_state()?;
        let start = cpu.registers[15].saturating_sub(before as u32);
        let length = before.checked_add(after).ok_or(RuntimeError::Stopped)?;
        Ok(RuntimeInspection::Execution(ExecutionInspection {
            registers: cpu.registers,
            cpsr: cpu.cpsr,
            spsr: cpu.spsr,
            instruction_address: VirtualAddress::new(start),
            instruction_bytes: self
                .backend
                .read_virtual_memory(VirtualAddress::new(start), length)?,
        }))
    }
}

fn configured_machine(
    config: &RuntimeConfig,
    source: &minemu_core::Machine,
) -> Result<minemu_core::Machine> {
    let mut machine = source.reset_clone()?;
    if let Some(path) = &config.block_media_path {
        machine
            .bus
            .block
            .update(BlockUpdate::Attach(fs::read(path)?))?;
    }
    for (address, bytes) in &config.initial_ram_writes {
        machine.memory.write_range(*address, bytes)?;
    }
    Ok(machine)
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
                    Ok(stop) if last_publish.elapsed() >= self.config.status_period => {
                        self.publish(
                            lifecycle,
                            Some(format!("{stop:?}")),
                            None,
                            Some(&mut emulator),
                        );
                        last_publish = Instant::now();
                    }
                    Ok(_) => {}
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
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        self.stop(&mut emulator, None);
                        break;
                    }
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
                self.publish(
                    *lifecycle,
                    None,
                    emulator.flush().err().map(|error| error.to_string()),
                    Some(emulator),
                );
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
            Command::LiveMemory(range, response) => {
                let _ = response.try_send(emulator.inspect_live_memory(range));
            }
            Command::Execution {
                before,
                after,
                response,
            } => {
                let _ = response.try_send(emulator.inspect_execution(before, after));
            }
            _ => {}
        }
        true
    }

    fn stop(&mut self, emulator: &mut Emulator, error: Option<String>) {
        self.publish(LifecycleState::Stopping, None, error, Some(emulator));
        match emulator.flush() {
            Ok(()) => self.publish(LifecycleState::Stopped, None, None, Some(emulator)),
            Err(error) => self.publish(
                LifecycleState::Failed,
                None,
                Some(error.to_string()),
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
