use minemu_core::{BlockUpdate, InterruptUpdate, Machine, PhysicalMemoryAccess, UartUpdate};
use minemu_platform::{
    CpuMode, ExceptionKind, FaultCause, MemRegion, PTE_DIRTY, PTE_EXECUTABLE, PTE_READABLE,
    PTE_VALID, PTE_WRITABLE, Peripheral, PhysicalAddress, PhysicalRange, VirtualAddress,
    peripherals::{block, interrupt, uart},
};
use unicorn_engine::RegisterARM;

use super::{BackendError, BackendStop, MEMORY_SEARCH_CHUNK_SIZE, UnicornBackend};

/// Builds a backend whose RAM starts with `code`, returning it and the RAM base.
fn backend_with_code(code: &[u8]) -> (UnicornBackend, u32) {
    let mut machine = Machine::default();
    let start = MemRegion::Ram.base().get();
    machine
        .memory
        .write_range(PhysicalAddress::new(start), code)
        .unwrap();
    (UnicornBackend::new(machine).unwrap(), start)
}

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
    let (mut backend, start) = backend_with_code(&[1, 0, 0xa0, 0xe3]); // mov r0, #1
    assert_eq!(
        backend.run(start, start + 4, 1),
        BackendStop::InstructionBudget
    );
    assert_eq!(backend.register(RegisterARM::R0).unwrap(), 1);
    assert_eq!(backend.machine().ticks(), 1);
}

#[test]
fn guest_breakpoint_stops_at_its_address_after_one_tick() {
    let (mut backend, start) = backend_with_code(&[0x74, 0x23, 0x21, 0xe1]); // bkpt #0x1234

    assert_eq!(
        backend.run(start, start + 4, 1),
        BackendStop::Breakpoint {
            address: start,
            immediate: 0x1234,
        }
    );
    assert_eq!(backend.program_counter().unwrap(), start);
    assert_eq!(backend.machine().ticks(), 1);
}

#[test]
fn conditionally_skipped_breakpoint_does_not_stop() {
    let (mut backend, start) = backend_with_code(&[0x70, 0x00, 0x20, 0x01]); // bkpteq #0
    backend
        .set_register(RegisterARM::CPSR, CpuMode::Supervisor.bits())
        .unwrap(); // Z clear

    assert_eq!(
        backend.run(start, start + 4, 1),
        BackendStop::InstructionBudget
    );
    assert_eq!(backend.program_counter().unwrap(), start + 4);
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
    backend
        .set_register(RegisterARM::CPSR, CpuMode::User.bits())
        .unwrap();

    assert_eq!(
        backend.run(0, 4, 1),
        BackendStop::Exception(ExceptionKind::PrefetchAbort)
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
        BackendStop::Exception(ExceptionKind::DataAbort)
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
fn guest_block_unit_write_and_read_route_through_the_typed_bus() {
    let mut machine = Machine::default();
    let start = MemRegion::Ram.base().get();
    let mut code = vec![
        0x08, 0x10, 0x9f, 0xe5, // ldr r1, [pc, #8]
        0x01, 0x00, 0xa0, 0xe3, // mov r0, #1
        0x20, 0x00, 0x81, 0xe5, // str r0, [r1, #0x20]
        0x20, 0x20, 0x91, 0xe5, // ldr r2, [r1, #0x20]
    ];
    code.extend_from_slice(&MemRegion::Dma.base().get().to_le_bytes());
    machine
        .memory
        .write_range(PhysicalAddress::new(start), &code)
        .unwrap();
    let mut backend = UnicornBackend::new(machine).unwrap();

    assert_eq!(
        backend.run(start, start + 16, 4),
        BackendStop::InstructionBudget
    );
    assert_eq!(backend.register(RegisterARM::R2).unwrap(), block::UNIT_SWAP);
    assert_eq!(backend.machine().bus.block.unit(), block::UNIT_SWAP);
}

#[test]
fn guest_block_command_snapshots_unit_before_later_mmio_write() {
    let mut machine = Machine::default();
    let start = MemRegion::Ram.base().get();
    let dma = start + 0x1000;
    let mut code = vec![
        0x20, 0x00, 0x81, 0xe5, // str r0, [r1, #0x20]
        0x04, 0x20, 0x81, 0xe5, // str r2, [r1, #4]
        0x08, 0x30, 0x81, 0xe5, // str r3, [r1, #8]
        0x0c, 0x40, 0x81, 0xe5, // str r4, [r1, #0xc]
        0x00, 0x30, 0x81, 0xe5, // str r3, [r1]
        0x20, 0x30, 0x81, 0xe5, // str r3, [r1, #0x20]
    ];
    code.extend(std::iter::repeat_n([0x00, 0xf0, 0x20, 0xe3], 32).flatten());
    machine
        .memory
        .write_range(PhysicalAddress::new(start), &code)
        .unwrap();
    machine
        .bus
        .block
        .update(BlockUpdate::Attach {
            unit: block::UNIT_FILESYSTEM,
            media: vec![0x11; 512],
        })
        .unwrap();
    machine
        .bus
        .block
        .update(BlockUpdate::Attach {
            unit: block::UNIT_SWAP,
            media: vec![0x22; 512],
        })
        .unwrap();
    let mut backend = UnicornBackend::new(machine).unwrap();
    backend.set_register(RegisterARM::R0, 0).unwrap();
    backend
        .set_register(RegisterARM::R1, MemRegion::Dma.base().get())
        .unwrap();
    backend.set_register(RegisterARM::R2, 0).unwrap();
    backend.set_register(RegisterARM::R3, 1).unwrap();
    backend.set_register(RegisterARM::R4, dma).unwrap();

    assert_eq!(
        backend.run(start, start + 24, 6),
        BackendStop::InstructionBudget
    );
    let inspection = backend.machine().bus.block.inspect();
    assert_eq!(inspection.unit, block::UNIT_SWAP);
    assert_eq!(inspection.active_unit, Some(block::UNIT_FILESYSTEM));

    assert_eq!(
        backend.run(start + 24, start + 152, 32),
        BackendStop::InstructionBudget
    );
    assert_eq!(
        backend
            .machine()
            .memory
            .read_u32(PhysicalAddress::new(dma))
            .unwrap(),
        0x1111_1111
    );
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
        BackendStop::Exception(ExceptionKind::DataAbort)
    );
    let fault = backend.machine().mmu.last_fault().unwrap();
    assert_eq!(fault.address, VirtualAddress::new(invalid_address));
    assert_eq!(fault.status.cause(), Some(FaultCause::DeviceAccess));
    assert!(!fault.status.is_write());
}

#[test]
fn physical_mmio_fetch_enters_prefetch_abort() {
    let address = MemRegion::Rng.base().get();
    let mut backend = UnicornBackend::new(Machine::default()).unwrap();

    assert_eq!(
        backend.run(address, address + 4, 1),
        BackendStop::Exception(ExceptionKind::PrefetchAbort)
    );
    let fault = backend.machine().mmu.last_fault().unwrap();
    assert_eq!(fault.address, VirtualAddress::new(address));
    assert_eq!(fault.status.cause(), Some(FaultCause::DeviceAccess));
    assert!(fault.status.is_fetch());
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
    backend
        .set_register(RegisterARM::CPSR, CpuMode::User.bits())
        .unwrap(); // USR
    let plan = minemu_core::ExceptionPlan::synchronous(
        ExceptionKind::SupervisorCall,
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
    backend
        .set_register(RegisterARM::CPSR, CpuMode::Supervisor.bits())
        .unwrap();
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
    let (mut backend, start) = backend_with_code(&[0, 0, 0, 0xef]); // svc #0
    backend
        .set_register(RegisterARM::CPSR, CpuMode::Supervisor.bits())
        .unwrap();
    assert_eq!(
        backend.run(start, start + 4, 1),
        BackendStop::Exception(ExceptionKind::SupervisorCall)
    );
    assert_eq!(backend.register(RegisterARM::PC).unwrap(), 8);
    assert_eq!(backend.machine().ticks(), 2);
}

#[test]
fn tick_deadline_can_pause_between_svc_retirement_and_exception_entry() {
    let (mut backend, start) = backend_with_code(&[0, 0, 0, 0xef]); // svc #0
    backend
        .set_register(RegisterARM::CPSR, CpuMode::Supervisor.bits())
        .unwrap();

    assert_eq!(
        backend.run_until_tick(start, start + 4, 1, 1),
        BackendStop::InstructionBudget
    );
    assert_eq!(backend.machine().ticks(), 1);
    assert_eq!(
        backend.run_until_tick(start, start + 4, 1, 2),
        BackendStop::Exception(ExceptionKind::SupervisorCall)
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
    backend
        .set_register(RegisterARM::CPSR, CpuMode::User.bits())
        .unwrap();
    assert_eq!(
        backend.run(start, start + 4, 1),
        BackendStop::Exception(ExceptionKind::Undefined)
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
        BackendStop::Exception(ExceptionKind::PrefetchAbort)
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
        BackendStop::Exception(ExceptionKind::DataAbort)
    );
    assert_eq!(backend.register(RegisterARM::PC).unwrap(), 16);
    assert_eq!(backend.register(RegisterARM::LR).unwrap(), 8);
    assert_eq!(backend.machine().ticks(), 1);
}

#[test]
fn fetch_fault_after_completed_instructions_charges_each_of_them() {
    let mut machine = Machine::default();
    let ram = MemRegion::Ram.base().get();
    let table = ram + 0x1000;
    let code = ram + 0x2000;
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
    machine
        .memory
        .write_range(
            PhysicalAddress::new(code),
            &[
                0x00, 0xf0, 0x20, 0xe3, // nop
                0xfd, 0x03, 0x00, 0xea, // b 0x1000 (unmapped)
            ],
        )
        .unwrap();
    machine.mmu.set_ttbr0(PhysicalAddress::new(ram));
    machine.mmu.set_enabled(true);
    let mut backend = UnicornBackend::new(machine).unwrap();
    assert_eq!(
        backend.run(0, u32::MAX, 16),
        BackendStop::Exception(ExceptionKind::PrefetchAbort)
    );
    // nop and b complete (one tick each); the abort entry costs one more.
    assert_eq!(backend.machine().ticks(), 3);
    assert_eq!(backend.register(RegisterARM::LR).unwrap(), 0x1004);
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
    backend
        .set_register(RegisterARM::CPSR, CpuMode::Supervisor.bits())
        .unwrap();
    assert_eq!(
        backend.run(start, start + 4, 1),
        BackendStop::Exception(ExceptionKind::Interrupt)
    );
    // The IRQ is taken at the boundary before the nop, which has not run.
    assert_eq!(backend.register(RegisterARM::PC).unwrap(), 24);
    assert_eq!(backend.register(RegisterARM::LR).unwrap(), start + 4);
    assert_eq!(backend.machine().ticks(), 1);
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
    backend
        .set_register(RegisterARM::CPSR, CpuMode::Supervisor.bits())
        .unwrap();
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
    backend
        .set_register(RegisterARM::CPSR, CpuMode::Supervisor.bits())
        .unwrap();
    backend.set_register(RegisterARM::R0, directory_b).unwrap();
    assert_eq!(backend.run(0, 4, 1), BackendStop::Cp15Boundary);
    assert_eq!(backend.run(4, 8, 1), BackendStop::Cp15Boundary);
    assert_eq!(backend.run(8, 12, 1), BackendStop::InstructionBudget);
    assert_eq!(backend.register(RegisterARM::R1).unwrap(), 2);
}

mod batch_independence {
    use minemu_core::{InterruptUpdate, Machine, PhysicalMemoryAccess};
    use minemu_platform::{
        Cp15Operation, CpuMode, ExceptionKind, InspectionRequest, InspectionResponse, MemRegion,
        ObservableEvent, Peripheral, PhysicalAddress,
        peripherals::{interrupt, systick},
    };
    use unicorn_engine::RegisterARM;

    use super::super::{BackendStop, UnicornBackend};

    fn movw(rd: u32, value: u32) -> u32 {
        0xe300_0000 | (value & 0xf000) << 4 | rd << 12 | (value & 0xfff)
    }

    fn movt(rd: u32, value: u32) -> u32 {
        0xe340_0000 | (value >> 28) << 16 | rd << 12 | (value >> 16 & 0xfff)
    }

    /// Encodes `str rt, [rn, #offset]`.
    fn store(rt: u32, rn: u32, offset: u32) -> u32 {
        0xe580_0000 | rn << 16 | rt << 12 | offset
    }

    /// The observable result of running until the first IRQ entry.
    #[derive(Debug, Eq, PartialEq)]
    struct Outcome {
        irq_tick: u64,
        irq_lr: u32,
        events: Vec<ObservableEvent>,
    }

    /// Programs a one-shot 50-tick SysTick from the guest after a few
    /// instructions, then writes an incrementing counter to the trace device
    /// until the timer IRQ is taken.
    fn run_until_irq(instruction_batch: usize) -> Outcome {
        let ram = MemRegion::Ram.base().get();
        let start = ram + 0x100;
        let systick_base = MemRegion::SysTick.base().get();
        let trace_base = MemRegion::Trace.base().get();
        let program = [
            0xe320_f000, // nop
            0xe320_f000, // nop
            0xe320_f000, // nop
            movw(0, systick_base),
            movt(0, systick_base),
            movw(1, 50),
            store(1, 0, 0x00), // PERIOD
            movw(1, systick::CONTROL_ENABLE | systick::CONTROL_IRQ_ENABLE),
            store(1, 0, 0x04), // CONTROL
            movw(2, trace_base),
            movt(2, trace_base),
            movw(3, 0),
            store(3, 2, 0), // loop: str r3, [r2]
            0xe283_3001,    // add r3, r3, #1
            0xeaff_fffc,    // b loop
        ];
        let bytes: Vec<u8> = program.iter().flat_map(|word| word.to_le_bytes()).collect();
        let mut machine = Machine::default();
        machine
            .memory
            .write_range(PhysicalAddress::new(start), &bytes)
            .unwrap();
        machine
            .mmu
            .apply_cp15(Cp15Operation::set_vector_base(ram).unwrap());
        machine
            .bus
            .interrupts
            .update(InterruptUpdate::Write {
                register: interrupt::Register::Enable,
                value: interrupt::Source::SysTick.bit(),
            })
            .unwrap();
        let mut backend = UnicornBackend::new(machine).unwrap();
        backend
            .set_register(RegisterARM::CPSR, CpuMode::Supervisor.bits())
            .unwrap();

        let mut pc = start;
        loop {
            match backend.run_until_tick(pc, u32::MAX, instruction_batch, 1_000) {
                BackendStop::Exception(ExceptionKind::Interrupt) => break,
                BackendStop::InstructionBudget => pc = backend.program_counter().unwrap(),
                stop => panic!("unexpected stop {stop:?}"),
            }
            assert!(backend.machine().ticks() < 1_000, "no IRQ was taken");
        }
        let InspectionResponse::Events(events) = backend
            .machine_mut()
            .inspect(InspectionRequest::Events)
            .unwrap()
        else {
            unreachable!("event inspection has a fixed response type")
        };
        Outcome {
            irq_tick: backend.machine().ticks(),
            irq_lr: backend.register(RegisterARM::LR).unwrap(),
            events,
        }
    }

    #[test]
    fn guest_visible_timing_does_not_depend_on_batch_size() {
        let exact = run_until_irq(1);
        // CONTROL issues at tick 8, so the deadline is 58 and IRQ entry costs one tick.
        assert_eq!(exact.irq_tick, 59, "{exact:#?}");
        assert_eq!(run_until_irq(7), exact);
        assert_eq!(run_until_irq(1024), exact);
    }
}
