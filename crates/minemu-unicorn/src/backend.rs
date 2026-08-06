//! Unicorn-backed A32 execution adapter for the backend-independent machine core.

use minemu_core::{ExceptionPlan, InstructionOutcome, Machine, MmuFault};
use minemu_platform::{
    Access, FaultCause, FaultStatus, MemRegion, MmioTransaction, MmioWidth, PhysicalAddress,
    VirtualAddress,
};
use thiserror::Error;
use unicorn_engine::{
    ArmCpuModel, RegisterARM, Unicorn,
    unicorn_const::{Arch, Mode, Prot, TlbEntry, TlbType, uc_error},
};

use crate::arm::{self, PendingCp15};

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

/// Errors while constructing or operating the Unicorn backend.
#[derive(Debug, Error)]
pub enum BackendError {
    #[error(transparent)]
    Core(#[from] minemu_core::CoreError),
    #[error("Unicorn operation failed: {0:?}")]
    Unicorn(uc_error),
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
    pub fn new(machine: Machine) -> Result<Self> {
        let boot_rom = machine.copy_region(MemRegion::BootRom)?;
        let system_rom = machine.copy_region(MemRegion::SystemRom)?;
        let ram = machine.copy_region(MemRegion::Ram)?;

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

        let mut backend = Self { engine };
        backend.map_bytes(MemRegion::BootRom, Prot::READ | Prot::EXEC, &boot_rom)?;
        backend.map_bytes(MemRegion::SystemRom, Prot::READ | Prot::EXEC, &system_rom)?;
        backend.map_bytes(MemRegion::Ram, Prot::ALL, &ram)?;
        backend.map_mmio_window()?;
        hooks::register(&mut backend.engine)?;

        Ok(backend)
    }

    /// Runs no more than `instruction_budget` instructions from `start` toward `end`.
    pub fn run(&mut self, start: u32, end: u32, instruction_budget: usize) -> BackendStop {
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
            } => self.finish_pending_exception(PendingException::Fault(MmuFault {
                address: VirtualAddress::new(address),
                status: FaultStatus::new(FaultCause::DeviceAccess, false, access),
            })),
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
            PendingException::Fault(fault) => self
                .machine_mut()
                .finish_instruction(InstructionOutcome::Fault(fault)),
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
            false,
        ) {
            Ok(physical) => Some(TlbEntry {
                paddr: u64::from(physical.get()),
                perms: Prot::ALL,
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
        MemRegion, PTE_EXECUTABLE, PTE_READABLE, PTE_VALID, Peripheral, PhysicalAddress,
        peripherals::{interrupt, uart},
    };
    use unicorn_engine::RegisterARM;

    use super::{BackendStop, UnicornBackend};

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
