use std::{
    collections::VecDeque,
    fs,
    num::NonZeroU64,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use minemu_core::{BlockUpdate, MachineStatus, PhysicalMemoryAccess, UartUpdate};
use minemu_platform::{
    InspectionRequest, InspectionResponse, Peripheral,
    peripherals::block::{UNIT_FILESYSTEM, UNIT_SWAP},
};
use minemu_unicorn::{BackendStop, UnicornBackend};
use tracing::{debug, error, info, trace, warn};

use crate::{
    LifecycleState, RuntimeConfig, RuntimeError, RuntimeInspection, RuntimeInspectionRequest,
    RuntimeStatus, ScheduledUartInput, UartPort,
    types::{InspectionResult, Result},
};

enum Command {
    Pause,
    Resume(Option<NonZeroU64>),
    Reset,
    Shutdown,
    Inspect(RuntimeInspectionRequest, SyncSender<InspectionResult>),
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
        canonicalize_block_media_paths(&mut config)?;
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
        info!(
            entry = format_args!("{:#010x}", config.entry),
            instruction_batch = config.instruction_batch,
            "spawning emulator runtime"
        );
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
        debug!("enqueueing runtime pause command");
        self.send(Command::Pause)
    }
    pub fn resume(&self) -> Result<()> {
        debug!("enqueueing runtime resume command");
        self.send(Command::Resume(None))
    }
    pub fn resume_for(&self, instructions: NonZeroU64) -> Result<()> {
        debug!(
            instructions = instructions.get(),
            "enqueueing bounded runtime resume command"
        );
        self.send(Command::Resume(Some(instructions)))
    }
    pub fn reset(&self) -> Result<()> {
        debug!("enqueueing runtime reset command");
        self.send(Command::Reset)
    }

    /// Queues shutdown and waits for the emulator thread to stop.
    pub fn shutdown(&self) -> Result<()> {
        info!("enqueueing runtime shutdown command");
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
        request: RuntimeInspectionRequest,
    ) -> Result<Receiver<InspectionResult>> {
        let status = self.status();
        debug!(
            request = ?request,
            lifecycle = ?status.lifecycle,
            tick = status.machine.ticks,
            "enqueueing runtime inspection request"
        );
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(Command::Inspect(request, sender))?;
        Ok(receiver)
    }

    pub fn inspect(&self, request: InspectionRequest) -> Result<RuntimeInspection> {
        self.request_inspection(RuntimeInspectionRequest::Machine(request))?
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
    block_media_paths: [Option<std::path::PathBuf>; 2],
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
            block_media_paths: [
                config.block_media_path.clone(),
                config.block1_media_path.clone(),
            ],
            initial_ram_writes: config.initial_ram_writes.clone(),
        })
    }

    fn run_batch(
        &mut self,
        instruction_budget: usize,
        tick_deadline: Option<u64>,
    ) -> Result<BackendStop> {
        let start = self.backend.program_counter().unwrap_or(self.entry);
        Ok(match tick_deadline {
            Some(deadline) => {
                self.backend
                    .run_until_tick(start, self.end, instruction_budget, deadline)
            }
            None => self.backend.run(start, self.end, instruction_budget),
        })
    }

    fn resume_from_breakpoint(&mut self, address: u32) -> Result<()> {
        if self.backend.program_counter()? == address {
            self.backend.set_program_counter(address.wrapping_add(4))?;
        }
        Ok(())
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
            block_media_path: self.block_media_paths[0].clone(),
            block1_media_path: self.block_media_paths[1].clone(),
            initial_ram_writes: self.initial_ram_writes.clone(),
            start_paused: false,
            execution_deadline: None,
            scheduled_uart: Vec::new(),
        };
        let machine = configured_machine(&config, &config.machine)?;
        self.backend = UnicornBackend::new(machine)?;
        self.backend.set_program_counter(self.entry)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        let mut first_error = None;
        for (unit, path) in [UNIT_FILESYSTEM, UNIT_SWAP]
            .into_iter()
            .zip(self.block_media_paths.iter())
        {
            let Some(path) = path else { continue };
            if let Err(error) = self
                .backend
                .machine_mut()
                .bus
                .block
                .flush(unit, |media| fs::write(path, media))
            {
                if first_error.is_none() {
                    first_error = Some(error);
                } else {
                    error!(unit, path = %path.display(), error = %error, "additional block flush failed");
                }
            }
        }
        first_error.map_or(Ok(()), Err).map_err(Into::into)
    }

    fn drain_uart(&mut self, inbox: &Arc<Mutex<UartInbox>>) {
        let mut inbox = inbox.lock().expect("UART ingress mutex poisoned");
        let uart0 = inbox.drain(UartPort::Uart0);
        let uart1 = inbox.drain(UartPort::Uart1);
        drop(inbox);
        self.deliver_uart(UartPort::Uart0, &uart0);
        self.deliver_uart(UartPort::Uart1, &uart1);
    }

    fn deliver_uart(&mut self, port: UartPort, bytes: &[u8]) {
        let uart = match port {
            UartPort::Uart0 => &mut self.backend.machine_mut().bus.uart0,
            UartPort::Uart1 => &mut self.backend.machine_mut().bus.uart1,
        };
        for byte in bytes {
            let _ = uart.update(UartUpdate::Receive(*byte));
        }
    }

    fn status(&mut self) -> MachineStatus {
        self.backend.machine_mut().status()
    }

    fn inspect(&mut self, request: &RuntimeInspectionRequest) -> Result<RuntimeInspection> {
        match request {
            RuntimeInspectionRequest::Machine(request) => {
                Ok(match self.backend.machine_mut().inspect(*request)? {
                    InspectionResponse::Memory(bytes) => RuntimeInspection::Memory(bytes.to_vec()),
                    InspectionResponse::Mmu(mmu) => RuntimeInspection::Mmu(mmu),
                    InspectionResponse::Peripherals(peripherals) => {
                        RuntimeInspection::Peripherals(peripherals)
                    }
                    InspectionResponse::Events(events) => RuntimeInspection::Events(events),
                })
            }
            RuntimeInspectionRequest::LiveMemory(range) => Ok(RuntimeInspection::LiveMemory(
                *range,
                self.backend.inspect_live_memory(*range)?,
            )),
            RuntimeInspectionRequest::VirtualMemory { address, length } => {
                Ok(RuntimeInspection::VirtualMemory(
                    *address,
                    self.backend.inspect_virtual_memory(*address, *length)?,
                ))
            }
            RuntimeInspectionRequest::Translate(address) => Ok(RuntimeInspection::Translation(
                *address,
                self.backend.translate_virtual_address(*address)?,
            )),
            RuntimeInspectionRequest::Execution {
                address,
                before,
                after,
            } => Ok(RuntimeInspection::Execution(
                self.backend.inspect_execution(*address, *before, *after)?,
            )),
            RuntimeInspectionRequest::SearchMemory { pattern } => Ok(
                RuntimeInspection::SearchMemory(self.backend.search_memory(pattern)?),
            ),
        }
    }
}

fn configured_machine(
    config: &RuntimeConfig,
    source: &minemu_core::Machine,
) -> Result<minemu_core::Machine> {
    let mut machine = source.reset_clone()?;
    for (unit, path) in [UNIT_FILESYSTEM, UNIT_SWAP]
        .into_iter()
        .zip([&config.block_media_path, &config.block1_media_path])
    {
        if let Some(path) = path {
            machine.bus.block.update(BlockUpdate::Attach {
                unit,
                media: fs::read(path)?,
            })?;
        }
    }
    for (address, bytes) in &config.initial_ram_writes {
        machine.memory.write_range(*address, bytes)?;
    }
    Ok(machine)
}

fn canonicalize_block_media_paths(config: &mut RuntimeConfig) -> Result<()> {
    for (unit, path) in [UNIT_FILESYSTEM, UNIT_SWAP]
        .into_iter()
        .zip([&mut config.block_media_path, &mut config.block1_media_path])
    {
        if let Some(original) = path {
            let canonical =
                fs::canonicalize(&*original).map_err(|source| RuntimeError::BlockMediaPath {
                    unit,
                    path: original.clone(),
                    source,
                })?;
            *original = canonical;
        }
    }
    if let (Some(unit0), Some(unit1)) = (&config.block_media_path, &config.block1_media_path)
        && unit0 == unit1
    {
        return Err(RuntimeError::DuplicateBlockMediaPath(unit0.clone()));
    }
    Ok(())
}

struct Service {
    config: RuntimeConfig,
    commands: Receiver<Command>,
    status: Arc<Mutex<RuntimeStatus>>,
    uart: Arc<Mutex<UartInbox>>,
    remaining_instructions: Option<u64>,
    pending_breakpoint: Option<u32>,
    execution_deadline: Option<u64>,
    scheduled_uart: VecDeque<ScheduledUartInput>,
}

impl Service {
    fn new(
        mut config: RuntimeConfig,
        commands: Receiver<Command>,
        status: Arc<Mutex<RuntimeStatus>>,
        uart: Arc<Mutex<UartInbox>>,
    ) -> Self {
        config.scheduled_uart.sort_by_key(|input| input.at_tick);
        let execution_deadline = config.execution_deadline;
        let scheduled_uart = std::mem::take(&mut config.scheduled_uart).into();
        Self {
            config,
            commands,
            status,
            uart,
            remaining_instructions: None,
            pending_breakpoint: None,
            execution_deadline,
            scheduled_uart,
        }
    }

    fn run(mut self) {
        let mut emulator = match Emulator::new(&self.config) {
            Ok(emulator) => emulator,
            Err(error) => {
                error!(error = %error, "failed to initialize emulator runtime");
                self.publish(LifecycleState::Failed, None, Some(error.to_string()), None);
                return;
            }
        };
        let mut lifecycle = if self.config.start_paused {
            LifecycleState::Paused
        } else {
            LifecycleState::Running
        };
        let mut last_publish = Instant::now();
        info!(
            lifecycle = ?lifecycle,
            tick = emulator.backend.machine().ticks(),
            "emulator runtime started"
        );
        self.publish(lifecycle, None, None, Some(&mut emulator));
        loop {
            if !self.process_commands(&mut emulator, &mut lifecycle) {
                break;
            }
            if lifecycle == LifecycleState::Running {
                let current_tick = emulator.backend.machine().ticks();
                while self
                    .scheduled_uart
                    .front()
                    .is_some_and(|input| input.at_tick <= current_tick)
                {
                    let input = self
                        .scheduled_uart
                        .pop_front()
                        .expect("scheduled UART queue was just checked");
                    emulator.deliver_uart(input.port, &input.bytes);
                }
                if self
                    .execution_deadline
                    .is_some_and(|deadline| current_tick >= deadline)
                {
                    lifecycle = LifecycleState::Paused;
                    self.publish(
                        lifecycle,
                        Some("execution deadline reached".into()),
                        emulator.flush().err().map(|error| error.to_string()),
                        Some(&mut emulator),
                    );
                    last_publish = Instant::now();
                    continue;
                }
                emulator.drain_uart(&self.uart);
                let mut instruction_budget =
                    self.remaining_instructions
                        .map_or(emulator.instruction_batch, |remaining| {
                            usize::try_from(remaining)
                                .unwrap_or(usize::MAX)
                                .min(emulator.instruction_batch)
                        });
                let tick_boundary = self
                    .execution_deadline
                    .into_iter()
                    .chain(self.scheduled_uart.front().map(|input| input.at_tick))
                    .filter(|boundary| *boundary > current_tick)
                    .min();
                if let Some(boundary) = tick_boundary {
                    instruction_budget = instruction_budget
                        .min(usize::try_from(boundary - current_tick).unwrap_or(usize::MAX));
                }
                let ticks_before = emulator.backend.machine().ticks();
                let result = emulator.run_batch(instruction_budget, tick_boundary);
                let executed = emulator
                    .backend
                    .machine()
                    .ticks()
                    .saturating_sub(ticks_before);
                let instruction_limit_reached =
                    self.remaining_instructions
                        .as_mut()
                        .is_some_and(|remaining| {
                            *remaining = remaining.saturating_sub(executed);
                            *remaining == 0
                        });
                match result {
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
                    Ok(BackendStop::Breakpoint { address, immediate }) => {
                        lifecycle = LifecycleState::Paused;
                        self.remaining_instructions = None;
                        self.pending_breakpoint = Some(address);
                        let stop = format!("breakpoint #0x{immediate:04x} at 0x{address:08x}");
                        info!(
                            address,
                            immediate,
                            tick = emulator.backend.machine().ticks(),
                            "guest breakpoint paused runtime"
                        );
                        self.publish(
                            lifecycle,
                            Some(stop),
                            emulator.flush().err().map(|error| error.to_string()),
                            Some(&mut emulator),
                        );
                        last_publish = Instant::now();
                    }
                    Ok(stop) => {
                        trace!(
                            ?stop,
                            pc = emulator.backend.program_counter().ok(),
                            tick = emulator.backend.machine().ticks(),
                            "emulator batch stopped"
                        );
                        if instruction_limit_reached {
                            lifecycle = LifecycleState::Paused;
                            self.remaining_instructions = None;
                            info!(
                                tick = emulator.backend.machine().ticks(),
                                "bounded runtime execution completed"
                            );
                            self.publish(
                                lifecycle,
                                Some(format!("instruction limit reached ({stop:?})")),
                                emulator.flush().err().map(|error| error.to_string()),
                                Some(&mut emulator),
                            );
                            last_publish = Instant::now();
                        } else if last_publish.elapsed() >= self.config.status_period {
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
                info!(
                    from = ?*lifecycle,
                    to = ?LifecycleState::Paused,
                    tick = emulator.backend.machine().ticks(),
                    "runtime lifecycle transition"
                );
                *lifecycle = LifecycleState::Paused;
                self.remaining_instructions = None;
                self.publish(
                    *lifecycle,
                    None,
                    emulator.flush().err().map(|error| error.to_string()),
                    Some(emulator),
                );
            }
            Command::Resume(instruction_limit) if *lifecycle == LifecycleState::Paused => {
                if let Some(address) = self.pending_breakpoint.take()
                    && let Err(error) = emulator.resume_from_breakpoint(address)
                {
                    self.publish(
                        LifecycleState::Failed,
                        None,
                        Some(error.to_string()),
                        Some(emulator),
                    );
                    return false;
                }
                info!(
                    from = ?*lifecycle,
                    to = ?LifecycleState::Running,
                    tick = emulator.backend.machine().ticks(),
                    "runtime lifecycle transition"
                );
                self.remaining_instructions = instruction_limit.map(NonZeroU64::get);
                *lifecycle = LifecycleState::Running;
                self.publish_clearing_stop(*lifecycle, emulator);
            }
            Command::Reset
                if matches!(*lifecycle, LifecycleState::Running | LifecycleState::Paused) =>
            {
                let previous = *lifecycle;
                info!(
                    lifecycle = ?previous,
                    tick = emulator.backend.machine().ticks(),
                    "resetting emulator runtime"
                );
                match emulator.reset() {
                    Ok(()) => {
                        self.pending_breakpoint = None;
                        self.publish_clearing_stop(previous, emulator);
                    }
                    Err(error) => {
                        error!(error = %error, "emulator reset failed");
                        self.publish(previous, None, Some(error.to_string()), Some(emulator))
                    }
                }
            }
            Command::Shutdown => {
                info!(
                    lifecycle = ?*lifecycle,
                    tick = emulator.backend.machine().ticks(),
                    "shutting down emulator runtime"
                );
                self.stop(emulator, None);
                return false;
            }
            Command::Inspect(request, response) => {
                let tick = emulator.backend.machine().ticks();
                debug!(
                    request = ?request,
                    lifecycle = ?*lifecycle,
                    tick,
                    "processing runtime inspection request"
                );
                let result = emulator.inspect(&request);
                match &result {
                    Ok(_) => {
                        debug!(request = ?request, lifecycle = ?*lifecycle, tick, "completed runtime inspection request")
                    }
                    Err(error) => {
                        warn!(request = ?request, lifecycle = ?*lifecycle, tick, error = %error, "runtime inspection request failed")
                    }
                }
                let _ = response.try_send(result);
            }
            _ => {}
        }
        true
    }

    fn stop(&mut self, emulator: &mut Emulator, error: Option<String>) {
        info!(
            to = ?LifecycleState::Stopping,
            tick = emulator.backend.machine().ticks(),
            "runtime lifecycle transition"
        );
        self.publish(LifecycleState::Stopping, None, error, Some(emulator));
        match emulator.flush() {
            Ok(()) => {
                info!(
                    to = ?LifecycleState::Stopped,
                    tick = emulator.backend.machine().ticks(),
                    "runtime lifecycle transition"
                );
                self.publish(LifecycleState::Stopped, None, None, Some(emulator));
            }
            Err(error) => {
                error!(error = %error, "runtime shutdown flush failed");
                self.publish(
                    LifecycleState::Failed,
                    None,
                    Some(error.to_string()),
                    Some(emulator),
                );
            }
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

    fn publish_clearing_stop(&self, lifecycle: LifecycleState, emulator: &mut Emulator) {
        let mut status = self.status.lock().expect("runtime status mutex poisoned");
        status.lifecycle = lifecycle;
        status.last_stop = None;
        status.machine = emulator.status();
    }
}

#[cfg(test)]
mod emulator_tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use minemu_core::{BlockUpdate, Machine, PhysicalMemoryAccess};
    use minemu_platform::{MemRegion, Peripheral, peripherals::block::Register};

    use super::{Emulator, RuntimeConfig};

    static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "minemu-{label}-{}-{}.img",
            std::process::id(),
            NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn dirty_unit(emulator: &mut Emulator, unit: u32, value: u8) {
        let dma = MemRegion::Ram.base();
        let machine = emulator.backend.machine_mut();
        machine.memory.write_u8(dma, value).unwrap();
        for (register, value) in [
            (Register::Lba, 0),
            (Register::SectorCount, 1),
            (Register::PhysicalAddress, dma.get()),
            (Register::Unit, unit),
            (Register::Command, 2),
        ] {
            machine
                .bus
                .block
                .update(BlockUpdate::Write {
                    register,
                    value,
                    now: machine.ticks(),
                })
                .unwrap();
        }
        for _ in 0..32 {
            machine.finish_instruction(minemu_core::InstructionOutcome::Completed);
        }
    }

    #[test]
    fn reset_flushes_dirty_block_media_before_rebuilding() {
        let path0 = temp_path("reset-flush-0");
        let path1 = temp_path("reset-flush-1");
        fs::write(&path0, vec![0; 512]).unwrap();
        fs::write(&path1, vec![0; 512]).unwrap();

        let mut config = RuntimeConfig::new(Machine::default(), 0);
        config.block_media_path = Some(path0.clone());
        config.block1_media_path = Some(path1.clone());
        let mut emulator = Emulator::new(&config).unwrap();
        dirty_unit(&mut emulator, 0, 0xa5);
        dirty_unit(&mut emulator, 1, 0x5a);

        emulator.reset().unwrap();

        assert_eq!(fs::read(&path0).unwrap()[0], 0xa5);
        assert_eq!(fs::read(&path1).unwrap()[0], 0x5a);
        let block = &emulator.backend.machine().bus.block;
        assert_eq!(block.dirty_sector_count(0).unwrap(), 0);
        assert_eq!(block.dirty_sector_count(1).unwrap(), 0);
        fs::remove_file(path0).unwrap();
        fs::remove_file(path1).unwrap();
    }

    #[test]
    fn flush_attempts_unit_one_after_unit_zero_fails() {
        let path0 = temp_path("failed-flush-0");
        let path1 = temp_path("successful-flush-1");
        fs::write(&path0, vec![0; 512]).unwrap();
        fs::write(&path1, vec![0; 512]).unwrap();
        let mut config = RuntimeConfig::new(Machine::default(), 0);
        config.block_media_path = Some(path0.clone());
        config.block1_media_path = Some(path1.clone());
        let mut emulator = Emulator::new(&config).unwrap();
        dirty_unit(&mut emulator, 0, 0xa5);
        dirty_unit(&mut emulator, 1, 0x5a);

        fs::remove_file(&path0).unwrap();
        fs::create_dir(&path0).unwrap();
        assert!(emulator.flush().is_err());
        assert_eq!(fs::read(&path1).unwrap()[0], 0x5a);
        let block = &emulator.backend.machine().bus.block;
        assert_eq!(block.dirty_sector_count(0).unwrap(), 1);
        assert_eq!(block.dirty_sector_count(1).unwrap(), 0);

        fs::remove_dir(path0).unwrap();
        fs::remove_file(path1).unwrap();
    }

    #[test]
    fn duplicate_block_media_paths_are_rejected() {
        let path = temp_path("duplicate");
        fs::write(&path, vec![0; 512]).unwrap();
        let mut config = RuntimeConfig::new(Machine::default(), 0);
        config.block_media_path = Some(path.clone());
        config.block1_media_path = Some(path.clone());
        assert!(matches!(
            super::RuntimeHandle::spawn(config),
            Err(super::RuntimeError::DuplicateBlockMediaPath(_))
        ));
        fs::remove_file(path).unwrap();
    }
}
