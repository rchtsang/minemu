//! All Unicorn callback registration is centralized here so the backend's
//! emulator-visible behavior is auditable without searching construction code.

use minemu_core::InstructionOutcome;
use minemu_platform::{CPSR_IRQ_DISABLE, MmioTransaction, MmioWidth, PhysicalAddress};
use unicorn_engine::unicorn_const::{MemType, TlbEntry, TlbType};

use super::{
    Access, BackendData, BackendStop, ExceptionKind, MMIO_BASE, MemRegion, Prot, RegisterARM,
    Result, SyncException, Unicorn, VirtualAddress, arm,
};
use crate::arm::PendingCp15;

/// Size of every A32 instruction.
const A32_INSTRUCTION_SIZE: u64 = 4;

/// Registers virtual translation, instruction-boundary, and invalid-instruction
/// callbacks.
pub(super) fn register(engine: &mut Unicorn<'static, BackendData>) -> Result<()> {
    engine.ctl_set_tlb_type(TlbType::VIRTUAL)?;
    engine.add_tlb_hook(1, 0, virtual_tlb_callback)?;
    engine.add_code_hook(1, 0, instruction_boundary_callback)?;
    engine.add_insn_invalid_hook(invalid_instruction_callback)?;
    Ok(())
}

fn virtual_tlb_callback(
    engine: &mut Unicorn<BackendData>,
    address: u64,
    memory_type: MemType,
) -> Option<TlbEntry> {
    let access = arm::mmu_access(memory_type)?;
    let user_mode = arm::is_user_mode(engine.reg_read(RegisterARM::CPSR).ok()? as u32);
    let BackendData {
        machine,
        callback_stop,
        ..
    } = engine.get_data_mut();
    let address = VirtualAddress::new(address as u32);
    match machine
        .mmu
        .translate(&mut machine.memory, address, access, user_mode)
    {
        // Device pages are never executable.
        Ok(physical)
            if matches!(access, Access::Fetch)
                && Option::<MemRegion>::from(physical).is_some_and(MemRegion::is_device) =>
        {
            let fault = machine
                .mmu
                .record_invalid_mmio_fault(address, access, user_mode);
            *callback_stop = Some(BackendStop::MmuFault(fault));
            None
        }
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

/// Runs at every instruction boundary, before the instruction at `address`.
///
/// Retiring the previous instruction here keeps devices, trace stamps, and IRQ
/// delivery exact regardless of how many instructions a batch requests
/// (MINEMU-ABI1-TIME-003/004).
fn instruction_boundary_callback(engine: &mut Unicorn<BackendData>, address: u64, size: u32) {
    let data = engine.get_data_mut();
    data.current_instruction = address as u32;
    if std::mem::take(&mut data.instruction_in_flight) {
        data.machine
            .finish_instruction(InstructionOutcome::Completed);
    }
    if irq_deliverable(engine) {
        // Stop before this instruction; the backend enters the IRQ vector.
        let _ = engine.emu_stop();
        return;
    }
    engine.get_data_mut().instruction_in_flight = true;
    a32_synchronous_trap(engine, address, size);
}

/// Whether an IRQ would be taken at this boundary.
fn irq_deliverable(engine: &mut Unicorn<BackendData>) -> bool {
    engine.get_data_mut().machine.bus.interrupts.has_claim()
        && engine
            .reg_read(RegisterARM::CPSR)
            .is_ok_and(|cpsr| cpsr as u32 & CPSR_IRQ_DISABLE == 0)
}

/// Intercepts BKPT, SVC, and MCR/MRC p15 before Unicorn executes them.
fn a32_synchronous_trap(engine: &mut Unicorn<BackendData>, address: u64, size: u32) {
    if u64::from(size) != A32_INSTRUCTION_SIZE {
        return;
    }
    let mut bytes = [0; 4];
    if engine.vmem_read(address, Prot::EXEC, &mut bytes).is_err() {
        return;
    }
    let instruction = u32::from_le_bytes(bytes);
    let cpsr = engine.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
    let pc = address as u32;

    if let Some(breakpoint) = arm::decode_breakpoint(instruction)
        && arm::condition_holds(breakpoint.condition, cpsr)
    {
        // The PC stays on the breakpoint so the host can inspect or skip it.
        stop_batch(
            engine,
            BackendStop::Breakpoint {
                address: pc,
                immediate: breakpoint.immediate,
            },
        );
    } else if is_svc(instruction) && arm::condition_holds(instruction >> 28, cpsr) {
        engine.get_data_mut().pending_exception = Some(SyncException {
            kind: ExceptionKind::SupervisorCall,
            pc: VirtualAddress::new(pc),
        });
        skip_and_stop(engine, address);
    } else if let Some(cp15) = arm::decode_cp15(instruction) {
        if arm::condition_holds(cp15.condition, cpsr) {
            engine.get_data_mut().pending_cp15 = Some(if arm::cp15_is_privileged(cpsr) {
                arm::cp15_pending(engine, cp15, pc)
            } else {
                PendingCp15::Undefined { pc }
            });
        }
        skip_and_stop(engine, address);
    }
}

fn is_svc(instruction: u32) -> bool {
    instruction & 0x0f00_0000 == 0x0f00_0000
}

fn invalid_instruction_callback(engine: &mut Unicorn<BackendData>) -> bool {
    let pc = engine.reg_read(RegisterARM::PC).unwrap_or(0);
    engine.get_data_mut().pending_exception = Some(SyncException {
        kind: ExceptionKind::Undefined,
        pc: VirtualAddress::new(pc as u32),
    });
    skip_and_stop(engine, u64::from(pc as u32));
    true
}

/// Advances past the trapping instruction at `address` and ends the batch.
fn skip_and_stop(engine: &mut Unicorn<BackendData>, address: u64) {
    let _ = engine.reg_write(RegisterARM::PC, address + A32_INSTRUCTION_SIZE);
    let _ = engine.emu_stop();
}

fn stop_batch(engine: &mut Unicorn<BackendData>, stop: BackendStop) {
    engine.get_data_mut().callback_stop = Some(stop);
    let _ = engine.emu_stop();
}

pub(super) fn mmio_read_callback(
    engine: &mut Unicorn<BackendData>,
    offset: u64,
    size: usize,
) -> u64 {
    mmio_access(engine, offset, size, Access::Read, 0).map_or(0, u64::from)
}

pub(super) fn mmio_write_callback(
    engine: &mut Unicorn<BackendData>,
    offset: u64,
    size: usize,
    value: u64,
) {
    mmio_access(engine, offset, size, Access::Write, value as u32);
}

/// Dispatches one guest MMIO access to the typed bus, stopping the batch on failure.
///
/// A failure is reported at the access's virtual address (MINEMU-MMU1-FAULT-003).
fn mmio_access(
    engine: &mut Unicorn<BackendData>,
    offset: u64,
    size: usize,
    access: Access,
    value: u32,
) -> Option<u32> {
    let physical = PhysicalAddress::new((u64::from(MMIO_BASE) + offset) as u32);
    let direction = if matches!(access, Access::Write) {
        "write"
    } else {
        "read"
    };
    let result = match MmioWidth::try_from(size) {
        Err(_) => Err(format!("unsupported {direction} width {size}")),
        Ok(width) => {
            let transaction = match access {
                Access::Write => MmioTransaction::write(physical, width, value),
                _ => MmioTransaction::read(physical, width),
            };
            let machine = &mut engine.get_data_mut().machine;
            let now = machine.ticks();
            machine
                .bus
                .access(transaction, now)
                .map_err(|error| error.to_string())
        }
    };
    result.unwrap_or_else(|detail| {
        let address = fault_virtual_address(engine, physical.get()).unwrap_or(physical.get());
        stop_batch(
            engine,
            BackendStop::MmioFault {
                address,
                access,
                detail,
            },
        );
        None
    })
}

/// Recovers the virtual address of a failed MMIO access, which Unicorn's MMIO
/// callbacks (and, under a virtual TLB, its memory hooks) only report
/// physically.
///
/// The callback runs before the instruction writes any register, so the
/// instruction's addressing registers still hold their original values.
fn fault_virtual_address(engine: &Unicorn<BackendData>, physical: u32) -> Option<u32> {
    let data = engine.get_data();
    if !data.machine.mmu.enabled() {
        return Some(physical);
    }
    let pc = data.current_instruction;
    let mut bytes = [0; 4];
    engine
        .vmem_read(u64::from(pc), Prot::EXEC, &mut bytes)
        .ok()?;
    let register = |index: u32| match index {
        15 => pc.wrapping_add(8),
        _ => engine
            .reg_read(arm::general_register(index))
            .map_or(0, |value| value as u32),
    };
    let range = arm::data_access_range(u32::from_le_bytes(bytes), register)?;
    arm::address_in_range_with_offset(range, physical)
}
