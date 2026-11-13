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
/// Returns the virtual byte range `(start, length)` that an A32 load or store
/// accesses, given each register's value before the instruction executes
/// (`register(15)` must return the instruction address plus 8).
///
/// Covers single, halfword, doubleword, exclusive, swap, and block transfers.
pub(crate) fn data_access_range(
    instruction: u32,
    register: impl Fn(u32) -> u32,
) -> Option<(u32, u32)> {
    let bit = |index: u32| instruction & (1 << index) != 0;
    let base = register((instruction >> 16) & 0x0f);
    let (pre_indexed, add) = (bit(24), bit(23));
    let indexed = |offset: u32| match (pre_indexed, add) {
        (false, _) => base,
        (true, true) => base.wrapping_add(offset),
        (true, false) => base.wrapping_sub(offset),
    };
    let size = |byte: bool| if byte { 1 } else { 4 };
    match (instruction >> 25) & 0b111 {
        // LDR/STR/LDRB/STRB with an immediate offset.
        0b010 => Some((indexed(instruction & 0xfff), size(bit(22)))),
        // LDR/STR/LDRB/STRB with a shifted register offset.
        0b011 if !bit(4) => {
            let rm = register(instruction & 0x0f);
            let amount = (instruction >> 7) & 0x1f;
            let offset = match ((instruction >> 5) & 0b11, amount) {
                (0b00, _) => rm << amount,
                (0b01, 0) => 0,
                (0b01, _) => rm >> amount,
                (0b10, 0) => ((rm as i32) >> 31) as u32,
                (0b10, _) => ((rm as i32) >> amount) as u32,
                // RRX depends on the carry flag; it is never used to address MMIO.
                (_, 0) => return None,
                _ => rm.rotate_right(amount),
            };
            Some((indexed(offset), size(bit(22))))
        }
        0b000 if bit(7) && bit(4) => match (instruction >> 5) & 0b11 {
            // SWP and the exclusive loads and stores address [Rn].
            0b00 if bit(24) => Some((base, 8)),
            0b00 => None,
            kind => {
                let offset = if bit(22) {
                    ((instruction >> 4) & 0xf0) | (instruction & 0x0f)
                } else {
                    register(instruction & 0x0f)
                };
                let length = match (kind, bit(20)) {
                    (0b10, true) => 1,                  // LDRSB
                    (0b10, false) | (0b11, false) => 8, // LDRD/STRD
                    _ => 2,                             // LDRH/STRH/LDRSH
                };
                Some((indexed(offset), length))
            }
        },
        // LDM/STM.
        0b100 => {
            let length = (instruction & 0xffff).count_ones() * 4;
            let start = match (pre_indexed, add) {
                (false, true) => base,
                (true, true) => base.wrapping_add(4),
                (false, false) => base.wrapping_sub(length).wrapping_add(4),
                (true, false) => base.wrapping_sub(length),
            };
            Some((start, length))
        }
        _ => None,
    }
}

/// Returns the address in `range` whose page offset matches `physical`. An
/// aligned device access never crosses a page, so for ranges shorter than a
/// page this is the virtual address of that access.
pub(crate) fn address_in_range_with_offset(
    (start, length): (u32, u32),
    physical: u32,
) -> Option<u32> {
    const PAGE_MASK: u32 = 0xfff;
    let mut candidate = (start & !PAGE_MASK) | (physical & PAGE_MASK);
    if candidate < start {
        candidate = candidate.wrapping_add(PAGE_MASK + 1);
    }
    (candidate.wrapping_sub(start) < length).then_some(candidate)
}

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
    fn device_offsets_select_the_matching_address_in_a_range() {
        use super::address_in_range_with_offset;
        assert_eq!(
            address_in_range_with_offset((0x5008, 8), 0x1000_300c),
            Some(0x500c)
        );
        // A range that crosses into the next page.
        assert_eq!(
            address_in_range_with_offset((0x5ffc, 8), 0x1000_3000),
            Some(0x6000)
        );
        assert_eq!(address_in_range_with_offset((0x5008, 4), 0x1000_300c), None);
    }

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
