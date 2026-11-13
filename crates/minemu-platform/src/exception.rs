use crate::{FaultStatus, VirtualAddress};

/// CPU modes used by the supported exception paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuMode {
    User,
    Supervisor,
    Interrupt,
    Abort,
    Undefined,
}

/// CPSR mode field.
pub const CPSR_MODE_MASK: u32 = 0x1f;
/// CPSR Thumb execution-state bit.
pub const CPSR_THUMB: u32 = 1 << 5;
/// CPSR FIQ-disable bit.
pub const CPSR_FIQ_DISABLE: u32 = 1 << 6;
/// CPSR IRQ-disable bit.
pub const CPSR_IRQ_DISABLE: u32 = 1 << 7;
/// CPSR condition flags.
pub const CPSR_NEGATIVE: u32 = 1 << 31;
pub const CPSR_ZERO: u32 = 1 << 30;
pub const CPSR_CARRY: u32 = 1 << 29;
pub const CPSR_OVERFLOW: u32 = 1 << 28;

impl CpuMode {
    /// Returns the A32 CPSR mode-field encoding.
    pub const fn bits(self) -> u32 {
        match self {
            Self::User => 0x10,
            Self::Interrupt => 0x12,
            Self::Supervisor => 0x13,
            Self::Abort => 0x17,
            Self::Undefined => 0x1b,
        }
    }

    /// Decodes the mode field of a CPSR value, if it names a supported mode.
    pub const fn from_cpsr(cpsr: u32) -> Option<Self> {
        match cpsr & CPSR_MODE_MASK {
            0x10 => Some(Self::User),
            0x12 => Some(Self::Interrupt),
            0x13 => Some(Self::Supervisor),
            0x17 => Some(Self::Abort),
            0x1b => Some(Self::Undefined),
            _ => None,
        }
    }

    /// Returns whether this is a privileged mode.
    pub const fn is_privileged(self) -> bool {
        !matches!(self, Self::User)
    }
}

/// A platform exception kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExceptionKind {
    Undefined,
    SupervisorCall,
    PrefetchAbort,
    DataAbort,
    Interrupt,
}

impl ExceptionKind {
    /// Returns the vector offset from VBAR.
    pub const fn vector_offset(self) -> u32 {
        match self {
            Self::Undefined => 0x04,
            Self::SupervisorCall => 0x08,
            Self::PrefetchAbort => 0x0c,
            Self::DataAbort => 0x10,
            Self::Interrupt => 0x18,
        }
    }

    /// Returns the CPU mode entered by this exception.
    pub const fn destination_mode(self) -> CpuMode {
        match self {
            Self::Undefined => CpuMode::Undefined,
            Self::SupervisorCall => CpuMode::Supervisor,
            Self::PrefetchAbort | Self::DataAbort => CpuMode::Abort,
            Self::Interrupt => CpuMode::Interrupt,
        }
    }

    /// Returns the fixed C dispatcher ID for a synchronous exception.
    pub const fn dispatch_id(self) -> Option<i32> {
        match self {
            Self::Undefined => Some(-4),
            Self::SupervisorCall => Some(-3),
            Self::PrefetchAbort => Some(-2),
            Self::DataAbort => Some(-1),
            Self::Interrupt => None,
        }
    }

    /// Returns the offset added to the exception PC to form the banked LR.
    pub const fn link_offset(self) -> u32 {
        match self {
            Self::DataAbort => 8,
            _ => 4,
        }
    }

    /// Returns virtual-time cost of platform exception entry.
    pub const fn entry_ticks(self) -> u64 {
        1
    }
}

/// Backend-neutral information needed to enter an exception vector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExceptionRequest {
    /// Exception kind to enter.
    pub kind: ExceptionKind,
    /// Address of the instruction or boundary that caused entry.
    pub pc: VirtualAddress,
    /// Fault metadata for prefetch and data aborts.
    pub fault: Option<FaultStatus>,
}

impl ExceptionRequest {
    /// Creates a synchronous exception request without fault metadata.
    pub const fn synchronous(kind: ExceptionKind, pc: VirtualAddress) -> Self {
        Self {
            kind,
            pc,
            fault: None,
        }
    }

    /// Creates an abort request with encoded fault metadata.
    pub const fn abort(kind: ExceptionKind, pc: VirtualAddress, fault: FaultStatus) -> Self {
        Self {
            kind,
            pc,
            fault: Some(fault),
        }
    }
}
