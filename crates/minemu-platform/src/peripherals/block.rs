use crate::Permissions;

/// Block-device registers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Command,
    Lba,
    SectorCount,
    PhysicalAddress,
    Status,
    Error,
    Ack,
    Control,
    Unit,
}

/// General-purpose/filesystem media unit.
pub const UNIT_FILESYSTEM: u32 = 0;
/// Swap media unit.
pub const UNIT_SWAP: u32 = 1;
/// Number of supported media units.
pub const UNIT_COUNT: usize = 2;

/// Bytes in one block-device sector.
pub const SECTOR_SIZE: u32 = 512;
/// Virtual ticks between accepting a command and completing it.
pub const COMPLETION_DELAY: u64 = 32;

/// COMMAND value that copies media sectors into RAM.
pub const COMMAND_READ: u32 = 1;
/// COMMAND value that copies RAM into media sectors.
pub const COMMAND_WRITE: u32 = 2;

/// Block CONTROL bit enabling the completion interrupt.
pub const CONTROL_IRQ_ENABLE: u32 = 1 << 0;

/// Block STATUS busy bit.
pub const STATUS_BUSY: u32 = 1 << 0;
/// Block STATUS complete bit.
pub const STATUS_COMPLETE: u32 = 1 << 1;
/// Block STATUS error bit.
pub const STATUS_ERROR: u32 = 1 << 2;

/// Guest-visible block error values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    None = 0,
    NoMedia = 1,
    Busy = 2,
    InvalidCommand = 3,
    InvalidDma = 4,
    InvalidLba = 5,
    DeferredPersistence = 6,
    InvalidUnit = 7,
}

impl Register {
    pub(crate) const fn decode(offset: u32) -> Option<Self> {
        Some(match offset {
            0x00 => Self::Command,
            0x04 => Self::Lba,
            0x08 => Self::SectorCount,
            0x0c => Self::PhysicalAddress,
            0x10 => Self::Status,
            0x14 => Self::Error,
            0x18 => Self::Ack,
            0x1c => Self::Control,
            0x20 => Self::Unit,
            _ => return None,
        })
    }

    pub(crate) const fn permissions(self) -> Permissions {
        match self {
            Self::Status | Self::Error => Permissions::READ,
            Self::Command | Self::Ack => Permissions::WRITE,
            _ => Permissions::READ.union(Permissions::WRITE),
        }
    }

    pub(crate) fn validate_write_value(self, value: u32) -> crate::Result<()> {
        if (matches!(self, Self::Ack) && value != 1)
            || (matches!(self, Self::Control) && value & !CONTROL_IRQ_ENABLE != 0)
        {
            return super::invalid_value("block-device register", value);
        }
        Ok(())
    }
}
