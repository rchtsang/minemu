//! Unicorn-backed A32 execution adapter for the backend-independent machine core.

use minemu_core::{ExceptionPlan, InstructionOutcome, Machine, MmuFault};
use minemu_platform::{
    Access, MemRegion, MmioTransaction, MmioWidth, PAGE_SIZE, PhysicalAddress, PhysicalRange,
    VirtualAddress,
};
use thiserror::Error;
use tracing::{debug, debug_span, trace, warn};
use unicorn_engine::{
    ArmCpuModel, RegisterARM, Unicorn,
    unicorn_const::{Arch, Mode, Prot, TlbEntry, TlbType, uc_error},
};

use crate::arm::{self, PendingCp15};

const MEMORY_SEARCH_CHUNK_SIZE: usize = 1024 * 1024;

/// An explicit reason why a bounded backend run stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendStop {
    InstructionBudget,
    MmioFault {
        address: u32,
        access: Access,
        detail: String,
    },
    MmuFault(MmuFault),
    Cp15Boundary,
    Exception(minemu_platform::ExceptionKind),
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
    #[error("virtual memory inspection resolves to device address 0x{0:08x}")]
    VirtualInspectionDevice(u32),
}

type Result<T> = std::result::Result<T, BackendError>;

pub(crate) struct BackendData {
    machine: Machine,
    executed_instructions: u64,
    callback_stop: Option<BackendStop>,
    pending_cp15: Option<PendingCp15>,
    pending_exception: Option<PendingException>,
}

enum PendingException {
    Synchronous {
        kind: minemu_platform::ExceptionKind,
        pc: u32,
    },
    Fault(MmuFault),
}

/// Safe, single-threaded Unicorn A32 backend bound to one core machine.
pub struct UnicornBackend {
    engine: Unicorn<'static, BackendData>,
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
                executed_instructions: 0,
                callback_stop: None,
                pending_cp15: None,
                pending_exception: None,
            },
        )
        .map_err(BackendError::Unicorn)?;
        engine
            .ctl_set_cpu_model(ArmCpuModel::CORTEX_A9 as i32)
            .map_err(BackendError::Unicorn)?;
        // Batches are bounded by instruction count. Address exits make
        // Unicorn translate the synthetic `until` value through the guest MMU.
        engine.ctl_exits_enable().map_err(BackendError::Unicorn)?;

        let mut backend = Self { engine };
        backend.map_bytes(MemRegion::BootRom, Prot::READ | Prot::EXEC, &boot_rom)?;
        backend.map_bytes(MemRegion::SystemRom, Prot::READ | Prot::EXEC, &system_rom)?;
        backend.map_shared_ram(ram)?;
        backend.map_mmio_window()?;
        hooks::register(&mut backend.engine)?;

        Ok(backend)
    }

    /// Runs no more than `instruction_budget` instructions from `start` toward `end`.
    pub fn run(&mut self, start: u32, end: u32, instruction_budget: usize) -> BackendStop {
        let exits = if end == u32::MAX {
            self.engine.ctl_set_exits(&[])
        } else {
            self.engine.ctl_set_exits(&[u64::from(end)])
        };
        if let Err(error) = exits {
            return BackendStop::Unicorn(error);
        }
        let data = self.engine.get_data_mut();
        data.executed_instructions = 0;
        data.callback_stop = None;
        data.pending_cp15 = None;
        data.pending_exception = None;

        let result = self
            .engine
            .emu_start(u64::from(start), u64::from(end), 0, instruction_budget);
        let (pending_cp15, pending_exception, callback_stop) = {
            let data = self.engine.get_data_mut();
            let pending_cp15 = data.pending_cp15.take();
            let pending_exception = data.pending_exception.take();
            let callback_stop = data.callback_stop.take();
            let faulting_instruction = matches!(pending_cp15, Some(PendingCp15::Undefined { .. }))
                || pending_exception.is_some()
                || matches!(
                    callback_stop,
                    Some(BackendStop::MmuFault(_) | BackendStop::MmioFault { .. })
                );
            let completed_instructions = data
                .executed_instructions
                .saturating_sub(u64::from(faulting_instruction));
            for _ in 0..completed_instructions {
                data.machine
                    .finish_instruction(InstructionOutcome::Completed);
            }
            (pending_cp15, pending_exception, callback_stop)
        };
        if let Some(stop) = callback_stop {
            return self.finish_callback_stop(stop);
        }
        if let Some(exception) = pending_exception {
            return self.finish_pending_exception(exception);
        }
        if let Some(pending) = pending_cp15 {
            return self.finish_cp15(pending);
        }
        if let Some(stop) = self.deliver_irq() {
            return stop;
        }
        match result {
            Ok(()) => BackendStop::InstructionBudget,
            Err(error) => BackendStop::Unicorn(error),
        }
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
        self.engine
            .reg_read(register)
            .map(|value| value as u32)
            .map_err(BackendError::Unicorn)
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
        let registers = [
            self.register(RegisterARM::R0)?,
            self.register(RegisterARM::R1)?,
            self.register(RegisterARM::R2)?,
            self.register(RegisterARM::R3)?,
            self.register(RegisterARM::R4)?,
            self.register(RegisterARM::R5)?,
            self.register(RegisterARM::R6)?,
            self.register(RegisterARM::R7)?,
            self.register(RegisterARM::R8)?,
            self.register(RegisterARM::R9)?,
            self.register(RegisterARM::R10)?,
            self.register(RegisterARM::R11)?,
            self.register(RegisterARM::R12)?,
            self.register(RegisterARM::SP)?,
            self.register(RegisterARM::LR)?,
            self.register(RegisterARM::PC)?,
        ];
        Ok(CpuState {
            registers,
            cpsr: self.register(RegisterARM::CPSR)?,
            spsr: self.register(RegisterARM::SPSR)?,
        })
    }

    /// Reads the authoritative physical bytes currently mapped by Unicorn.
    pub fn read_physical_memory(&self, address: PhysicalAddress, length: usize) -> Result<Vec<u8>> {
        trace!(
            physical_address = address.get(),
            length, "reading physical Unicorn memory for inspection"
        );
        let mut bytes = vec![0; length];
        match self.engine.mem_read(u64::from(address.get()), &mut bytes) {
            Ok(()) => Ok(bytes),
            Err(error) => {
                warn!(
                    physical_address = address.get(),
                    length,
                    error = ?error,
                    "Unicorn physical inspection read failed"
                );
                Err(BackendError::Unicorn(error))
            }
        }
    }

    /// Reads virtual instruction bytes through the active Unicorn translation state.
    pub fn read_virtual_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
    ) -> Result<Vec<u8>> {
        self.read_translated_memory(address, length, Prot::EXEC, false)
    }

    /// Reads virtual data bytes without allowing inspection to trigger MMIO reads.
    pub fn inspect_virtual_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
    ) -> Result<Vec<u8>> {
        self.read_translated_memory(address, length, Prot::READ, true)
    }

    /// Translates one virtual address using read permissions and current MMU state.
    pub fn translate_virtual_address(
        &mut self,
        address: VirtualAddress,
    ) -> Result<PhysicalAddress> {
        let physical = self
            .engine
            .vmem_translate(u64::from(address.get()), Prot::READ)
            .map_err(BackendError::Unicorn)?;
        let physical = u32::try_from(physical).map_err(|_| BackendError::InspectionRange)?;
        Ok(PhysicalAddress::new(physical))
    }

    fn read_translated_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
        protection: Prot,
        reject_devices: bool,
    ) -> Result<Vec<u8>> {
        let access = if protection == Prot::EXEC {
            "execute"
        } else {
            "read"
        };
        trace!(
            virtual_address = address.get(),
            length, access, "reading translated Unicorn instruction bytes for inspection"
        );
        let mut bytes = vec![0; length];
        let mut offset = 0;
        while offset < length {
            let offset_u32 = u32::try_from(offset).map_err(|_| BackendError::InspectionRange)?;
            let virtual_address = address
                .get()
                .checked_add(offset_u32)
                .ok_or(BackendError::InspectionRange)?;
            let page_remaining = PAGE_SIZE - virtual_address % PAGE_SIZE;
            let count = (length - offset).min(page_remaining as usize);
            let physical_address = self
                .engine
                .vmem_translate(u64::from(virtual_address), protection)
                .map_err(|error| {
                    warn!(
                        virtual_address,
                        length = count,
                        access,
                        error = ?error,
                        "Unicorn virtual inspection translation failed"
                    );
                    BackendError::Unicorn(error)
                })?;
            let physical_u32 =
                u32::try_from(physical_address).map_err(|_| BackendError::InspectionRange)?;
            if reject_devices
                && Option::<MemRegion>::from(PhysicalAddress::new(physical_u32))
                    .is_some_and(MemRegion::is_device)
            {
                return Err(BackendError::VirtualInspectionDevice(physical_u32));
            }
            self.engine
                .mem_read(physical_address, &mut bytes[offset..offset + count])
                .map_err(|error| {
                    warn!(
                        virtual_address,
                        physical_address,
                        length = count,
                        access,
                        error = ?error,
                        "Unicorn translated physical inspection read failed"
                    );
                    BackendError::Unicorn(error)
                })?;
            offset += count;
        }
        Ok(bytes)
    }

    /// Captures the authoritative physical bytes currently mapped by Unicorn.
    pub fn inspect_live_memory(&self, range: PhysicalRange) -> Result<Vec<u8>> {
        debug!(
            physical_address = range.start().get(),
            length = range.length(),
            "capturing Unicorn live-memory inspection snapshot"
        );
        self.read_physical_memory(range.start(), range.length() as usize)
    }

    /// Searches the authoritative physical RAM mapping in bounded, overlapping chunks.
    pub fn search_memory(&self, pattern: &[u8]) -> Result<Option<PhysicalAddress>> {
        if pattern.is_empty() {
            warn!("cannot search Unicorn RAM for an empty pattern");
            return Err(BackendError::EmptySearchPattern);
        }
        if pattern.len() > MEMORY_SEARCH_CHUNK_SIZE {
            warn!(
                pattern_length = pattern.len(),
                maximum = MEMORY_SEARCH_CHUNK_SIZE,
                "Unicorn RAM search pattern is too large"
            );
            return Err(BackendError::SearchPatternTooLarge {
                length: pattern.len(),
                maximum: MEMORY_SEARCH_CHUNK_SIZE,
            });
        }

        let ram = MemRegion::Ram.range();
        let ram_length = ram.length() as usize;
        let overlap = pattern.len() - 1;
        debug!(
            physical_address = ram.start().get(),
            length = ram.length(),
            pattern_length = pattern.len(),
            chunk_size = MEMORY_SEARCH_CHUNK_SIZE,
            "searching authoritative Unicorn RAM"
        );

        for offset in (0..ram_length).step_by(MEMORY_SEARCH_CHUNK_SIZE) {
            let length = (MEMORY_SEARCH_CHUNK_SIZE + overlap).min(ram_length - offset);
            let address = PhysicalAddress::new(ram.start().get() + offset as u32);
            let bytes = self.read_physical_memory(address, length)?;
            if let Some(index) = bytes
                .windows(pattern.len())
                .position(|window| window == pattern)
            {
                let address = PhysicalAddress::new(address.get() + index as u32);
                debug!(
                    physical_address = address.get(),
                    pattern_length = pattern.len(),
                    "found byte pattern in Unicorn RAM"
                );
                return Ok(Some(address));
            }
        }

        debug!(
            pattern_length = pattern.len(),
            "byte pattern was not found in Unicorn RAM"
        );
        Ok(None)
    }

    /// Captures CPU state and instruction bytes at one execution boundary.
    pub fn inspect_execution(
        &mut self,
        address: Option<VirtualAddress>,
        before: usize,
        after: usize,
    ) -> Result<ExecutionInspection> {
        let cpu = self.cpu_state().map_err(|error| {
            warn!(before, after, error = %error, "Unicorn CPU inspection failed");
            error
        })?;
        let mmu_enabled = self.machine().mmu.enabled();
        let start = address
            .map(VirtualAddress::get)
            .unwrap_or(cpu.registers[15])
            .saturating_sub(before as u32);
        let Some(length) = before.checked_add(after) else {
            warn!(
                pc = cpu.registers[15],
                cpsr = cpu.cpsr,
                before,
                after,
                "Unicorn execution inspection range overflow"
            );
            return Err(BackendError::InspectionRange);
        };
        let span = debug_span!(
            "unicorn_execution_inspection",
            pc = cpu.registers[15],
            cpsr = cpu.cpsr,
            virtual_address = start,
            length,
            access = "execute",
        );
        let _entered = span.enter();
        debug!("capturing Unicorn execution inspection snapshot");
        let (instruction_bytes, instruction_error) =
            match self.read_virtual_memory(VirtualAddress::new(start), length) {
                Ok(bytes) => (bytes, None),
                Err(error) => {
                    warn!(error = %error, "execution inspection has no readable instruction bytes");
                    (Vec::new(), Some(error.to_string()))
                }
            };
        Ok(ExecutionInspection {
            registers: cpu.registers,
            cpsr: cpu.cpsr,
            spsr: cpu.spsr,
            mmu_enabled,
            instruction_address: VirtualAddress::new(start),
            instruction_bytes,
            instruction_error,
        })
    }

    /// Writes an ARM register outside active emulation.
    pub fn set_register(&mut self, register: RegisterARM, value: u32) -> Result<()> {
        self.engine
            .reg_write(register, u64::from(value))
            .map_err(BackendError::Unicorn)
    }

    /// Returns the CP15 VBAR value maintained by the core MMU state.
    pub fn vector_base(&self) -> u32 {
        self.engine.get_data().machine.mmu.vector_base().get()
    }

    fn finish_cp15(&mut self, pending: PendingCp15) -> BackendStop {
        match pending {
            PendingCp15::Operation(operation) => {
                let flush_tlb = matches!(operation, minemu_platform::Cp15Operation::InvalidateAll);
                self.machine_mut().mmu.apply_cp15(operation);
                if flush_tlb && self.engine.ctl_flush_tlb().is_err() {
                    return BackendStop::Unicorn(uc_error::ARG);
                }
                BackendStop::Cp15Boundary
            }
            PendingCp15::ReadFaultStatus(register) => {
                let value = self
                    .machine()
                    .mmu
                    .last_fault()
                    .map(|fault| fault.status.raw())
                    .unwrap_or(0);
                if self.set_register(register, value).is_err() {
                    return BackendStop::Unicorn(uc_error::ARG);
                }
                BackendStop::Cp15Boundary
            }
            PendingCp15::ReadFaultAddress(register) => {
                let value = self
                    .machine()
                    .mmu
                    .last_fault()
                    .map(|fault| fault.address.get())
                    .unwrap_or(0);
                if self.set_register(register, value).is_err() {
                    return BackendStop::Unicorn(uc_error::ARG);
                }
                BackendStop::Cp15Boundary
            }
            PendingCp15::Undefined { pc } => {
                let plan = self.machine_mut().finish_instruction(
                    InstructionOutcome::SynchronousException(
                        minemu_platform::ExceptionKind::Undefined,
                        minemu_platform::VirtualAddress::new(pc),
                    ),
                );
                if let Some(plan) = plan
                    && self.enter_exception(plan).is_err()
                {
                    return BackendStop::Unicorn(uc_error::ARG);
                }
                BackendStop::Exception(minemu_platform::ExceptionKind::Undefined)
            }
        }
    }

    fn finish_callback_stop(&mut self, stop: BackendStop) -> BackendStop {
        match stop {
            BackendStop::MmuFault(fault) => {
                self.finish_pending_exception(PendingException::Fault(fault))
            }
            BackendStop::MmioFault {
                address, access, ..
            } => {
                let user_mode = self
                    .register(RegisterARM::CPSR)
                    .is_ok_and(|cpsr| cpsr & 0x1f == 0x10);
                let fault = self.machine_mut().mmu.record_invalid_mmio_fault(
                    VirtualAddress::new(address),
                    access,
                    user_mode,
                );
                self.finish_pending_exception(PendingException::Fault(fault))
            }
            stop => stop,
        }
    }

    fn finish_pending_exception(&mut self, exception: PendingException) -> BackendStop {
        let plan = match exception {
            PendingException::Synchronous { kind, pc } => {
                self.machine_mut()
                    .finish_instruction(InstructionOutcome::SynchronousException(
                        kind,
                        VirtualAddress::new(pc),
                    ))
            }
            PendingException::Fault(fault) => {
                let pc = self.program_counter().unwrap_or(fault.address.get());
                trace!(
                    address = fault.address.get(),
                    pc,
                    status = fault.status.raw(),
                    "entering exception for MMU or MMIO fault"
                );
                self.machine_mut()
                    .finish_instruction(InstructionOutcome::Fault(fault, VirtualAddress::new(pc)))
            }
        };
        let Some(plan) = plan else {
            return BackendStop::Unicorn(uc_error::ARG);
        };
        let kind = plan.request.kind;
        if self.enter_exception(plan).is_err() {
            return BackendStop::Unicorn(uc_error::ARG);
        }
        BackendStop::Exception(kind)
    }

    fn deliver_irq(&mut self) -> Option<BackendStop> {
        let cpsr = self.engine.reg_read(RegisterARM::CPSR).ok()? as u32;
        if cpsr & (1 << 7) != 0 {
            return None;
        }
        let _source = self.machine_mut().bus.interrupts.claim()?;
        let pc = self.engine.reg_read(RegisterARM::PC).ok()? as u32;
        let plan = self.machine_mut().enter_exception(
            minemu_platform::ExceptionKind::Interrupt,
            VirtualAddress::new(pc),
        );
        if self.enter_exception(plan).is_err() {
            return Some(BackendStop::Unicorn(uc_error::ARG));
        }
        Some(BackendStop::Exception(
            minemu_platform::ExceptionKind::Interrupt,
        ))
    }

    /// Applies the documented A32 state transition for an exception plan.
    pub fn enter_exception(&mut self, plan: ExceptionPlan) -> Result<()> {
        let previous_cpsr = self
            .engine
            .reg_read(RegisterARM::CPSR)
            .map_err(BackendError::Unicorn)? as u32;
        let mode = match plan.request.kind.destination_mode() {
            minemu_platform::CpuMode::Supervisor => 0x13,
            minemu_platform::CpuMode::Interrupt => 0x12,
            minemu_platform::CpuMode::Abort => 0x17,
            minemu_platform::CpuMode::Undefined => 0x1b,
            minemu_platform::CpuMode::User => unreachable!("exceptions never enter USR mode"),
        };
        let link_offset = match plan.request.kind {
            minemu_platform::ExceptionKind::DataAbort => 8,
            _ => 4,
        };
        let destination_cpsr = (previous_cpsr & !0x3f) | mode | (1 << 7);
        self.engine
            .reg_write(RegisterARM::CPSR, u64::from(destination_cpsr))
            .map_err(BackendError::Unicorn)?;
        self.engine
            .reg_write(RegisterARM::SPSR, u64::from(previous_cpsr))
            .map_err(BackendError::Unicorn)?;
        self.engine
            .reg_write(
                RegisterARM::LR,
                u64::from(plan.request.pc.get() + link_offset),
            )
            .map_err(BackendError::Unicorn)?;
        let vector = self.vector_base() + plan.request.kind.vector_offset();
        self.engine
            .reg_write(RegisterARM::PC, u64::from(vector))
            .map_err(BackendError::Unicorn)
    }

    fn map_bytes(&mut self, region: MemRegion, permissions: Prot, bytes: &[u8]) -> Result<()> {
        self.engine
            .mem_map(
                u64::from(region.base().get()),
                u64::from(region.size()),
                permissions,
            )
            .map_err(BackendError::Unicorn)?;
        self.engine
            .mem_write(u64::from(region.base().get()), bytes)
            .map_err(BackendError::Unicorn)
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
        }
        .map_err(BackendError::Unicorn)
    }

    fn map_mmio_window(&mut self) -> Result<()> {
        const MMIO_BASE: u64 = 0x1000_0000;
        const MMIO_SIZE: u64 = 0x1_0000;
        self.engine
            .mmio_map(
                MMIO_BASE,
                MMIO_SIZE,
                Some(hooks::mmio_read_callback),
                Some(hooks::mmio_write_callback),
            )
            .map_err(BackendError::Unicorn)
    }
}

/// All Unicorn callback registration is centralized here so the backend's
/// emulator-visible behavior is auditable without searching construction code.
mod hooks {
    use super::*;

    /// Registers virtual translation, instruction accounting, synchronous A32
    /// trap interception, and invalid-instruction callbacks.
    pub(super) fn register(engine: &mut Unicorn<'static, BackendData>) -> Result<()> {
        engine
            .ctl_set_tlb_type(TlbType::VIRTUAL)
            .map_err(BackendError::Unicorn)?;
        engine
            .add_tlb_hook(1, 0, virtual_tlb_callback)
            .map(|_| ())
            .map_err(BackendError::Unicorn)?;
        engine
            .add_code_hook(1, 0, instruction_counter_callback)
            .map(|_| ())
            .map_err(BackendError::Unicorn)?;
        engine
            .add_code_hook(1, 0, a32_synchronous_trap_callback)
            .map(|_| ())
            .map_err(BackendError::Unicorn)?;
        engine
            .add_insn_invalid_hook(invalid_instruction_callback)
            .map(|_| ())
            .map_err(BackendError::Unicorn)
    }

    fn virtual_tlb_callback(
        engine: &mut Unicorn<BackendData>,
        address: u64,
        memory_type: unicorn_engine::unicorn_const::MemType,
    ) -> Option<TlbEntry> {
        let access = arm::mmu_access(memory_type)?;
        let user_mode = engine.reg_read(RegisterARM::CPSR).ok()? as u32 & 0x1f == 0x10;
        let data = engine.get_data_mut();
        let BackendData {
            machine,
            callback_stop,
            ..
        } = data;
        match machine.mmu.translate(
            &mut machine.memory,
            VirtualAddress::new(address as u32),
            access,
            user_mode,
        ) {
            Ok(physical) => Some(TlbEntry {
                paddr: u64::from(physical.get()),
                perms: match access {
                    Access::Fetch => Prot::EXEC,
                    Access::Read => Prot::READ,
                    Access::Write => Prot::WRITE,
                },
            }),
            Err(fault) => {
                *callback_stop = Some(BackendStop::MmuFault(fault));
                None
            }
        }
    }

    fn instruction_counter_callback(engine: &mut Unicorn<BackendData>, _: u64, _: u32) {
        engine.get_data_mut().executed_instructions += 1;
    }

    fn a32_synchronous_trap_callback(engine: &mut Unicorn<BackendData>, address: u64, size: u32) {
        if size != 4 {
            return;
        }
        let mut bytes = [0; 4];
        if engine.vmem_read(address, Prot::EXEC, &mut bytes).is_err() {
            return;
        }
        let instruction = u32::from_le_bytes(bytes);
        if instruction & 0x0f00_0000 == 0x0f00_0000
            && arm::condition_holds(
                instruction >> 28,
                engine.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32,
            )
        {
            engine.get_data_mut().pending_exception = Some(PendingException::Synchronous {
                kind: minemu_platform::ExceptionKind::SupervisorCall,
                pc: address as u32,
            });
            let _ = engine.reg_write(RegisterARM::PC, address + 4);
            let _ = engine.emu_stop();
            return;
        }
        let Some(cp15) = arm::decode_cp15(instruction) else {
            return;
        };
        let cpsr = engine.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
        let pending = if !arm::condition_holds(cp15.condition, cpsr) {
            None
        } else if !arm::cp15_is_privileged(cpsr) {
            Some(PendingCp15::Undefined { pc: address as u32 })
        } else {
            arm::cp15_pending(engine, cp15, address as u32)
        };
        if let Some(pending) = pending {
            engine.get_data_mut().pending_cp15 = Some(pending);
        }
        let _ = engine.reg_write(RegisterARM::PC, address + 4);
        let _ = engine.emu_stop();
    }

    fn invalid_instruction_callback(engine: &mut Unicorn<BackendData>) -> bool {
        let pc = engine.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
        engine.get_data_mut().pending_exception = Some(PendingException::Synchronous {
            kind: minemu_platform::ExceptionKind::Undefined,
            pc,
        });
        let _ = engine.reg_write(RegisterARM::PC, u64::from(pc + 4));
        let _ = engine.emu_stop();
        true
    }

    pub(super) fn mmio_read_callback(
        engine: &mut Unicorn<BackendData>,
        offset: u64,
        size: usize,
    ) -> u64 {
        const MMIO_BASE: u64 = 0x1000_0000;
        let address = MMIO_BASE + offset;
        let Ok(width) = MmioWidth::try_from(size) else {
            record_mmio_fault(
                engine,
                address,
                Access::Read,
                format!("unsupported read width {size}"),
            );
            return 0;
        };
        let transaction = MmioTransaction::read(PhysicalAddress::new(address as u32), width);
        let now = engine.get_data().machine.ticks();
        match engine.get_data_mut().machine.bus.access(transaction, now) {
            Ok(Some(value)) => u64::from(value),
            Ok(None) => 0,
            Err(error) => {
                record_mmio_fault(engine, address, Access::Read, error.to_string());
                0
            }
        }
    }

    pub(super) fn mmio_write_callback(
        engine: &mut Unicorn<BackendData>,
        offset: u64,
        size: usize,
        value: u64,
    ) {
        const MMIO_BASE: u64 = 0x1000_0000;
        let address = MMIO_BASE + offset;
        let Ok(width) = MmioWidth::try_from(size) else {
            record_mmio_fault(
                engine,
                address,
                Access::Write,
                format!("unsupported write width {size}"),
            );
            return;
        };
        let transaction =
            MmioTransaction::write(PhysicalAddress::new(address as u32), width, value as u32);
        let now = engine.get_data().machine.ticks();
        if let Err(error) = engine.get_data_mut().machine.bus.access(transaction, now) {
            record_mmio_fault(engine, address, Access::Write, error.to_string());
        }
    }

    fn record_mmio_fault(
        engine: &mut Unicorn<BackendData>,
        address: u64,
        access: Access,
        detail: String,
    ) {
        engine.get_data_mut().callback_stop = Some(BackendStop::MmioFault {
            address: address as u32,
            access,
            detail,
        });
        let _ = engine.emu_stop();
    }
}

#[cfg(test)]
mod tests {
    use minemu_core::{InterruptUpdate, Machine, PhysicalMemoryAccess, UartUpdate};
    use minemu_platform::{
        FaultCause, MemRegion, PTE_DIRTY, PTE_EXECUTABLE, PTE_READABLE, PTE_VALID, PTE_WRITABLE,
        Peripheral, PhysicalAddress, PhysicalRange, VirtualAddress,
        peripherals::{interrupt, uart},
    };
    use unicorn_engine::RegisterARM;

    use super::{BackendError, BackendStop, MEMORY_SEARCH_CHUNK_SIZE, UnicornBackend};

    fn read_then_write_machine(data_permissions: u32) -> (Machine, PhysicalAddress) {
        let mut machine = Machine::default();
        let ram = MemRegion::Ram.base().get();
        let table = ram + 0x1000;
        let code = ram + 0x2000;
        let data = ram + 0x3000;
        machine
            .memory
            .write_u32(PhysicalAddress::new(ram), table | PTE_VALID)
            .unwrap();
        machine
            .memory
            .write_u32(
                PhysicalAddress::new(table),
                code | PTE_VALID | PTE_READABLE | PTE_EXECUTABLE,
            )
            .unwrap();
        let data_pte = PhysicalAddress::new(table + 4);
        machine
            .memory
            .write_u32(data_pte, data | PTE_VALID | PTE_READABLE | data_permissions)
            .unwrap();
        machine
            .memory
            .write_range(
                PhysicalAddress::new(code),
                &[
                    0x00, 0x00, 0x91, 0xe5, // ldr r0, [r1]
                    0x00, 0x00, 0x81, 0xe5, // str r0, [r1]
                ],
            )
            .unwrap();
        machine
            .memory
            .write_u32(PhysicalAddress::new(data), 0x1234_5678)
            .unwrap();
        machine.mmu.set_ttbr0(PhysicalAddress::new(ram));
        machine.mmu.set_enabled(true);
        (machine, data_pte)
    }

    #[test]
    fn cortex_a9_reset_state_is_privileged_a32_at_zero() {
        let backend = UnicornBackend::new(Machine::default()).unwrap();
        let state = backend.cpu_state().unwrap();
        assert_eq!(state.registers[15], 0);
        assert_eq!(state.cpsr & 0x1f, 0x13);
        assert_eq!(state.cpsr & (1 << 5), 0);
        assert_ne!(state.cpsr & (1 << 7), 0);
        assert_ne!(state.cpsr & (1 << 6), 0);
    }

    #[test]
    fn cortex_a9_executes_one_a32_instruction() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        machine
            .memory
            .write_range(PhysicalAddress::new(start), &[1, 0, 0xa0, 0xe3])
            .unwrap(); // mov r0, #1
        let mut backend = UnicornBackend::new(machine).unwrap();
        assert_eq!(
            backend.run(start, start + 4, 1),
            BackendStop::InstructionBudget
        );
        assert_eq!(backend.register(RegisterARM::R0).unwrap(), 1);
        assert_eq!(backend.machine().ticks(), 1);
    }

    #[test]
    fn inspection_snapshots_come_from_the_unicorn_mapping() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        let instruction = [1, 0, 0xa0, 0xe3]; // mov r0, #1
        machine
            .memory
            .write_range(PhysicalAddress::new(start), &instruction)
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_program_counter(start).unwrap();

        assert_eq!(
            backend
                .inspect_live_memory(PhysicalRange::new(PhysicalAddress::new(start), 4).unwrap())
                .unwrap(),
            instruction
        );
        let execution = backend.inspect_execution(None, 0, 4).unwrap();
        assert!(!execution.mmu_enabled);
        assert_eq!(execution.instruction_address.get(), start);
        assert_eq!(execution.instruction_bytes, instruction);
    }

    #[test]
    fn cpu_and_core_share_one_ram_backing() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        let target = start + 0x1000;
        let value = 0x4433_2211_u32;
        let mut code = vec![
            0x04, 0x00, 0x9f, 0xe5, // ldr r0, [pc, #4]
            0x04, 0x10, 0x9f, 0xe5, // ldr r1, [pc, #4]
            0x00, 0x10, 0x80, 0xe5, // str r1, [r0]
        ];
        code.extend_from_slice(&target.to_le_bytes());
        code.extend_from_slice(&value.to_le_bytes());
        machine
            .memory
            .write_range(PhysicalAddress::new(start), &code)
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();

        assert_eq!(
            backend.run(start, start + 12, 3),
            BackendStop::InstructionBudget
        );
        assert_eq!(
            backend
                .machine()
                .memory
                .read_u32(PhysicalAddress::new(target))
                .unwrap(),
            value
        );

        let replacement = 0xaabb_ccdd_u32;
        backend
            .machine_mut()
            .memory
            .write_u32(PhysicalAddress::new(target), replacement)
            .unwrap();
        assert_eq!(
            backend
                .read_physical_memory(PhysicalAddress::new(target), 4)
                .unwrap(),
            replacement.to_le_bytes()
        );
    }

    #[test]
    fn memory_search_finds_a_match_crossing_a_chunk_boundary() {
        let mut machine = Machine::default();
        let pattern = [0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe];
        let address = MemRegion::Ram.base().get() + MEMORY_SEARCH_CHUNK_SIZE as u32 - 3;
        machine
            .memory
            .write_range(PhysicalAddress::new(address), &pattern)
            .unwrap();
        let backend = UnicornBackend::new(machine).unwrap();

        assert_eq!(
            backend.search_memory(&pattern).unwrap(),
            Some(PhysicalAddress::new(address))
        );
    }

    #[test]
    fn memory_search_reports_no_match() {
        let backend = UnicornBackend::new(Machine::default()).unwrap();

        assert_eq!(
            backend
                .search_memory(&[0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe])
                .unwrap(),
            None
        );
    }

    #[test]
    fn memory_search_rejects_an_empty_pattern() {
        let backend = UnicornBackend::new(Machine::default()).unwrap();

        assert!(matches!(
            backend.search_memory(&[]),
            Err(BackendError::EmptySearchPattern)
        ));
    }

    #[test]
    fn virtual_tlb_delegates_to_the_core_mmu() {
        let mut machine = Machine::default();
        let ram = MemRegion::Ram.base().get();
        let target = ram + 0x3000;
        machine
            .memory
            .write_u32(PhysicalAddress::new(ram), (ram + 0x1000) | PTE_VALID)
            .unwrap();
        machine
            .memory
            .write_u32(
                PhysicalAddress::new(ram + 0x1000),
                target | PTE_VALID | PTE_READABLE | PTE_EXECUTABLE,
            )
            .unwrap();
        machine
            .memory
            .write_range(PhysicalAddress::new(target), &[7, 0, 0xa0, 0xe3])
            .unwrap(); // mov r0, #7
        machine.mmu.set_ttbr0(PhysicalAddress::new(ram));
        machine.mmu.set_enabled(true);
        let mut backend = UnicornBackend::new(machine).unwrap();
        assert_eq!(backend.run(0, 4, 1), BackendStop::InstructionBudget);
        assert_eq!(backend.register(RegisterARM::R0).unwrap(), 7);
        assert_eq!(backend.machine().mmu.last_fault(), None);
        let inspection = backend.inspect_execution(None, 0, 4).unwrap();
        assert_eq!(inspection.instruction_address.get(), 4);
        assert_eq!(inspection.instruction_bytes, [0; 4]);
        assert_eq!(inspection.instruction_error, None);
    }

    #[test]
    fn virtual_tlb_enforces_supervisor_only_mapping_in_user_mode() {
        let mut machine = Machine::default();
        let ram = MemRegion::Ram.base().get();
        let target = ram + 0x3000;
        machine
            .memory
            .write_u32(PhysicalAddress::new(ram), (ram + 0x1000) | PTE_VALID)
            .unwrap();
        machine
            .memory
            .write_u32(
                PhysicalAddress::new(ram + 0x1000),
                target | PTE_VALID | PTE_READABLE | PTE_EXECUTABLE,
            )
            .unwrap();
        machine
            .memory
            .write_range(PhysicalAddress::new(target), &[0, 0xf0, 0x20, 0xe3])
            .unwrap(); // nop
        machine.mmu.set_ttbr0(PhysicalAddress::new(ram));
        machine.mmu.set_enabled(true);
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::CPSR, 0x10).unwrap();

        assert_eq!(
            backend.run(0, 4, 1),
            BackendStop::Exception(minemu_platform::ExceptionKind::PrefetchAbort)
        );
        let fault = backend.machine().mmu.last_fault().unwrap();
        assert_eq!(fault.status.cause(), Some(FaultCause::ExecuteProtection));
        assert!(fault.status.from_user());
    }

    #[test]
    fn read_tlb_entry_does_not_bypass_later_write_protection() {
        let (machine, data_pte) = read_then_write_machine(0);
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::R1, 0x1000).unwrap();

        assert_eq!(
            backend.run(0, 8, 2),
            BackendStop::Exception(minemu_platform::ExceptionKind::DataAbort)
        );
        let fault = backend.machine().mmu.last_fault().unwrap();
        assert_eq!(fault.status.cause(), Some(FaultCause::WriteProtection));
        assert_eq!(
            backend.machine().memory.read_u32(data_pte).unwrap() & PTE_DIRTY,
            0
        );
    }

    #[test]
    fn write_after_read_reenters_translation_and_sets_dirty() {
        let (machine, data_pte) = read_then_write_machine(PTE_WRITABLE);
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::R1, 0x1000).unwrap();

        assert_eq!(backend.run(0, 8, 2), BackendStop::InstructionBudget);
        assert_ne!(
            backend.machine().memory.read_u32(data_pte).unwrap() & PTE_DIRTY,
            0
        );
    }

    #[test]
    fn higher_half_execution_inspection_reads_translated_instruction_bytes() {
        let mut machine = Machine::default();
        let ram = MemRegion::Ram.base().get();
        let virtual_page = 0xc003_0000;
        let physical_page = ram + 0x30000;
        let next_physical_page = ram + 0x50000;
        let directory_index = virtual_page >> 22;
        let table_index = (virtual_page >> 12) & 0x3ff;
        machine
            .memory
            .write_u32(
                PhysicalAddress::new(ram + directory_index * 4),
                (ram + 0x1000) | PTE_VALID,
            )
            .unwrap();
        machine
            .memory
            .write_u32(
                PhysicalAddress::new(ram + 0x1000 + table_index * 4),
                physical_page | PTE_VALID | PTE_READABLE | PTE_EXECUTABLE,
            )
            .unwrap();
        machine
            .memory
            .write_u32(
                PhysicalAddress::new(ram + 0x1000 + (table_index + 1) * 4),
                next_physical_page | PTE_VALID | PTE_READABLE | PTE_EXECUTABLE,
            )
            .unwrap();
        machine
            .memory
            .write_u32(
                PhysicalAddress::new(ram + 0x1000 + (table_index + 2) * 4),
                MemRegion::Uart0.base().get() | PTE_VALID | PTE_READABLE,
            )
            .unwrap();
        let instructions = [
            0x00, 0xf0, 0x20, 0xe3, // nop
            0xfd, 0xff, 0xff, 0xea, // b 0xc0030260
        ];
        machine
            .memory
            .write_range(PhysicalAddress::new(physical_page + 0x260), &instructions)
            .unwrap();
        machine
            .memory
            .write_range(PhysicalAddress::new(physical_page + 0xffc), &[1, 2, 3, 4])
            .unwrap();
        machine
            .memory
            .write_range(PhysicalAddress::new(next_physical_page), &[5, 6, 7, 8])
            .unwrap();
        machine.mmu.set_ttbr0(PhysicalAddress::new(ram));
        machine.mmu.set_enabled(true);
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_program_counter(0xc003_0264).unwrap();

        let inspection = backend.inspect_execution(None, 4, 4).unwrap();
        assert!(inspection.mmu_enabled);
        assert_eq!(inspection.instruction_address.get(), 0xc003_0260);
        assert_eq!(inspection.instruction_bytes, instructions);
        assert_eq!(inspection.instruction_error, None);
        assert_eq!(
            backend
                .translate_virtual_address(VirtualAddress::new(virtual_page + 0xffc))
                .unwrap(),
            PhysicalAddress::new(physical_page + 0xffc)
        );
        assert_eq!(
            backend
                .inspect_virtual_memory(VirtualAddress::new(virtual_page + 0xffc), 8)
                .unwrap(),
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            backend
                .read_virtual_memory(VirtualAddress::new(virtual_page + 0xffc), 8)
                .unwrap(),
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert!(matches!(
            backend.inspect_virtual_memory(VirtualAddress::new(virtual_page + 0x2000), 1),
            Err(BackendError::VirtualInspectionDevice(address))
                if address == MemRegion::Uart0.base().get()
        ));
    }

    #[test]
    fn enabled_mmu_inspection_retains_cpu_state_when_instruction_bytes_are_unmapped() {
        let mut machine = Machine::default();
        machine.mmu.set_enabled(true);
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_program_counter(0x2000).unwrap();

        let inspection = backend.inspect_execution(None, 0, 4).unwrap();
        assert_eq!(inspection.registers[15], 0x2000);
        assert!(inspection.instruction_bytes.is_empty());
        assert!(inspection.instruction_error.is_some());
    }

    #[test]
    fn guest_mmio_load_routes_through_the_typed_core_bus() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        machine
            .memory
            .write_range(
                PhysicalAddress::new(start),
                &[
                    0x00, 0x10, 0x9f, 0xe5, // ldr r1, [pc]
                    0x00, 0x00, 0x91, 0xe5, // ldr r0, [r1]
                    0x04, 0x30, 0x00, 0x10, // .word RNG DATA
                ],
            )
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();
        assert_eq!(
            backend.run(start, start + 8, 2),
            BackendStop::InstructionBudget
        );
        assert_eq!(backend.register(RegisterARM::R0).unwrap(), 0x791c_7b62);
        assert_eq!(backend.machine().ticks(), 2);
    }

    #[test]
    fn invalid_mmio_load_updates_fault_status_and_address() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        let invalid_address = MemRegion::Rng.base().get() + 0x0c;
        let mut code = vec![
            0x00, 0x10, 0x9f, 0xe5, // ldr r1, [pc]
            0x00, 0x00, 0x91, 0xe5, // ldr r0, [r1]
        ];
        code.extend_from_slice(&invalid_address.to_le_bytes());
        machine
            .memory
            .write_range(PhysicalAddress::new(start), &code)
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();

        assert_eq!(
            backend.run(start, start + 8, 2),
            BackendStop::Exception(minemu_platform::ExceptionKind::DataAbort)
        );
        let fault = backend.machine().mmu.last_fault().unwrap();
        assert_eq!(fault.address, VirtualAddress::new(invalid_address));
        assert_eq!(fault.status.cause(), Some(FaultCause::DeviceAccess));
        assert!(!fault.status.is_write());
    }

    #[test]
    fn cp15_boundary_and_exception_entry_are_explicit() {
        let mut backend = UnicornBackend::new(Machine::default()).unwrap();
        backend
            .machine_mut()
            .mmu
            .apply_cp15(minemu_platform::Cp15Operation::SetVectorBase(
                minemu_platform::VirtualAddress::new(0x4000_8000),
            ));
        backend.set_register(RegisterARM::CPSR, 0x10).unwrap(); // USR
        let plan = minemu_core::ExceptionPlan::synchronous(
            minemu_platform::ExceptionKind::SupervisorCall,
            minemu_platform::VirtualAddress::new(0x1000),
        );
        backend.enter_exception(plan).unwrap();
        assert_eq!(backend.vector_base(), 0x4000_8000);
        assert_eq!(backend.register(RegisterARM::PC).unwrap(), 0x4000_8008);
        assert_eq!(backend.register(RegisterARM::LR).unwrap(), 0x1004);
        assert_eq!(backend.register(RegisterARM::CPSR).unwrap() & 0x1f, 0x13);
    }

    #[test]
    fn guest_cp15_mcr_is_condition_checked_and_applied_at_a_boundary() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        machine
            .memory
            .write_range(
                PhysicalAddress::new(start),
                &[
                    1, 0, 0xa0, 0xe3, // mov r0, #1
                    0x10, 0x0f, 0x01, 0xee, // mcr p15, 0, r0, c1, c0, 0
                ],
            )
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::CPSR, 0x13).unwrap();
        assert_eq!(
            backend.run(start, start + 4, 1),
            BackendStop::InstructionBudget
        );
        assert_eq!(
            backend.run(start + 4, start + 8, 1),
            BackendStop::Cp15Boundary
        );
        assert!(backend.machine().mmu.enabled());
        assert_eq!(backend.machine().ticks(), 2);
    }

    #[test]
    fn guest_svc_enters_the_supervisor_vector_with_two_ticks() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        machine
            .memory
            .write_range(PhysicalAddress::new(start), &[0, 0, 0, 0xef])
            .unwrap(); // svc #0
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::CPSR, 0x13).unwrap();
        assert_eq!(
            backend.run(start, start + 4, 1),
            BackendStop::Exception(minemu_platform::ExceptionKind::SupervisorCall)
        );
        assert_eq!(backend.register(RegisterARM::PC).unwrap(), 8);
        assert_eq!(backend.machine().ticks(), 2);
    }

    #[test]
    fn unprivileged_cp15_enters_the_undefined_vector() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        machine
            .memory
            .write_range(
                PhysicalAddress::new(start),
                &[0x10, 0x0f, 0x01, 0xee], // mcr p15, 0, r0, c1, c0, 0
            )
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::CPSR, 0x10).unwrap();
        assert_eq!(
            backend.run(start, start + 4, 1),
            BackendStop::Exception(minemu_platform::ExceptionKind::Undefined)
        );
        assert_eq!(backend.register(RegisterARM::PC).unwrap(), 4);
        assert_eq!(backend.machine().ticks(), 2);
    }

    #[test]
    fn prefetch_and_data_faults_enter_the_correct_abort_vectors() {
        let mut machine = Machine::default();
        let ram = MemRegion::Ram.base().get();
        machine.mmu.set_ttbr0(PhysicalAddress::new(ram));
        machine.mmu.set_enabled(true);
        let mut backend = UnicornBackend::new(machine).unwrap();
        assert_eq!(
            backend.run(0, 4, 1),
            BackendStop::Exception(minemu_platform::ExceptionKind::PrefetchAbort)
        );
        assert_eq!(backend.register(RegisterARM::PC).unwrap(), 12);
        assert_eq!(backend.machine().ticks(), 1);

        let mut machine = Machine::default();
        let target = ram + 0x3000;
        machine
            .memory
            .write_u32(PhysicalAddress::new(ram), (ram + 0x1000) | PTE_VALID)
            .unwrap();
        machine
            .memory
            .write_u32(
                PhysicalAddress::new(ram + 0x1000),
                target | PTE_VALID | PTE_READABLE | PTE_EXECUTABLE,
            )
            .unwrap();
        machine
            .memory
            .write_range(PhysicalAddress::new(target), &[0, 0, 0x91, 0xe5])
            .unwrap(); // ldr r0, [r1]
        machine.mmu.set_ttbr0(PhysicalAddress::new(ram));
        machine.mmu.set_enabled(true);
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::R1, 0x1000).unwrap();
        assert_eq!(
            backend.run(0, 4, 1),
            BackendStop::Exception(minemu_platform::ExceptionKind::DataAbort)
        );
        assert_eq!(backend.register(RegisterARM::PC).unwrap(), 16);
        assert_eq!(backend.register(RegisterARM::LR).unwrap(), 8);
        assert_eq!(backend.machine().ticks(), 1);
    }

    #[test]
    fn enabled_pending_uart_delivers_an_irq_at_the_instruction_boundary() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        machine
            .memory
            .write_range(PhysicalAddress::new(start), &[0, 0xf0, 0x20, 0xe3])
            .unwrap(); // nop
        machine
            .bus
            .interrupts
            .update(InterruptUpdate::Write {
                register: interrupt::Register::Enable,
                value: interrupt::Source::Uart0.bit(),
            })
            .unwrap();
        machine.bus.uart0.update(UartUpdate::Receive(b'x')).unwrap();
        machine
            .bus
            .uart0
            .update(UartUpdate::Write {
                register: uart::Register::Control,
                value: 1,
            })
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::CPSR, 0x13).unwrap();
        assert_eq!(
            backend.run(start, start + 4, 1),
            BackendStop::Exception(minemu_platform::ExceptionKind::Interrupt)
        );
        assert_eq!(backend.register(RegisterARM::PC).unwrap(), 24);
        assert_eq!(backend.machine().ticks(), 2);
    }

    #[test]
    fn guest_cp15_mrc_reads_fault_registers_at_a_boundary() {
        let mut machine = Machine::default();
        let start = MemRegion::Ram.base().get();
        machine
            .memory
            .write_range(
                PhysicalAddress::new(start),
                &[0x10, 0x0f, 0x15, 0xee], // mrc p15, 0, r0, c5, c0, 0
            )
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::CPSR, 0x13).unwrap();
        assert_eq!(backend.run(start, start + 4, 1), BackendStop::Cp15Boundary);
        assert_eq!(backend.register(RegisterARM::R0).unwrap(), 0);
    }

    #[test]
    fn ttbr_switch_requires_and_honors_tlbiall() {
        let mut machine = Machine::default();
        let ram = MemRegion::Ram.base().get();
        let directory_a = ram;
        let table_a = ram + 0x1000;
        let directory_b = ram + 0x2000;
        let table_b = ram + 0x3000;
        let page_a = ram + 0x4000;
        let page_b = ram + 0x5000;
        for (directory, table, page) in [
            (directory_a, table_a, page_a),
            (directory_b, table_b, page_b),
        ] {
            machine
                .memory
                .write_u32(PhysicalAddress::new(directory), table | PTE_VALID)
                .unwrap();
            machine
                .memory
                .write_u32(
                    PhysicalAddress::new(table),
                    page | PTE_VALID | PTE_READABLE | PTE_EXECUTABLE,
                )
                .unwrap();
        }
        machine
            .memory
            .write_range(
                PhysicalAddress::new(page_a),
                &[
                    0x10, 0x0f, 0x02, 0xee, // mcr p15, 0, r0, c2, c0, 0
                    0x17, 0x0f, 0x08, 0xee, // mcr p15, 0, r0, c8, c7, 0
                    1, 0x10, 0xa0, 0xe3, // mov r1, #1 (must not execute)
                ],
            )
            .unwrap();
        machine
            .memory
            .write_range(PhysicalAddress::new(page_b + 8), &[2, 0x10, 0xa0, 0xe3])
            .unwrap(); // mov r1, #2
        machine.mmu.set_ttbr0(PhysicalAddress::new(directory_a));
        machine.mmu.set_enabled(true);
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend.set_register(RegisterARM::CPSR, 0x13).unwrap();
        backend.set_register(RegisterARM::R0, directory_b).unwrap();
        assert_eq!(backend.run(0, 4, 1), BackendStop::Cp15Boundary);
        assert_eq!(backend.run(4, 8, 1), BackendStop::Cp15Boundary);
        assert_eq!(backend.run(8, 12, 1), BackendStop::InstructionBudget);
        assert_eq!(backend.register(RegisterARM::R1).unwrap(), 2);
    }
}
