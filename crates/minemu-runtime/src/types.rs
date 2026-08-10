use std::{path::PathBuf, time::Duration};

use minemu_core::{CoreError, Machine, MachineStatus};
use minemu_platform::{
    InspectionRequest, MmuInspection, ObservableEvent, PeripheralsInspection, PhysicalAddress,
    PhysicalRange,
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
    Execution(ExecutionInspection),
}

/// An inspection request that is always performed on the emulator thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeInspectionRequest {
    /// Delegates to the backend-independent machine inspection API.
    Machine(InspectionRequest),
    /// Reads the authoritative physical RAM mapping from Unicorn.
    LiveMemory(PhysicalRange),
    /// Captures CPU state and instruction bytes from Unicorn.
    Execution { before: usize, after: usize },
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
