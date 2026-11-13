//! Unicorn-backed A32 execution adapter for the backend-independent machine core.

use std::collections::HashSet;

use minemu_core::{ExceptionPlan, InstructionOutcome, Machine, MmuFault};
use minemu_platform::{
    Access, CPSR_IRQ_DISABLE, CPSR_MODE_MASK, CPSR_THUMB, Cp15Operation, CpuMode, ExceptionKind,
    FaultCause, MMIO_BASE, MMIO_SIZE, MemRegion, PAGE_SIZE, VirtualAddress,
};
use thiserror::Error;
use tracing::trace;
use unicorn_engine::{
    ArmCpuModel, RegisterARM, Unicorn,
    unicorn_const::{Arch, Mode, Prot, uc_error},
};

use crate::arm::{self, FaultRegister, PendingCp15};

mod hooks;
mod inspect;
#[cfg(test)]
mod tests;

const MEMORY_SEARCH_CHUNK_SIZE: usize = 1024 * 1024;

/// An explicit reason why a bounded backend run stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendStop {
    InstructionBudget,
    Breakpoint {
        address: u32,
        immediate: u16,
    },
    MmioFault {
        address: u32,
        access: Access,
        detail: String,
    },
    MmuFault(MmuFault),
    Cp15Boundary,
    Exception(ExceptionKind),
    Unicorn(uc_error),
}

/// CPU state captured at an emulator-thread execution boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuState {
    pub registers: [u32; 16],
    pub cpsr: u32,
    pub spsr: u32,
}

/// Live execution data captured directly from Unicorn at an execution boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionInspection {
    pub registers: [u32; 16],
    pub cpsr: u32,
    pub spsr: u32,
    pub mmu_enabled: bool,
    pub instruction_address: VirtualAddress,
    pub instruction_bytes: Vec<u8>,
    pub instruction_error: Option<String>,
}

/// Errors while constructing or operating the Unicorn backend.
#[derive(Debug, Error)]
pub enum BackendError {
    #[error(transparent)]
    Core(#[from] minemu_core::CoreError),
    #[error("Unicorn operation failed: {0:?}")]
    Unicorn(uc_error),
    #[error("inspection byte range is too large")]
    InspectionRange,
    #[error("memory search pattern is empty")]
    EmptySearchPattern,
    #[error("memory search pattern exceeds the {maximum}-byte limit: {length} bytes")]
    SearchPatternTooLarge { length: usize, maximum: usize },
    #[error("virtual address 0x{address:08x} has no translation for inspection ({cause:?})")]
    InspectionTranslation { address: u32, cause: FaultCause },
    #[error("virtual memory inspection resolves to device address 0x{0:08x}")]
    VirtualInspectionDevice(u32),
}

impl From<uc_error> for BackendError {
    fn from(error: uc_error) -> Self {
        Self::Unicorn(error)
    }
}

type Result<T> = std::result::Result<T, BackendError>;

/// A synchronous exception raised by an instruction, entered at a batch boundary.
#[derive(Clone, Copy)]
struct SyncException {
    kind: ExceptionKind,
    pc: VirtualAddress,
}

/// State shared with Unicorn callbacks during one batch.
pub(crate) struct BackendData {
    machine: Machine,
    /// Whether an instruction has begun and not yet been retired. It is retired
    /// at the next instruction boundary, or when the batch ends.
    instruction_in_flight: bool,
    /// Virtual address of the instruction currently executing.
    current_instruction: u32,
    /// Physical page numbers Unicorn has fetched code from since its
    /// translation cache was last flushed.
    code_pages: HashSet<u32>,
    callback_stop: Option<BackendStop>,
    pending_cp15: Option<PendingCp15>,
    pending_exception: Option<SyncException>,
}

/// Stop requests recorded by callbacks during one batch.
struct BatchRequests {
    callback_stop: Option<BackendStop>,
    pending_exception: Option<SyncException>,
    pending_cp15: Option<PendingCp15>,
}

/// Safe, single-threaded Unicorn A32 backend bound to one core machine.
pub struct UnicornBackend {
    engine: Unicorn<'static, BackendData>,
    /// An exception whose instruction retired exactly at a tick deadline; it is
    /// entered at the start of the next run.
    deferred_exception: Option<SyncException>,
}

impl UnicornBackend {
    /// Configures an A32 little-endian Cortex-A9 with the ABI physical map.
    pub fn new(mut machine: Machine) -> Result<Self> {
        let boot_rom = machine.copy_region(MemRegion::BootRom)?;
        let system_rom = machine.copy_region(MemRegion::SystemRom)?;
        let ram = machine.memory.ram_mut_ptr();

        let mut engine = Unicorn::new_with_data(
            Arch::ARM,
            Mode::ARM | Mode::LITTLE_ENDIAN,
            BackendData {
                machine,
                instruction_in_flight: false,
                current_instruction: 0,
                code_pages: HashSet::new(),
                callback_stop: None,
                pending_cp15: None,
                pending_exception: None,
            },
        )?;
        engine.ctl_set_cpu_model(ArmCpuModel::CORTEX_A9 as i32)?;
        // Batches are bounded by instruction count. Address exits make
        // Unicorn translate the synthetic `until` value through the guest MMU.
        engine.ctl_exits_enable()?;

        let mut backend = Self {
            engine,
            deferred_exception: None,
        };
        backend.map_bytes(MemRegion::BootRom, Prot::READ | Prot::EXEC, &boot_rom)?;
        backend.map_bytes(MemRegion::SystemRom, Prot::READ | Prot::EXEC, &system_rom)?;
        backend.map_shared_ram(ram)?;
        backend.map_mmio_window()?;
        hooks::register(&mut backend.engine)?;

        Ok(backend)
    }

    /// Runs no more than `instruction_budget` instructions from `start` toward `end`.
    pub fn run(&mut self, start: u32, end: u32, instruction_budget: usize) -> BackendStop {
        self.run_with_tick_deadline(start, end, instruction_budget, None)
    }

    /// Runs without crossing the supplied virtual-time boundary.
    pub fn run_until_tick(
        &mut self,
        start: u32,
        end: u32,
        instruction_budget: usize,
        tick_deadline: u64,
    ) -> BackendStop {
        self.run_with_tick_deadline(start, end, instruction_budget, Some(tick_deadline))
    }

    fn run_with_tick_deadline(
        &mut self,
        start: u32,
        end: u32,
        instruction_budget: usize,
        tick_deadline: Option<u64>,
    ) -> BackendStop {
        if let Some(stop) = self.resume_deferred_exception(tick_deadline) {
            return stop;
        }
        if self.deadline_reached(tick_deadline) {
            return BackendStop::InstructionBudget;
        }
        if tick_deadline.is_some()
            && let Some(stop) = self.deliver_irq()
        {
            return stop;
        }
        let instruction_budget = tick_deadline.map_or(instruction_budget, |deadline| {
            instruction_budget
                .min(usize::try_from(deadline - self.machine().ticks()).unwrap_or(usize::MAX))
        });
        if let Err(error) = self.prepare_batch(end) {
            return BackendStop::Unicorn(error);
        }

        let result = self
            .engine
            .emu_start(u64::from(start), u64::from(end), 0, instruction_budget);

        let requests = self.retire_batch();
        if let Some(stop) = requests.callback_stop {
            return self.finish_callback_stop(stop);
        }
        if let Some(exception) = requests.pending_exception {
            return self.finish_synchronous_exception(exception, tick_deadline);
        }
        if let Some(pending) = requests.pending_cp15 {
            return self.finish_cp15(pending, tick_deadline);
        }
        if !self.deadline_reached(tick_deadline)
            && let Some(stop) = self.deliver_irq()
        {
            return stop;
        }
        match result {
            Ok(()) => BackendStop::InstructionBudget,
            Err(error) => BackendStop::Unicorn(error),
        }
    }

    fn deadline_reached(&self, tick_deadline: Option<u64>) -> bool {
        tick_deadline.is_some_and(|deadline| self.machine().ticks() >= deadline)
    }

    /// Installs the batch exit address and clears per-batch callback state.
    fn prepare_batch(&mut self, end: u32) -> std::result::Result<(), uc_error> {
        self.sync_dma_writes()?;
        if end == u32::MAX {
            self.engine.ctl_set_exits(&[])?;
        } else {
            self.engine.ctl_set_exits(&[u64::from(end)])?;
        }
        let data = self.engine.get_data_mut();
        data.instruction_in_flight = false;
        data.callback_stop = None;
        data.pending_cp15 = None;
        data.pending_exception = None;
        Ok(())
    }

    /// Discards code Unicorn translated from RAM that block reads have since
    /// overwritten. Device DMA writes the shared RAM directly, which Unicorn
    /// cannot observe.
    ///
    /// Unicorn can only invalidate by virtual address, through the guest's
    /// translation, so the whole translation cache is flushed instead, and
    /// only when a read overwrote a page code was fetched from. The TLB is
    /// flushed too so that later fetches refill `code_pages`.
    fn sync_dma_writes(&mut self) -> std::result::Result<(), uc_error> {
        let ranges = self.machine_mut().bus.block.take_ram_writes();
        let data = self.engine.get_data_mut();
        let overwrote_code = ranges.iter().any(|range| {
            let first = range.start().get() / PAGE_SIZE;
            let last = (range.start().get() + (range.length() - 1)) / PAGE_SIZE;
            (first..=last).any(|page| data.code_pages.contains(&page))
        });
        if overwrote_code {
            data.code_pages.clear();
            self.engine.ctl_flush_tb()?;
            self.engine.ctl_flush_tlb()?;
        }
        Ok(())
    }

    /// Collects callback requests and retires the batch's final instruction.
    ///
    /// Earlier instructions were retired at the following instruction boundary
    /// by the boundary hook. An instruction that stopped the batch by faulting
    /// or trapping has not completed; its tick is charged when its exception is
    /// entered. A failed fetch happens before its instruction begins, so the
    /// instruction in flight then is the previous, completed one.
    fn retire_batch(&mut self) -> BatchRequests {
        let data = self.engine.get_data_mut();
        let requests = BatchRequests {
            callback_stop: data.callback_stop.take(),
            pending_exception: data.pending_exception.take(),
            pending_cp15: data.pending_cp15.take(),
        };
        let faulting_instruction =
            matches!(requests.pending_cp15, Some(PendingCp15::Undefined { .. }))
                || requests.pending_exception.is_some()
                || matches!(
                    requests.callback_stop,
                    Some(BackendStop::MmuFault(fault)) if !fault.status.is_fetch()
                )
                || matches!(requests.callback_stop, Some(BackendStop::MmioFault { .. }));
        if std::mem::take(&mut data.instruction_in_flight) && !faulting_instruction {
            data.machine
                .finish_instruction(InstructionOutcome::Completed);
        }
        requests
    }

    /// Returns the core machine owned by this single-threaded backend.
    pub fn machine(&self) -> &Machine {
        &self.engine.get_data().machine
    }

    /// Returns exclusive access to the core machine outside active emulation.
    pub fn machine_mut(&mut self) -> &mut Machine {
        &mut self.engine.get_data_mut().machine
    }

    /// Reads an ARM register from Unicorn.
    pub fn register(&self, register: RegisterARM) -> Result<u32> {
        Ok(self.engine.reg_read(register)? as u32)
    }

    /// Returns the guest program counter outside active emulation.
    pub fn program_counter(&self) -> Result<u32> {
        self.register(RegisterARM::PC)
    }

    /// Sets the guest program counter outside active emulation.
    pub fn set_program_counter(&mut self, value: u32) -> Result<()> {
        self.set_register(RegisterARM::PC, value)
    }

    /// Captures the architecturally visible A32 general register file and status registers.
    pub fn cpu_state(&self) -> Result<CpuState> {
        let mut registers = [0; 16];
        for (index, value) in (0..).zip(&mut registers) {
            *value = self.register(arm::general_register(index))?;
        }
        Ok(CpuState {
            registers,
            cpsr: self.register(RegisterARM::CPSR)?,
            spsr: self.register(RegisterARM::SPSR)?,
        })
    }

    /// Writes an ARM register outside active emulation.
    pub fn set_register(&mut self, register: RegisterARM, value: u32) -> Result<()> {
        Ok(self.engine.reg_write(register, u64::from(value))?)
    }

    /// Returns the CP15 VBAR value maintained by the core MMU state.
    pub fn vector_base(&self) -> u32 {
        self.machine().mmu.vector_base().get()
    }

    fn finish_cp15(&mut self, pending: PendingCp15, tick_deadline: Option<u64>) -> BackendStop {
        match pending {
            PendingCp15::Operation(operation) => {
                self.machine_mut().mmu.apply_cp15(operation);
                if matches!(operation, Cp15Operation::InvalidateAll)
                    && self.engine.ctl_flush_tlb().is_err()
                {
                    return BackendStop::Unicorn(uc_error::ARG);
                }
                BackendStop::Cp15Boundary
            }
            PendingCp15::ReadFault(register, field) => {
                let value = self
                    .machine()
                    .mmu
                    .last_fault()
                    .map_or(0, |fault| match field {
                        FaultRegister::Status => fault.status.raw(),
                        FaultRegister::Address => fault.address.get(),
                    });
                if self.set_register(register, value).is_err() {
                    return BackendStop::Unicorn(uc_error::ARG);
                }
                BackendStop::Cp15Boundary
            }
            PendingCp15::Undefined { pc } => self.finish_synchronous_exception(
                SyncException {
                    kind: ExceptionKind::Undefined,
                    pc: VirtualAddress::new(pc),
                },
                tick_deadline,
            ),
        }
    }

    fn finish_callback_stop(&mut self, stop: BackendStop) -> BackendStop {
        match stop {
            BackendStop::MmuFault(fault) => self.finish_fault(fault),
            BackendStop::MmioFault {
                address, access, ..
            } => {
                let user_mode = self
                    .register(RegisterARM::CPSR)
                    .is_ok_and(arm::is_user_mode);
                let fault = self.machine_mut().mmu.record_invalid_mmio_fault(
                    VirtualAddress::new(address),
                    access,
                    user_mode,
                );
                self.finish_fault(fault)
            }
            stop => stop,
        }
    }

    /// Enters the abort vector for a faulting access, which costs no instruction tick.
    fn finish_fault(&mut self, mut fault: MmuFault) -> BackendStop {
        let pc = self.program_counter().unwrap_or(fault.address.get());
        if fault.status.is_fetch() && fault.address.get() != pc {
            // Unicorn translates code a page at a time, so the translation
            // callback sees the page address; the faulting VA is the PC.
            fault.address = VirtualAddress::new(pc);
            self.machine_mut().mmu.record_fault(fault);
        }
        trace!(
            address = fault.address.get(),
            pc,
            status = fault.status.raw(),
            "entering exception for MMU or MMIO fault"
        );
        match self
            .machine_mut()
            .finish_instruction(InstructionOutcome::Fault(fault, VirtualAddress::new(pc)))
        {
            Some(plan) => self.enter_planned(plan),
            None => BackendStop::Unicorn(uc_error::ARG),
        }
    }

    /// Retires a trapping instruction and enters its exception, unless retiring
    /// it reaches the tick deadline, in which case entry is deferred.
    fn finish_synchronous_exception(
        &mut self,
        exception: SyncException,
        tick_deadline: Option<u64>,
    ) -> BackendStop {
        if tick_deadline.is_some_and(|deadline| self.machine().ticks() + 1 == deadline) {
            self.machine_mut()
                .finish_instruction(InstructionOutcome::Completed);
            self.deferred_exception = Some(exception);
            return BackendStop::InstructionBudget;
        }
        let SyncException { kind, pc } = exception;
        match self
            .machine_mut()
            .finish_instruction(InstructionOutcome::SynchronousException(kind, pc))
        {
            Some(plan) => self.enter_planned(plan),
            None => BackendStop::Exception(kind),
        }
    }

    fn resume_deferred_exception(&mut self, tick_deadline: Option<u64>) -> Option<BackendStop> {
        let SyncException { kind, pc } = self.deferred_exception?;
        if self.deadline_reached(tick_deadline) {
            return Some(BackendStop::InstructionBudget);
        }
        self.deferred_exception = None;
        let plan = self.machine_mut().enter_exception(kind, pc);
        Some(self.enter_planned(plan))
    }

    fn deliver_irq(&mut self) -> Option<BackendStop> {
        let cpsr = self.register(RegisterARM::CPSR).ok()?;
        if cpsr & CPSR_IRQ_DISABLE != 0 {
            return None;
        }
        let _source = self.machine_mut().bus.interrupts.claim()?;
        let pc = self.program_counter().ok()?;
        let plan = self
            .machine_mut()
            .enter_exception(ExceptionKind::Interrupt, VirtualAddress::new(pc));
        Some(self.enter_planned(plan))
    }

    /// Applies an exception plan to the CPU and reports the resulting stop.
    fn enter_planned(&mut self, plan: ExceptionPlan) -> BackendStop {
        let kind = plan.request.kind;
        match self.enter_exception(plan) {
            Ok(()) => BackendStop::Exception(kind),
            Err(_) => BackendStop::Unicorn(uc_error::ARG),
        }
    }

    /// Applies the documented A32 state transition for an exception plan.
    pub fn enter_exception(&mut self, plan: ExceptionPlan) -> Result<()> {
        let previous_cpsr = self.register(RegisterARM::CPSR)?;
        let kind = plan.request.kind;
        let mode = kind.destination_mode();
        debug_assert!(mode != CpuMode::User, "exceptions never enter USR mode");
        // Exception entry selects the destination mode in A32 state with IRQs masked.
        let destination_cpsr =
            (previous_cpsr & !(CPSR_MODE_MASK | CPSR_THUMB)) | mode.bits() | CPSR_IRQ_DISABLE;
        self.set_register(RegisterARM::CPSR, destination_cpsr)?;
        self.set_register(RegisterARM::SPSR, previous_cpsr)?;
        self.set_register(RegisterARM::LR, plan.request.pc.get() + kind.link_offset())?;
        self.set_register(RegisterARM::PC, self.vector_base() + kind.vector_offset())
    }

    fn map_bytes(&mut self, region: MemRegion, permissions: Prot, bytes: &[u8]) -> Result<()> {
        let base = u64::from(region.base().get());
        self.engine
            .mem_map(base, u64::from(region.size()), permissions)?;
        Ok(self.engine.mem_write(base, bytes)?)
    }

    fn map_shared_ram(&mut self, ram: *mut u8) -> Result<()> {
        let region = MemRegion::Ram;
        // `ram` points to the fixed-size allocation owned by BackendData's
        // Machine. Moving the Box does not move its allocation, and Unicorn is
        // closed before BackendData is dropped.
        unsafe {
            self.engine.mem_map_ptr(
                u64::from(region.base().get()),
                u64::from(region.size()),
                Prot::ALL,
                ram.cast(),
            )
        }?;
        Ok(())
    }

    fn map_mmio_window(&mut self) -> Result<()> {
        Ok(self.engine.mmio_map(
            u64::from(MMIO_BASE),
            u64::from(MMIO_SIZE),
            Some(hooks::mmio_read_callback),
            Some(hooks::mmio_write_callback),
        )?)
    }
}
