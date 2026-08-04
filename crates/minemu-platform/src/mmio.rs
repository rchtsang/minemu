use crate::{
    Access, MemRegion, Permissions, PhysicalAddress, PlatformError, Result,
    peripherals::{block, interrupt, rng, systick, trace, uart},
};

/// Width of an attempted MMIO transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MmioWidth {
    U8 = 1,
    U16 = 2,
    U32 = 4,
    U64 = 8,
}

/// A backend-neutral MMIO transaction to validate and dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MmioTransaction {
    pub address: PhysicalAddress,
    pub width: MmioWidth,
    pub access: Access,
    pub value: u32,
}

impl MmioTransaction {
    /// Creates a read transaction.
    pub const fn read(address: PhysicalAddress, width: MmioWidth) -> Self {
        Self {
            address,
            width,
            access: Access::Read,
            value: 0,
        }
    }

    /// Creates a write transaction.
    pub const fn write(address: PhysicalAddress, width: MmioWidth, value: u32) -> Self {
        Self {
            address,
            width,
            access: Access::Write,
            value,
        }
    }
}

/// A decoded implemented MMIO register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MmioRegister {
    Interrupt(interrupt::Register),
    SysTick(systick::Register),
    Dma(block::Register),
    Rng(rng::Register),
    Uart0(uart::Register),
    Uart1(uart::Register),
    Trace(trace::Register),
}

impl MmioRegister {
    fn decode(region: MemRegion, offset: u32) -> Option<Self> {
        match region {
            MemRegion::InterruptController => {
                interrupt::Register::decode(offset).map(Self::Interrupt)
            }
            MemRegion::SysTick => systick::Register::decode(offset).map(Self::SysTick),
            MemRegion::Dma => block::Register::decode(offset).map(Self::Dma),
            MemRegion::Rng => rng::Register::decode(offset).map(Self::Rng),
            MemRegion::Uart0 => uart::Register::decode(offset).map(Self::Uart0),
            MemRegion::Uart1 => uart::Register::decode(offset).map(Self::Uart1),
            MemRegion::Trace => trace::Register::decode(offset).map(Self::Trace),
            _ => None,
        }
    }

    const fn permissions(self) -> Permissions {
        match self {
            Self::Interrupt(register) => register.permissions(),
            Self::SysTick(register) => register.permissions(),
            Self::Dma(register) => register.permissions(),
            Self::Rng(register) => register.permissions(),
            Self::Uart0(register) | Self::Uart1(register) => register.permissions(),
            Self::Trace(register) => register.permissions(),
        }
    }

    fn validate_write_value(self, value: u32) -> Result<()> {
        match self {
            Self::Interrupt(register) => register.validate_write_value(value),
            Self::SysTick(register) => register.validate_write_value(value),
            Self::Dma(register) => register.validate_write_value(value),
            Self::Rng(register) => register.validate_write_value(value),
            Self::Uart0(register) | Self::Uart1(register) => register.validate_write_value(value),
            Self::Trace(register) => register.validate_write_value(value),
        }
    }
}

/// Validates a transaction and identifies its target peripheral register.
pub fn decode_mmio(transaction: MmioTransaction) -> Result<MmioRegister> {
    if transaction.width != MmioWidth::U32 {
        return Err(PlatformError::InvalidMmioWidth(transaction.width as u8));
    }
    let address = transaction.address.get();
    if address & 3 != 0 {
        return Err(PlatformError::UnalignedMmioAddress(address));
    }
    if !matches!(transaction.access, Access::Read | Access::Write) {
        return Err(PlatformError::InvalidMmioDirection);
    }

    let Some(region) = Option::<MemRegion>::from(transaction.address) else {
        return Err(PlatformError::InvalidMmioAddress(address));
    };
    let offset = address - region.base().get();
    let register =
        MmioRegister::decode(region, offset).ok_or(PlatformError::InvalidMmioAddress(address))?;
    if !register.permissions().permits(transaction.access, false) {
        return Err(PlatformError::InvalidMmioDirection);
    }
    if matches!(transaction.access, Access::Write) {
        register.validate_write_value(transaction.value)?;
    }
    Ok(register)
}
