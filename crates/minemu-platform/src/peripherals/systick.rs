use crate::{Permissions, Result, peripherals::invalid_value};

/// SysTick CONTROL bit that starts the timer.
pub const CONTROL_ENABLE: u32 = 1 << 0;
/// SysTick CONTROL bit that reloads the timer after each deadline.
pub const CONTROL_PERIODIC: u32 = 1 << 1;
/// SysTick CONTROL bit enabling the timer interrupt.
pub const CONTROL_IRQ_ENABLE: u32 = 1 << 2;
/// SysTick STATUS bit set when a deadline has expired.
pub const STATUS_PENDING: u32 = 1 << 0;

/// SysTick registers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Period,
    Control,
    Status,
    Ack,
}

impl Register {
    pub(crate) const fn decode(offset: u32) -> Option<Self> {
        Some(match offset {
            0x00 => Self::Period,
            0x04 => Self::Control,
            0x08 => Self::Status,
            0x0c => Self::Ack,
            _ => return None,
        })
    }

    pub(crate) const fn permissions(self) -> Permissions {
        match self {
            Self::Status => Permissions::READ,
            Self::Ack => Permissions::WRITE,
            _ => Permissions::READ.union(Permissions::WRITE),
        }
    }

    pub(crate) fn validate_write_value(self, value: u32) -> Result<()> {
        let valid = match self {
            Self::Period => value != 0,
            Self::Control => value & !(CONTROL_ENABLE | CONTROL_PERIODIC | CONTROL_IRQ_ENABLE) == 0,
            Self::Ack => value == 1,
            _ => true,
        };
        if valid {
            Ok(())
        } else {
            invalid_value("SysTick register", value)
        }
    }
}
