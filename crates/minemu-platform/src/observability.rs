use crate::{ExceptionKind, FaultStatus, PhysicalAddress, PhysicalRange};

/// Data requested on demand rather than copied into every machine status update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionRequest {
    Memory(PhysicalRange),
    Mmu,
    Peripherals,
    Events,
}

/// MMU state suitable for observation without exposing backend internals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MmuInspection {
    pub enabled: bool,
    pub ttbr0: PhysicalAddress,
    pub last_fault_address: Option<u32>,
    pub last_fault_status: Option<FaultStatus>,
}

/// Snapshot of UART state and bounded transmit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UartInspection {
    pub rx_queued: usize,
    pub rx_irq_enabled: bool,
    pub tx_history: Vec<u8>,
}

/// Snapshot of SysTick state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SysTickInspection {
    pub period: u32,
    pub control: u32,
    pub status: u32,
}

/// Snapshot of interrupt-controller state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptInspection {
    pub enabled: u32,
    pub claim: Option<u32>,
    pub priorities: [u8; 4],
}

/// Snapshot of block-device command and media state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockInspection {
    pub status: u32,
    pub error: u32,
    pub dirty_sector_count: usize,
    pub media_attached: bool,
}

/// Snapshot of RNG state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RngInspection {
    pub state: u32,
}

/// One guest trace event in the bounded trace history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceInspectionEvent {
    pub tick: u64,
    pub value: u32,
}

/// A bounded observable machine event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservableEvent {
    Trace(TraceInspectionEvent),
    Exception(ExceptionKind),
}

/// Snapshot of all platform peripheral state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeripheralsInspection {
    pub interrupts: InterruptInspection,
    pub systick: SysTickInspection,
    pub block: BlockInspection,
    pub rng: RngInspection,
    pub uart0: UartInspection,
    pub uart1: UartInspection,
    pub trace: Vec<TraceInspectionEvent>,
}

/// Response to an `InspectionRequest`.
///
/// Memory responses borrow the machine for their lifetime. This permits
/// zero-copy synchronous inspection while preventing concurrent mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InspectionResponse<'a> {
    Memory(&'a [u8]),
    Mmu(MmuInspection),
    Peripherals(PeripheralsInspection),
    Events(Vec<ObservableEvent>),
}
