use std::{path::PathBuf, time::Duration};

use minemu_core::{CoreError, Machine, MachineStatus};
use minemu_platform::{
    InspectionRequest, MmuInspection, ObservableEvent, PeripheralsInspection, PhysicalAddress,
    PhysicalRange, VirtualAddress,
};
use minemu_unicorn::{BackendError, ExecutionInspection};
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
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Uart0 => 0,
            Self::Uart1 => 1,
        }
    }
}

/// UART bytes delivered by the emulator thread at an exact virtual-time boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledUartInput {
    pub at_tick: u64,
    pub port: UartPort,
    pub bytes: Vec<u8>,
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
    LiveMemory(PhysicalRange, Vec<u8>),
    VirtualMemory(VirtualAddress, Vec<u8>),
    Translation(VirtualAddress, PhysicalAddress),
    Execution(ExecutionInspection),
    SearchMemory(Option<PhysicalAddress>),
}

/// An inspection request that is always performed on the emulator thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeInspectionRequest {
    /// Delegates to the backend-independent machine inspection API.
    Machine(InspectionRequest),
    /// Reads the authoritative physical RAM mapping from Unicorn.
    LiveMemory(PhysicalRange),
    /// Reads virtual data memory through the active MMU without invoking MMIO.
    VirtualMemory {
        address: VirtualAddress,
        length: usize,
    },
    /// Translates one virtual address through the active MMU.
    Translate(VirtualAddress),
    /// Captures CPU state and instruction bytes from Unicorn.
    Execution {
        address: Option<minemu_platform::VirtualAddress>,
        before: usize,
        after: usize,
    },
    /// Searches the authoritative physical RAM mapping from Unicorn.
    SearchMemory { pattern: Vec<u8> },
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
    pub initial_ram_writes: Vec<(PhysicalAddress, Vec<u8>)>,
    pub start_paused: bool,
    pub execution_deadline: Option<u64>,
    pub scheduled_uart: Vec<ScheduledUartInput>,
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
            initial_ram_writes: Vec::new(),
            start_paused: false,
            execution_deadline: None,
            scheduled_uart: Vec::new(),
        }
    }

    /// Adds a RAM write applied after every machine reset and before execution.
    pub fn with_initial_ram_write(mut self, address: PhysicalAddress, bytes: Vec<u8>) -> Self {
        self.initial_ram_writes.push((address, bytes));
        self
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

pub(crate) type Result<T> = std::result::Result<T, RuntimeError>;
pub type InspectionResult = std::result::Result<RuntimeInspection, RuntimeError>;
