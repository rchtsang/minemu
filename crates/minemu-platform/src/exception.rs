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
