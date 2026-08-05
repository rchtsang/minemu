use minemu_platform::{Access, Cp15Operation};
use unicorn_engine::{RegisterARM, Unicorn, unicorn_const::MemType};

use crate::backend::BackendData;

pub(crate) enum PendingCp15 {
    Operation(Cp15Operation),
    ReadFaultStatus(RegisterARM),
    ReadFaultAddress(RegisterARM),
    Undefined { pc: u32 },
}

#[derive(Clone, Copy)]
pub(crate) struct DecodedCp15 {
    pub condition: u32,
    read: bool,
    rt: RegisterARM,
    crn: u32,
    crm: u32,
    opc1: u32,
    opc2: u32,
}

pub(crate) fn decode_cp15(instruction: u32) -> Option<DecodedCp15> {
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

pub(crate) fn cp15_pending(
    engine: &mut Unicorn<BackendData>,
    cp15: DecodedCp15,
    pc: u32,
) -> Option<PendingCp15> {
    match (cp15.read, cp15.crn, cp15.crm, cp15.opc1, cp15.opc2) {
        (false, 2, 0, 0, 0) => match engine.reg_read(cp15.rt) {
            Ok(value) => Some(
                Cp15Operation::set_ttbr0(value as u32)
                    .map(PendingCp15::Operation)
                    .unwrap_or(PendingCp15::Undefined { pc }),
            ),
            Err(_) => Some(PendingCp15::Undefined { pc }),
        },
        (false, 1, 0, 0, 0) => {
            engine
                .reg_read(cp15.rt)
                .ok()
                .map_or(Some(PendingCp15::Undefined { pc }), |value| {
                    Some(PendingCp15::Operation(Cp15Operation::set_mmu_enabled(
                        value as u32,
                    )))
                })
        }
        (false, 8, 7, 0, 0) => Some(PendingCp15::Operation(Cp15Operation::InvalidateAll)),
        (false, 12, 0, 0, 0) => match engine.reg_read(cp15.rt) {
            Ok(value) => Some(
                Cp15Operation::set_vector_base(value as u32)
                    .map(PendingCp15::Operation)
                    .unwrap_or(PendingCp15::Undefined { pc }),
            ),
            Err(_) => Some(PendingCp15::Undefined { pc }),
        },
        (true, 5, 0, 0, 0) => Some(PendingCp15::ReadFaultStatus(cp15.rt)),
        (true, 6, 0, 0, 0) => Some(PendingCp15::ReadFaultAddress(cp15.rt)),
        _ => Some(PendingCp15::Undefined { pc }),
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
    matches!(cpsr & 0x1f, 0x13 | 0x12 | 0x17 | 0x1b)
}

pub(crate) fn condition_holds(condition: u32, cpsr: u32) -> bool {
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
