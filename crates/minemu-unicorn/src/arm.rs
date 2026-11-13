use minemu_platform::{
    Access, CPSR_CARRY, CPSR_NEGATIVE, CPSR_OVERFLOW, CPSR_ZERO, Cp15Operation, CpuMode,
};
use unicorn_engine::{RegisterARM, Unicorn, unicorn_const::MemType};

use crate::backend::BackendData;

/// A CP15 access recognized during a batch and completed at its boundary.
pub(crate) enum PendingCp15 {
    Operation(Cp15Operation),
    /// MRC of DFSR or DFAR into the given register.
    ReadFault(RegisterARM, FaultRegister),
    Undefined {
        pc: u32,
    },
}

/// The fault-reporting CP15 register read by an MRC.
#[derive(Clone, Copy)]
pub(crate) enum FaultRegister {
    Status,
    Address,
}

#[derive(Clone, Copy)]
pub(crate) struct DecodedCp15 {
    pub(crate) condition: u32,
    read: bool,
    rt: RegisterARM,
    crn: u32,
    crm: u32,
    opc1: u32,
    opc2: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DecodedBreakpoint {
    pub condition: u32,
    pub immediate: u16,
}

pub(crate) fn decode_breakpoint(instruction: u32) -> Option<DecodedBreakpoint> {
    if instruction & 0x0ff0_00f0 != 0x0120_0070 {
        return None;
    }
    let condition = instruction >> 28;
    if condition == 0x0f {
        return None;
    }
    Some(DecodedBreakpoint {
        condition,
        immediate: (((instruction >> 4) & 0xfff0) | (instruction & 0x0f)) as u16,
    })
}

pub(crate) fn decode_cp15(instruction: u32) -> Option<DecodedCp15> {
    if instruction & 0x0f00_0010 != 0x0e00_0010 || (instruction >> 8) & 0x0f != 15 {
        return None;
    }
    Some(DecodedCp15 {
        condition: instruction >> 28,
        read: instruction & (1 << 20) != 0,
        rt: general_register((instruction >> 12) & 0x0f),
        crn: (instruction >> 16) & 0x0f,
        crm: instruction & 0x0f,
        opc1: (instruction >> 21) & 0x07,
        opc2: (instruction >> 5) & 0x07,
    })
}

/// Resolves a decoded, privileged, condition-passing CP15 access.
pub(crate) fn cp15_pending(
    engine: &Unicorn<BackendData>,
    cp15: DecodedCp15,
    pc: u32,
) -> PendingCp15 {
    let undefined = PendingCp15::Undefined { pc };
    let source = || engine.reg_read(cp15.rt).map(|value| value as u32);
    match (cp15.read, cp15.crn, cp15.crm, cp15.opc1, cp15.opc2) {
        (false, 2, 0, 0, 0) => source()
            .ok()
            .and_then(|value| Cp15Operation::set_ttbr0(value).ok())
            .map_or(undefined, PendingCp15::Operation),
        (false, 1, 0, 0, 0) => source().map_or(undefined, |value| {
            PendingCp15::Operation(Cp15Operation::set_mmu_enabled(value))
        }),
        (false, 8, 7, 0, 0) => PendingCp15::Operation(Cp15Operation::InvalidateAll),
        (false, 12, 0, 0, 0) => source()
            .ok()
            .and_then(|value| Cp15Operation::set_vector_base(value).ok())
            .map_or(undefined, PendingCp15::Operation),
        (true, 5, 0, 0, 0) => PendingCp15::ReadFault(cp15.rt, FaultRegister::Status),
        (true, 6, 0, 0, 0) => PendingCp15::ReadFault(cp15.rt, FaultRegister::Address),
        _ => undefined,
    }
}

pub(crate) fn mmu_access(memory_type: MemType) -> Option<Access> {
    match memory_type {
        MemType::FETCH => Some(Access::Fetch),
        MemType::READ => Some(Access::Read),
        MemType::WRITE => Some(Access::Write),
        _ => None,
    }
}

pub(crate) fn cp15_is_privileged(cpsr: u32) -> bool {
    CpuMode::from_cpsr(cpsr).is_some_and(CpuMode::is_privileged)
}

/// Returns whether a raw CPSR register value selects USR mode.
pub(crate) fn is_user_mode(cpsr: u32) -> bool {
    CpuMode::from_cpsr(cpsr) == Some(CpuMode::User)
}

pub(crate) fn condition_holds(condition: u32, cpsr: u32) -> bool {
    let negative = cpsr & CPSR_NEGATIVE != 0;
    let zero = cpsr & CPSR_ZERO != 0;
    let carry = cpsr & CPSR_CARRY != 0;
    let overflow = cpsr & CPSR_OVERFLOW != 0;
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

/// Maps a four-bit A32 register number to its Unicorn register.
pub(crate) fn general_register(index: u32) -> RegisterARM {
    match index & 0x0f {
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
        _ => RegisterARM::PC,
    }
}

#[cfg(test)]
mod tests {
    use super::{DecodedBreakpoint, decode_breakpoint};

    #[test]
    fn decodes_only_valid_a32_breakpoints() {
        assert_eq!(
            decode_breakpoint(0xe121_2374),
            Some(DecodedBreakpoint {
                condition: 0x0e,
                immediate: 0x1234,
            })
        );
        assert_eq!(
            decode_breakpoint(0x0120_0070),
            Some(DecodedBreakpoint {
                condition: 0,
                immediate: 0,
            })
        );
        assert_eq!(decode_breakpoint(0xf120_0070), None);
        assert_eq!(decode_breakpoint(0xe120_0060), None);
    }
}
