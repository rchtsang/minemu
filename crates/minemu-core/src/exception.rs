use minemu_platform::{ExceptionKind, ExceptionRequest, FaultStatus, VirtualAddress};

use crate::MmuFault;

/// Backend-neutral exception entry plan for the later CPU adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExceptionPlan {
    pub request: ExceptionRequest,
    pub dispatch_id: Option<i32>,
}

impl ExceptionPlan {
    pub const fn synchronous(kind: ExceptionKind, pc: VirtualAddress) -> Self {
        Self {
            request: ExceptionRequest::synchronous(kind, pc),
            dispatch_id: kind.dispatch_id(),
        }
    }

    pub const fn fault(fault: MmuFault, pc: VirtualAddress) -> Self {
        let kind = if fault.status.is_fetch() {
            ExceptionKind::PrefetchAbort
        } else {
            ExceptionKind::DataAbort
        };
        Self {
            request: ExceptionRequest::abort(kind, pc, fault.status),
            dispatch_id: kind.dispatch_id(),
        }
    }

    pub const fn invalid_mmio(pc: VirtualAddress, status: FaultStatus) -> Self {
        Self {
            request: ExceptionRequest::abort(ExceptionKind::DataAbort, pc, status),
            dispatch_id: ExceptionKind::DataAbort.dispatch_id(),
        }
    }
}
