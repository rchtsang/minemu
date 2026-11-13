//! All Unicorn callback registration is centralized here so the backend's
//! emulator-visible behavior is auditable without searching construction code.

use minemu_platform::{MmioTransaction, MmioWidth, PhysicalAddress};
use unicorn_engine::unicorn_const::{MemType, TlbEntry, TlbType};

use super::{
    Access, BackendData, BackendStop, ExceptionKind, MMIO_BASE, MemRegion, Prot, RegisterARM,
    Result, SyncException, Unicorn, VirtualAddress, arm,
};
use crate::arm::PendingCp15;

/// Size of every A32 instruction.
const A32_INSTRUCTION_SIZE: u64 = 4;

/// Registers virtual translation, instruction accounting, synchronous A32
/// trap and breakpoint interception, and invalid-instruction callbacks.
pub(super) fn register(engine: &mut Unicorn<'static, BackendData>) -> Result<()> {
    engine.ctl_set_tlb_type(TlbType::VIRTUAL)?;
    engine.add_tlb_hook(1, 0, virtual_tlb_callback)?;
    engine.add_code_hook(1, 0, instruction_counter_callback)?;
    engine.add_code_hook(1, 0, a32_synchronous_trap_callback)?;
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

fn instruction_counter_callback(engine: &mut Unicorn<BackendData>, _: u64, _: u32) {
    engine.get_data_mut().executed_instructions += 1;
}

/// Intercepts BKPT, SVC, and MCR/MRC p15 before Unicorn executes them.
fn a32_synchronous_trap_callback(engine: &mut Unicorn<BackendData>, address: u64, size: u32) {
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
fn mmio_access(
    engine: &mut Unicorn<BackendData>,
    offset: u64,
    size: usize,
    access: Access,
    value: u32,
) -> Option<u32> {
    let address = u64::from(MMIO_BASE) + offset;
    let direction = if matches!(access, Access::Write) {
        "write"
    } else {
        "read"
    };
    let Ok(width) = MmioWidth::try_from(size) else {
        stop_batch(
            engine,
            BackendStop::MmioFault {
                address: address as u32,
                access,
                detail: format!("unsupported {direction} width {size}"),
            },
        );
        return None;
    };
    let physical = PhysicalAddress::new(address as u32);
    let transaction = match access {
        Access::Write => MmioTransaction::write(physical, width, value),
        _ => MmioTransaction::read(physical, width),
    };
    let machine = &mut engine.get_data_mut().machine;
    let now = machine.ticks();
    match machine.bus.access(transaction, now) {
        Ok(value) => value,
        Err(error) => {
            stop_batch(
                engine,
                BackendStop::MmioFault {
                    address: address as u32,
                    access,
                    detail: error.to_string(),
                },
            );
            None
        }
    }
}
