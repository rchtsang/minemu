use crate::{Permissions, Result, peripherals::invalid_value};

/// Interrupt-controller registers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Pending,
    Enable,
    Claim,
    Eoi,
    PrioritySysTick,
    PriorityUart0,
    PriorityUart1,
    PriorityBlock,
}

/// A nonnegative interrupt-controller source ID passed through the IRQ vector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Source {
    SysTick = 0,
    Uart0 = 1,
    Uart1 = 2,
    Block = 3,
}

impl Source {
    /// Bit corresponding to this source in PENDING and ENABLE registers.
    pub const fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// Reset priority for SysTick, the highest-priority asynchronous source.
pub const DEFAULT_PRIORITY_SYSTICK: u8 = 0;
/// Reset priority for UART0 RX.
pub const DEFAULT_PRIORITY_UART0: u8 = 64;
/// Reset priority for UART1 RX.
pub const DEFAULT_PRIORITY_UART1: u8 = 64;
/// Reset priority for block completion.
pub const DEFAULT_PRIORITY_BLOCK: u8 = 128;

impl Register {
    pub(crate) const fn decode(offset: u32) -> Option<Self> {
        Some(match offset {
            0x00 => Self::Pending,
            0x04 => Self::Enable,
            0x08 => Self::Claim,
            0x0c => Self::Eoi,
            0x10 => Self::PrioritySysTick,
            0x14 => Self::PriorityUart0,
            0x18 => Self::PriorityUart1,
            0x1c => Self::PriorityBlock,
            _ => return None,
        })
    }

    pub(crate) const fn permissions(self) -> Permissions {
        match self {
            Self::Pending | Self::Claim => Permissions::READ,
            Self::Eoi => Permissions::WRITE,
            _ => Permissions::READ.union(Permissions::WRITE),
        }
    }

    pub(crate) fn validate_write_value(self, value: u32) -> Result<()> {
        let valid = match self {
            Self::Enable => value & !0x0f == 0,
            Self::PrioritySysTick
            | Self::PriorityUart0
            | Self::PriorityUart1
            | Self::PriorityBlock => value & !0xff == 0,
            _ => true,
        };
        if valid {
            Ok(())
        } else {
            invalid_value("interrupt controller register", value)
        }
    }
}
