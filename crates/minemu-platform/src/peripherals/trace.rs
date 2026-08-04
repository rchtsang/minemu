use crate::Permissions;

/// Trace peripheral registers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Event,
}

impl Register {
    pub(crate) const fn decode(offset: u32) -> Option<Self> {
        if offset == 0 { Some(Self::Event) } else { None }
    }

    pub(crate) const fn permissions(self) -> Permissions {
        Permissions::WRITE
    }

    pub(crate) fn validate_write_value(self, _value: u32) -> crate::Result<()> {
        Ok(())
    }
}
