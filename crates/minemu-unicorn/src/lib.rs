//! Unicorn-backed A32 execution adapter for the backend-independent machine core.

use minemu_core::{ExceptionPlan, InstructionOutcome, Machine, MmuFault, PhysicalMemoryAccess};
use minemu_platform::{Access, MemRegion, MmioTransaction, MmioWidth, PhysicalAddress};
use thiserror::Error;
use unicorn_engine::{
    ArmCpuModel, RegisterARM, Unicorn,
    unicorn_const::{Arch, MemType, Mode, Prot, TlbEntry, TlbType, uc_error},
};

/// An explicit reason why a bounded backend run stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendStop {
    InstructionBudget,
    MmioFault { address: u32, detail: String },
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
    #[error("MMIO callback received unsupported access width {0}")]
    UnsupportedMmioWidth(usize),
}

type Result<T> = std::result::Result<T, BackendError>;

struct BackendData {
    machine: Machine,
    vector_base: u32,
    executed_instructions: u64,
    callback_stop: Option<BackendStop>,
    pending_cp15: Option<PendingCp15>,
}

enum PendingCp15 {
    Operation(minemu_platform::Cp15Operation),
    ReadFaultStatus(RegisterARM),
    ReadFaultAddress(RegisterARM),
    Undefined { pc: u32 },
}

/// Safe, single-threaded Unicorn A32 backend bound to one core machine.
pub struct UnicornBackend {
    engine: Unicorn<'static, BackendData>,
}

impl UnicornBackend {
    /// Configures an A32 little-endian Cortex-A9 with the ABI physical map.
    pub fn new(machine: Machine) -> Result<Self> {
        let boot_rom = copy_region(&machine, MemRegion::BootRom)?;
        let system_rom = copy_region(&machine, MemRegion::SystemRom)?;
        let ram = copy_region(&machine, MemRegion::Ram)?;

        let mut engine = Unicorn::new_with_data(
            Arch::ARM,
            Mode::ARM | Mode::LITTLE_ENDIAN,
            BackendData {
                machine,
                vector_base: 0,
                executed_instructions: 0,
                callback_stop: None,
                pending_cp15: None,
            },
        )
        .map_err(BackendError::Unicorn)?;
        engine
            .ctl_set_cpu_model(ArmCpuModel::CORTEX_A9 as i32)
            .map_err(BackendError::Unicorn)?;

        map_bytes(
            &mut engine,
            MemRegion::BootRom,
            Prot::READ | Prot::EXEC,
            &boot_rom,
        )?;
        map_bytes(
            &mut engine,
            MemRegion::SystemRom,
            Prot::READ | Prot::EXEC,
            &system_rom,
        )?;
        map_bytes(&mut engine, MemRegion::Ram, Prot::ALL, &ram)?;
        map_mmio(&mut engine, MemRegion::InterruptController)?;
        map_mmio(&mut engine, MemRegion::SysTick)?;
        map_mmio(&mut engine, MemRegion::Dma)?;
        map_mmio(&mut engine, MemRegion::Rng)?;
        map_mmio(&mut engine, MemRegion::Uart0)?;
        map_mmio(&mut engine, MemRegion::Uart1)?;
        map_mmio(&mut engine, MemRegion::Trace)?;
        engine
            .ctl_set_tlb_type(TlbType::VIRTUAL)
            .map_err(BackendError::Unicorn)?;
        engine
            .add_tlb_hook(1, 0, |engine, address, memory_type| {
                let access = mmu_access(memory_type)?;
                let data = engine.get_data_mut();
                let BackendData {
                    machine,
                    callback_stop,
                    ..
                } = data;
                match machine.mmu.translate(
                    &mut machine.memory,
                    minemu_platform::VirtualAddress::new(address as u32),
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
            })
            .map_err(BackendError::Unicorn)?;

        engine
            .add_code_hook(1, 0, |engine, _, _| {
                engine.get_data_mut().executed_instructions += 1;
            })
            .map_err(BackendError::Unicorn)?;
        engine
            .add_code_hook(1, 0, |engine, address, size| {
                if size != 4 {
                    return;
                }
                let mut bytes = [0; 4];
                if engine.vmem_read(address, Prot::EXEC, &mut bytes).is_err() {
                    return;
                }
                let instruction = u32::from_le_bytes(bytes);
                let Some(cp15) = decode_cp15(instruction) else {
                    return;
                };
                let cpsr = engine.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
                let pending = if !condition_holds(cp15.condition, cpsr) {
                    None
                } else if !cp15_is_privileged(cpsr) {
                    Some(PendingCp15::Undefined { pc: address as u32 })
                } else {
                    cp15_pending(engine, cp15, address as u32)
                };
                if let Some(pending) = pending {
                    engine.get_data_mut().pending_cp15 = Some(pending);
                }
                let _ = engine.reg_write(RegisterARM::PC, address + 4);
                let _ = engine.emu_stop();
            })
            .map_err(BackendError::Unicorn)?;

        Ok(Self { engine })
    }

    /// Runs no more than `instruction_budget` instructions from `start` toward `end`.
    pub fn run(&mut self, start: u32, end: u32, instruction_budget: usize) -> BackendStop {
        let data = self.engine.get_data_mut();
        data.executed_instructions = 0;
        data.callback_stop = None;
        data.pending_cp15 = None;

        let result = self
            .engine
            .emu_start(u64::from(start), u64::from(end), 0, instruction_budget);
        let pending_cp15 = {
            let data = self.engine.get_data_mut();
            let pending_cp15 = data.pending_cp15.take();
            let completed_instructions = data.executed_instructions
                - u64::from(matches!(pending_cp15, Some(PendingCp15::Undefined { .. })));
            for _ in 0..completed_instructions {
                data.machine
                    .finish_instruction(InstructionOutcome::Completed);
            }
            if let Some(stop) = data.callback_stop.take() {
                return stop;
            }
            pending_cp15
        };
        if let Some(pending) = pending_cp15 {
            return self.finish_cp15(pending);
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

    /// Writes an ARM register outside active emulation.
    pub fn set_register(&mut self, register: RegisterARM, value: u32) -> Result<()> {
        self.engine
            .reg_write(register, u64::from(value))
            .map_err(BackendError::Unicorn)
    }

    /// Applies a validated CP15 operation at an explicit execution boundary.
    ///
    /// VBAR is CPU-adapter state; TTBR0, SCTLR.M, and TLBIALL belong to the
    /// backend-independent MMU state in `minemu-core`.
    pub fn apply_cp15(&mut self, operation: minemu_platform::Cp15Operation) -> Option<u32> {
        let data = self.engine.get_data_mut();
        match operation {
            minemu_platform::Cp15Operation::SetVectorBase(address) => {
                data.vector_base = address.get();
                None
            }
            minemu_platform::Cp15Operation::ReadFaultStatus => data
                .machine
                .mmu
                .last_fault()
                .map(|fault| fault.status.raw()),
            minemu_platform::Cp15Operation::ReadFaultAddress => data
                .machine
                .mmu
                .last_fault()
                .map(|fault| fault.address.get()),
            operation => {
                data.machine.mmu.apply_cp15(operation);
                None
            }
        }
    }

    /// Returns the VBAR value maintained by this CPU adapter.
    pub fn vector_base(&self) -> u32 {
        self.engine.get_data().vector_base
    }

    fn finish_cp15(&mut self, pending: PendingCp15) -> BackendStop {
        match pending {
            PendingCp15::Operation(operation) => {
                self.apply_cp15(operation);
                BackendStop::Cp15Boundary
            }
            PendingCp15::ReadFaultStatus(register) => {
                let value = self
                    .apply_cp15(minemu_platform::Cp15Operation::ReadFaultStatus)
                    .unwrap_or(0);
                if self.set_register(register, value).is_err() {
                    return BackendStop::Unicorn(uc_error::ARG);
                }
                BackendStop::Cp15Boundary
            }
            PendingCp15::ReadFaultAddress(register) => {
                let value = self
                    .apply_cp15(minemu_platform::Cp15Operation::ReadFaultAddress)
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
}

fn copy_region(machine: &Machine, region: MemRegion) -> Result<Vec<u8>> {
    let range = region.range();
    let mut bytes = vec![0; range.length() as usize];
    machine.memory.read_range(range, &mut bytes)?;
    Ok(bytes)
}

fn map_bytes(
    engine: &mut Unicorn<'static, BackendData>,
    region: MemRegion,
    permissions: Prot,
    bytes: &[u8],
) -> Result<()> {
    engine
        .mem_map(
            u64::from(region.base().get()),
            u64::from(region.size()),
            permissions,
        )
        .map_err(BackendError::Unicorn)?;
    engine
        .mem_write(u64::from(region.base().get()), bytes)
        .map_err(BackendError::Unicorn)
}

fn map_mmio(engine: &mut Unicorn<'static, BackendData>, region: MemRegion) -> Result<()> {
    let base = u64::from(region.base().get());
    let size = u64::from(region.size());
    engine
        .mmio_map(
            base,
            size,
            Some(move |engine: &mut Unicorn<BackendData>, offset, size| {
                let address = base + offset;
                let Some(width) = mmio_width(size) else {
                    record_mmio_fault(engine, address, format!("unsupported read width {size}"));
                    return 0;
                };
                let transaction =
                    MmioTransaction::read(PhysicalAddress::new(address as u32), width);
                let now = engine.get_data().machine.ticks();
                let result = engine.get_data_mut().machine.bus.access(transaction, now);
                match result {
                    Ok(Some(value)) => u64::from(value),
                    Ok(None) => 0,
                    Err(error) => {
                        record_mmio_fault(engine, address, error.to_string());
                        0
                    }
                }
            }),
            Some(
                move |engine: &mut Unicorn<BackendData>, offset, size, value| {
                    let address = base + offset;
                    let Some(width) = mmio_width(size) else {
                        record_mmio_fault(
                            engine,
                            address,
                            format!("unsupported write width {size}"),
                        );
                        return;
                    };
                    let transaction = MmioTransaction::write(
                        PhysicalAddress::new(address as u32),
                        width,
                        value as u32,
                    );
                    let now = engine.get_data().machine.ticks();
                    if let Err(error) = engine.get_data_mut().machine.bus.access(transaction, now) {
                        record_mmio_fault(engine, address, error.to_string());
                    }
                },
            ),
        )
        .map_err(BackendError::Unicorn)
}

fn mmio_width(size: usize) -> Option<MmioWidth> {
    match size {
        1 => Some(MmioWidth::U8),
        2 => Some(MmioWidth::U16),
        4 => Some(MmioWidth::U32),
        8 => Some(MmioWidth::U64),
        _ => None,
    }
}

fn mmu_access(memory_type: MemType) -> Option<Access> {
    match memory_type {
        MemType::FETCH => Some(Access::Fetch),
        MemType::READ => Some(Access::Read),
        MemType::WRITE => Some(Access::Write),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct DecodedCp15 {
    condition: u32,
    read: bool,
    rt: RegisterARM,
    crn: u32,
    crm: u32,
    opc1: u32,
    opc2: u32,
}

fn decode_cp15(instruction: u32) -> Option<DecodedCp15> {
    if instruction & 0x0f00_0010 != 0x0e00_0010 || (instruction >> 8) & 0x0f != 15 {
        return None;
    }
    Some(DecodedCp15 {
        condition: instruction >> 28,
        read: instruction & (1 << 20) != 0,
        rt: arm_register((instruction >> 12) & 0x0f)?,
        crn: (instruction >> 16) & 0x0f,
        crm: instruction & 0x0f,
        opc1: (instruction >> 21) & 0x07,
        opc2: (instruction >> 5) & 0x07,
    })
}

fn cp15_pending(
    engine: &mut Unicorn<BackendData>,
    cp15: DecodedCp15,
    pc: u32,
) -> Option<PendingCp15> {
    match (cp15.read, cp15.crn, cp15.crm, cp15.opc1, cp15.opc2) {
        (false, 2, 0, 0, 0) => match engine.reg_read(cp15.rt) {
            Ok(value) => minemu_platform::Cp15Operation::set_ttbr0(value as u32)
                .map(PendingCp15::Operation)
                .unwrap_or(PendingCp15::Undefined { pc })
                .into(),
            Err(_) => Some(PendingCp15::Undefined { pc }),
        },
        (false, 1, 0, 0, 0) => engine
            .reg_read(cp15.rt)
            .ok()
            .map(|value| {
                PendingCp15::Operation(minemu_platform::Cp15Operation::set_mmu_enabled(
                    value as u32,
                ))
            })
            .or(Some(PendingCp15::Undefined { pc })),
        (false, 8, 7, 0, 0) => Some(PendingCp15::Operation(
            minemu_platform::Cp15Operation::InvalidateAll,
        )),
        (false, 12, 0, 0, 0) => match engine.reg_read(cp15.rt) {
            Ok(value) => minemu_platform::Cp15Operation::set_vector_base(value as u32)
                .map(PendingCp15::Operation)
                .unwrap_or(PendingCp15::Undefined { pc })
                .into(),
            Err(_) => Some(PendingCp15::Undefined { pc }),
        },
        (true, 5, 0, 0, 0) => Some(PendingCp15::ReadFaultStatus(cp15.rt)),
        (true, 6, 0, 0, 0) => Some(PendingCp15::ReadFaultAddress(cp15.rt)),
        _ => Some(PendingCp15::Undefined { pc }),
    }
}

fn arm_register(index: u32) -> Option<RegisterARM> {
    Some(match index {
        0 => RegisterARM::R0,
        1 => RegisterARM::R1,
        2 => RegisterARM::R2,
        3 => RegisterARM::R3,
        4 => RegisterARM::R4,
        5 => RegisterARM::R5,
        6 => RegisterARM::R6,
        7 => RegisterARM::R7,
        8 => RegisterARM::R8,
        9 => RegisterARM::R9,
        10 => RegisterARM::R10,
        11 => RegisterARM::R11,
        12 => RegisterARM::R12,
        13 => RegisterARM::SP,
        14 => RegisterARM::LR,
        15 => RegisterARM::PC,
        _ => return None,
    })
}

fn cp15_is_privileged(cpsr: u32) -> bool {
    matches!(cpsr & 0x1f, 0x13 | 0x12 | 0x17 | 0x1b)
}

fn condition_holds(condition: u32, cpsr: u32) -> bool {
    let negative = cpsr & (1 << 31) != 0;
    let zero = cpsr & (1 << 30) != 0;
    let carry = cpsr & (1 << 29) != 0;
    let overflow = cpsr & (1 << 28) != 0;
    match condition {
        0 => zero,
        1 => !zero,
        2 => carry,
        3 => !carry,
        4 => negative,
        5 => !negative,
        6 => overflow,
        7 => !overflow,
        8 => carry && !zero,
        9 => !carry || zero,
        10 => negative == overflow,
        11 => negative != overflow,
        12 => !zero && negative == overflow,
        13 => zero || negative != overflow,
        14 => true,
        _ => false,
    }
}

fn record_mmio_fault(engine: &mut Unicorn<BackendData>, address: u64, detail: String) {
    engine.get_data_mut().callback_stop = Some(BackendStop::MmioFault {
        address: address as u32,
        detail,
    });
    let _ = engine.emu_stop();
}

#[cfg(test)]
mod tests {
    use minemu_core::{Machine, PhysicalMemoryAccess};
    use minemu_platform::{MemRegion, PTE_EXECUTABLE, PTE_READABLE, PTE_VALID, PhysicalAddress};
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
        backend.apply_cp15(minemu_platform::Cp15Operation::SetVectorBase(
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
}
