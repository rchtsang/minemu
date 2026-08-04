use crate::Permissions;

/// Reset value and zero-seed replacement defined by the RNG ABI.
pub const DEFAULT_SEED: u32 = 0x4d45_4d55;

/// RNG registers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Seed,
    Data,
    State,
}

impl Register {
    pub(crate) const fn decode(offset: u32) -> Option<Self> {
        Some(match offset {
            0x00 => Self::Seed,
            0x04 => Self::Data,
            0x08 => Self::State,
            _ => return None,
        })
    }

    pub(crate) const fn permissions(self) -> Permissions {
        match self {
            Self::Seed => Permissions::READ.union(Permissions::WRITE),
            Self::Data | Self::State => Permissions::READ,
        }
    }

    pub(crate) fn validate_write_value(self, _value: u32) -> crate::Result<()> {
        Ok(())
    }
}
