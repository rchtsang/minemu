use std::{
    collections::VecDeque,
    fs,
    num::NonZeroU64,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use minemu_core::{BlockUpdate, Machine, MachineStatus, PhysicalMemoryAccess, UartUpdate};
use minemu_platform::{
    InspectionRequest, InspectionResponse, Peripheral, PhysicalAddress,
    peripherals::block::{UNIT_COUNT, UNIT_FILESYSTEM, UNIT_SWAP},
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
            .spawn(move || Service::start(config, command_receiver, service_status, service_uart))
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

    /// Appends bytes for one port, discarding the oldest when full.
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

/// Attached block media files, indexed by unit.
type BlockMediaPaths = [Option<PathBuf>; UNIT_COUNT];

/// Yields `(unit, path)` for each unit that has a media file.
fn block_media(paths: &BlockMediaPaths) -> impl Iterator<Item = (u32, &PathBuf)> {
    [UNIT_FILESYSTEM, UNIT_SWAP]
        .into_iter()
        .zip(paths)
        .filter_map(|(unit, path)| Some((unit, path.as_ref()?)))
}

/// The backend plus everything needed to rebuild it on reset.
struct Emulator {
    backend: UnicornBackend,
    entry: u32,
    end: u32,
    instruction_batch: usize,
    block_media_paths: BlockMediaPaths,
    initial_ram_writes: Vec<(PhysicalAddress, Vec<u8>)>,
}

impl Emulator {
    fn new(config: &RuntimeConfig) -> Result<Self> {
        let block_media_paths = [
            config.block_media_path.clone(),
            config.block1_media_path.clone(),
        ];
        let backend = build_backend(
            &config.machine,
            config.entry,
            &block_media_paths,
            &config.initial_ram_writes,
        )?;
        Ok(Self {
            backend,
            entry: config.entry,
            end: config.end,
            instruction_batch: config.instruction_batch,
            block_media_paths,
            initial_ram_writes: config.initial_ram_writes.clone(),
        })
    }

    fn ticks(&self) -> u64 {
        self.backend.machine().ticks()
    }

    fn run_batch(&mut self, instruction_budget: usize, tick_deadline: Option<u64>) -> BackendStop {
        let start = self.backend.program_counter().unwrap_or(self.entry);
        match tick_deadline {
            Some(deadline) => {
                self.backend
                    .run_until_tick(start, self.end, instruction_budget, deadline)
            }
            None => self.backend.run(start, self.end, instruction_budget),
        }
    }

    /// Steps over a breakpoint the guest is still stopped on.
    fn resume_from_breakpoint(&mut self, address: u32) -> Result<()> {
        if self.backend.program_counter()? == address {
            self.backend.set_program_counter(address.wrapping_add(4))?;
        }
        Ok(())
    }

    /// Persists media, then rebuilds the machine from reset state and the media files.
    fn reset(&mut self) -> Result<()> {
        self.flush()?;
        self.backend = build_backend(
            self.backend.machine(),
            self.entry,
            &self.block_media_paths,
            &self.initial_ram_writes,
        )?;
        Ok(())
    }

    /// Writes dirty media for every unit, attempting all units before reporting
    /// the first failure.
    fn flush(&mut self) -> Result<()> {
        let mut first_error = None;
        for (unit, path) in block_media(&self.block_media_paths) {
            let result = self
                .backend
                .machine_mut()
                .bus
                .block
                .flush(unit, |media| fs::write(path, media));
            if let Err(error) = result {
                if first_error.is_none() {
                    first_error = Some(error);
                } else {
                    error!(unit, path = %path.display(), error = %error, "additional block flush failed");
                }
            }
        }
        first_error.map_or(Ok(()), |error| Err(error.into()))
    }

    fn drain_uart(&mut self, inbox: &Mutex<UartInbox>) {
        let pending = {
            let mut inbox = inbox.lock().expect("UART ingress mutex poisoned");
            UartPort::ALL.map(|port| inbox.drain(port))
        };
        for (port, bytes) in UartPort::ALL.into_iter().zip(pending) {
            self.deliver_uart(port, &bytes);
        }
    }

    fn deliver_uart(&mut self, port: UartPort, bytes: &[u8]) {
        let bus = &mut self.backend.machine_mut().bus;
        let uart = match port {
            UartPort::Uart0 => &mut bus.uart0,
            UartPort::Uart1 => &mut bus.uart1,
        };
        for byte in bytes {
            let _ = uart.update(UartUpdate::Receive(*byte));
        }
    }

    fn status(&mut self) -> MachineStatus {
        self.backend.machine_mut().status()
    }

    fn inspect(&mut self, request: &RuntimeInspectionRequest) -> Result<RuntimeInspection> {
        let backend = &mut self.backend;
        Ok(match request {
            RuntimeInspectionRequest::Machine(request) => {
                match backend.machine_mut().inspect(*request)? {
                    InspectionResponse::Memory(bytes) => RuntimeInspection::Memory(bytes.to_vec()),
                    InspectionResponse::Mmu(mmu) => RuntimeInspection::Mmu(mmu),
                    InspectionResponse::Peripherals(peripherals) => {
                        RuntimeInspection::Peripherals(peripherals)
                    }
                    InspectionResponse::Events(events) => RuntimeInspection::Events(events),
                }
            }
            RuntimeInspectionRequest::LiveMemory(range) => {
                RuntimeInspection::LiveMemory(*range, backend.inspect_live_memory(*range)?)
            }
            RuntimeInspectionRequest::VirtualMemory { address, length } => {
                RuntimeInspection::VirtualMemory(
                    *address,
                    backend.inspect_virtual_memory(*address, *length)?,
                )
            }
            RuntimeInspectionRequest::Translate(address) => RuntimeInspection::Translation(
                *address,
                backend.translate_virtual_address(*address)?,
            ),
            RuntimeInspectionRequest::Execution {
                address,
                before,
                after,
            } => {
                RuntimeInspection::Execution(backend.inspect_execution(*address, *before, *after)?)
            }
            RuntimeInspectionRequest::SearchMemory { pattern } => {
                RuntimeInspection::SearchMemory(backend.search_memory(pattern)?)
            }
        })
    }
}

/// Builds a backend at `entry` from a reset copy of `source`, with block media
/// reloaded from disk and the configured RAM prefill applied.
fn build_backend(
    source: &Machine,
    entry: u32,
    block_media_paths: &BlockMediaPaths,
    initial_ram_writes: &[(PhysicalAddress, Vec<u8>)],
) -> Result<UnicornBackend> {
    let mut machine = source.reset_clone()?;
    for (unit, path) in block_media(block_media_paths) {
        machine.bus.block.update(BlockUpdate::Attach {
            unit,
            media: fs::read(path)?,
        })?;
    }
    for (address, bytes) in initial_ram_writes {
        machine.memory.write_range(*address, bytes)?;
    }
    let mut backend = UnicornBackend::new(machine)?;
    backend.set_program_counter(entry)?;
    Ok(backend)
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

/// Whether the service loop should keep running.
type Continue = bool;

/// The emulator-thread command loop.
struct Service {
    emulator: Emulator,
    lifecycle: LifecycleState,
    commands: Receiver<Command>,
    status: Arc<Mutex<RuntimeStatus>>,
    uart: Arc<Mutex<UartInbox>>,
    status_period: Duration,
    last_publish: Instant,
    remaining_instructions: Option<u64>,
    pending_breakpoint: Option<u32>,
    /// The configured deadline, re-armed on every reset.
    configured_deadline: Option<u64>,
    /// The configured UART inputs in tick order, re-armed on every reset.
    configured_uart: Vec<ScheduledUartInput>,
    /// The armed deadline; cleared once reached so later resumes can run past it.
    execution_deadline: Option<u64>,
    /// Armed UART inputs not yet delivered.
    scheduled_uart: VecDeque<ScheduledUartInput>,
}

impl Service {
    /// Builds the emulator from `config` and runs until shutdown or failure.
    fn start(
        mut config: RuntimeConfig,
        commands: Receiver<Command>,
        status: Arc<Mutex<RuntimeStatus>>,
        uart: Arc<Mutex<UartInbox>>,
    ) {
        let emulator = match Emulator::new(&config) {
            Ok(emulator) => emulator,
            Err(error) => {
                error!(error = %error, "failed to initialize emulator runtime");
                let mut status = status.lock().expect("runtime status mutex poisoned");
                status.lifecycle = LifecycleState::Failed;
                status.last_error = Some(error.to_string());
                return;
            }
        };
        config.scheduled_uart.sort_by_key(|input| input.at_tick);
        let mut service = Self {
            emulator,
            lifecycle: if config.start_paused {
                LifecycleState::Paused
            } else {
                LifecycleState::Running
            },
            commands,
            status,
            uart,
            status_period: config.status_period,
            last_publish: Instant::now(),
            remaining_instructions: None,
            pending_breakpoint: None,
            configured_deadline: config.execution_deadline,
            configured_uart: config.scheduled_uart,
            execution_deadline: None,
            scheduled_uart: VecDeque::new(),
        };
        service.arm_schedule();
        // The emulator holds its own reset copy; release the configured machine's RAM.
        drop(config.machine);
        service.run();
    }

    fn run(mut self) {
        info!(
            lifecycle = ?self.lifecycle,
            tick = self.emulator.ticks(),
            "emulator runtime started"
        );
        self.publish(None, None);
        loop {
            let proceed = self.process_commands()
                && match self.lifecycle {
                    LifecycleState::Running => self.step(),
                    LifecycleState::Paused => self.wait_for_command(),
                    _ => true,
                };
            if !proceed {
                break;
            }
        }
    }

    /// Runs one bounded batch of guest instructions.
    fn step(&mut self) -> Continue {
        let current_tick = self.emulator.ticks();
        self.deliver_scheduled_uart(current_tick);
        if self
            .execution_deadline
            .is_some_and(|deadline| current_tick >= deadline)
        {
            self.execution_deadline = None;
            self.pause(Some("execution deadline reached".into()));
            return true;
        }
        self.emulator.drain_uart(&self.uart);

        // Never run past the next scheduled event or the caller's instruction limit.
        let tick_boundary = self
            .execution_deadline
            .into_iter()
            .chain(self.scheduled_uart.front().map(|input| input.at_tick))
            .filter(|boundary| *boundary > current_tick)
            .min();
        let instruction_budget = [
            self.remaining_instructions,
            tick_boundary.map(|boundary| boundary - current_tick),
        ]
        .into_iter()
        .flatten()
        .map(|limit| usize::try_from(limit).unwrap_or(usize::MAX))
        .fold(self.emulator.instruction_batch, usize::min);

        let stop = self.emulator.run_batch(instruction_budget, tick_boundary);
        let executed = self.emulator.ticks().saturating_sub(current_tick);
        let instruction_limit_reached =
            self.remaining_instructions
                .as_mut()
                .is_some_and(|remaining| {
                    *remaining = remaining.saturating_sub(executed);
                    *remaining == 0
                });
        self.handle_stop(stop, instruction_limit_reached)
    }

    /// Arms the configured deadline and UART inputs for a run starting at tick zero.
    fn arm_schedule(&mut self) {
        self.execution_deadline = self.configured_deadline;
        self.scheduled_uart = self.configured_uart.iter().cloned().collect();
    }

    fn deliver_scheduled_uart(&mut self, current_tick: u64) {
        while let Some(input) = self
            .scheduled_uart
            .pop_front_if(|input| input.at_tick <= current_tick)
        {
            self.emulator.deliver_uart(input.port, &input.bytes);
        }
    }

    fn handle_stop(&mut self, stop: BackendStop, instruction_limit_reached: bool) -> Continue {
        match stop {
            BackendStop::Unicorn(error) => {
                self.fail_with_flush(Some(format!("{error:?}")), format!("backend {error:?}"));
                false
            }
            BackendStop::Breakpoint { address, immediate } => {
                info!(
                    address,
                    immediate,
                    tick = self.emulator.ticks(),
                    "guest breakpoint paused runtime"
                );
                self.pending_breakpoint = Some(address);
                self.pause(Some(format!(
                    "breakpoint #0x{immediate:04x} at 0x{address:08x}"
                )));
                true
            }
            stop => {
                trace!(
                    ?stop,
                    pc = self.emulator.backend.program_counter().ok(),
                    tick = self.emulator.ticks(),
                    "emulator batch stopped"
                );
                if instruction_limit_reached {
                    info!(
                        tick = self.emulator.ticks(),
                        "bounded runtime execution completed"
                    );
                    self.pause(Some(format!("instruction limit reached ({stop:?})")));
                } else if self.last_publish.elapsed() >= self.status_period {
                    self.publish(Some(format!("{stop:?}")), None);
                    self.last_publish = Instant::now();
                }
                true
            }
        }
    }

    /// Blocks briefly for the next command while paused.
    fn wait_for_command(&mut self) -> Continue {
        match self.commands.recv_timeout(Duration::from_millis(10)) {
            Ok(command) => self.process_command(command),
            Err(mpsc::RecvTimeoutError::Timeout) => true,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.stop();
                false
            }
        }
    }

    /// Processes every queued command without blocking.
    fn process_commands(&mut self) -> Continue {
        loop {
            match self.commands.try_recv() {
                Ok(command) => {
                    if !self.process_command(command) {
                        return false;
                    }
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => {
                    self.stop();
                    return false;
                }
            }
        }
    }

    fn process_command(&mut self, command: Command) -> Continue {
        match command {
            Command::Pause if self.lifecycle == LifecycleState::Running => self.pause(None),
            Command::Resume(limit) if self.lifecycle == LifecycleState::Paused => {
                return self.resume(limit);
            }
            Command::Reset
                if matches!(
                    self.lifecycle,
                    LifecycleState::Running | LifecycleState::Paused
                ) =>
            {
                self.reset();
            }
            Command::Shutdown => {
                info!(
                    lifecycle = ?self.lifecycle,
                    tick = self.emulator.ticks(),
                    "shutting down emulator runtime"
                );
                self.stop();
                return false;
            }
            Command::Inspect(request, response) => self.inspect(request, response),
            _ => {}
        }
        true
    }

    /// Pauses execution, persists media, and publishes `stop` as the reason.
    fn pause(&mut self, stop: Option<String>) {
        self.transition(LifecycleState::Paused);
        self.remaining_instructions = None;
        let flush_error = self.emulator.flush().err().map(|error| error.to_string());
        self.publish(stop, flush_error);
        self.last_publish = Instant::now();
    }

    fn resume(&mut self, instruction_limit: Option<NonZeroU64>) -> Continue {
        if let Some(address) = self.pending_breakpoint.take()
            && let Err(error) = self.emulator.resume_from_breakpoint(address)
        {
            self.lifecycle = LifecycleState::Failed;
            self.publish(None, Some(error.to_string()));
            return false;
        }
        self.transition(LifecycleState::Running);
        self.remaining_instructions = instruction_limit.map(NonZeroU64::get);
        self.publish_cleared();
        true
    }

    fn reset(&mut self) {
        info!(
            lifecycle = ?self.lifecycle,
            tick = self.emulator.ticks(),
            "resetting emulator runtime"
        );
        match self.emulator.reset() {
            Ok(()) => {
                self.pending_breakpoint = None;
                self.arm_schedule();
                self.publish_cleared();
            }
            Err(error) => {
                error!(error = %error, "emulator reset failed");
                self.publish(None, Some(error.to_string()));
            }
        }
    }

    fn inspect(
        &mut self,
        request: RuntimeInspectionRequest,
        response: SyncSender<InspectionResult>,
    ) {
        let lifecycle = self.lifecycle;
        let tick = self.emulator.ticks();
        debug!(
            ?request,
            ?lifecycle,
            tick,
            "processing runtime inspection request"
        );
        let result = self.emulator.inspect(&request);
        match &result {
            Ok(_) => debug!(
                ?request,
                ?lifecycle,
                tick,
                "completed runtime inspection request"
            ),
            Err(error) => {
                warn!(?request, ?lifecycle, tick, error = %error, "runtime inspection request failed")
            }
        }
        // A receiver that has gone away no longer wants the result.
        let _ = response.try_send(result);
    }

    /// Persists media and publishes the terminal lifecycle state.
    fn stop(&mut self) {
        self.transition(LifecycleState::Stopping);
        self.publish(None, None);
        match self.emulator.flush() {
            Ok(()) => {
                self.transition(LifecycleState::Stopped);
                self.publish(None, None);
            }
            Err(error) => {
                error!(error = %error, "runtime shutdown flush failed");
                self.lifecycle = LifecycleState::Failed;
                self.publish(None, Some(error.to_string()));
            }
        }
    }

    /// Marks the runtime failed after attempting to persist media.
    fn fail_with_flush(&mut self, stop: Option<String>, detail: String) {
        let detail = match self.emulator.flush() {
            Ok(()) => detail,
            Err(flush) => format!("{detail}; flush {flush}"),
        };
        self.lifecycle = LifecycleState::Failed;
        self.publish(stop, Some(detail));
    }

    fn transition(&mut self, to: LifecycleState) {
        info!(
            from = ?self.lifecycle,
            ?to,
            tick = self.emulator.ticks(),
            "runtime lifecycle transition"
        );
        self.lifecycle = to;
    }

    /// Publishes the current lifecycle and machine status. `None` leaves the
    /// previously published stop reason or error unchanged.
    fn publish(&mut self, last_stop: Option<String>, last_error: Option<String>) {
        self.update_status(|status| {
            if let Some(stop) = last_stop {
                status.last_stop = Some(stop);
            }
            if let Some(error) = last_error {
                status.last_error = Some(error);
            }
        });
    }

    /// Publishes status with the previous stop reason and error cleared.
    fn publish_cleared(&mut self) {
        self.update_status(|status| {
            status.last_stop = None;
            status.last_error = None;
        });
    }

    /// Atomically publishes lifecycle and machine status along with `update`.
    fn update_status(&mut self, update: impl FnOnce(&mut RuntimeStatus)) {
        let machine = self.emulator.status();
        let mut status = self.status.lock().expect("runtime status mutex poisoned");
        status.lifecycle = self.lifecycle;
        status.machine = machine;
        update(&mut status);
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
