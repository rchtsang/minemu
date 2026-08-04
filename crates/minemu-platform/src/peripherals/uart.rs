use crate::{Permissions, Result, peripherals::invalid_value};

/// UART registers shared by UART0 and UART1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    ReceiveData,
    TransmitData,
    Status,
    Control,
}

impl Register {
    pub(crate) const fn decode(offset: u32) -> Option<Self> {
        Some(match offset {
            0x00 => Self::ReceiveData,
            0x04 => Self::TransmitData,
            0x08 => Self::Status,
            0x0c => Self::Control,
            _ => return None,
        })
    }

    pub(crate) const fn permissions(self) -> Permissions {
        match self {
            Self::ReceiveData | Self::Status => Permissions::READ,
            Self::TransmitData => Permissions::WRITE,
            Self::Control => Permissions::READ.union(Permissions::WRITE),
        }
    }

    pub(crate) fn validate_write_value(self, value: u32) -> Result<()> {
        let valid = match self {
            Self::TransmitData => value & !0xff == 0,
            Self::Control => value & !1 == 0,
            _ => true,
        };
        if valid {
            Ok(())
        } else {
            invalid_value("UART register", value)
        }
    }
}
