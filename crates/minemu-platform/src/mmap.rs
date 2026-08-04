use crate::{PhysicalAddress, PhysicalRange, PlatformError, Result, VirtualAddress};

/// Size of an MMU page and implemented MMIO page.
pub const PAGE_SIZE: u32 = 4096;
/// Physical boot-ROM base address and size.
pub const BOOT_ROM_BASE: u32 = 0x0000_0000;
pub const BOOT_ROM_SIZE: u32 = 64 * 1024;
/// Physical system-ROM base address and size.
pub const SYSTEM_ROM_BASE: u32 = 0x0800_0000;
pub const SYSTEM_ROM_SIZE: u32 = 16 * 1024 * 1024;
/// Physical RAM base address and size.
pub const RAM_BASE: u32 = 0x4000_0000;
pub const RAM_SIZE: u32 = 64 * 1024 * 1024;

/// A classified physical memory region.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemRegion {
    BootRom,
    SystemRom,
    InterruptController,
    SysTick,
    Dma,
    Rng,
    Uart0,
    Uart1,
    Trace,
    Ram,
}

impl MemRegion {
    /// Returns this region's fixed physical range.
    pub const fn range(self) -> PhysicalRange {
        let (base, size) = match self {
            Self::BootRom => (BOOT_ROM_BASE, BOOT_ROM_SIZE),
            Self::SystemRom => (SYSTEM_ROM_BASE, SYSTEM_ROM_SIZE),
            Self::InterruptController => (0x1000_0000, PAGE_SIZE),
            Self::SysTick => (0x1000_1000, PAGE_SIZE),
            Self::Dma => (0x1000_2000, PAGE_SIZE),
            Self::Rng => (0x1000_3000, PAGE_SIZE),
            Self::Uart0 => (0x1000_4000, PAGE_SIZE),
            Self::Uart1 => (0x1000_5000, PAGE_SIZE),
            Self::Trace => (0x1000_f000, PAGE_SIZE),
            Self::Ram => (RAM_BASE, RAM_SIZE),
        };
        PhysicalRange::from_known_valid(PhysicalAddress::new(base), size)
    }

    /// Returns this region's first physical address.
    pub const fn base(self) -> PhysicalAddress {
        self.range().start()
    }

    /// Returns this region's size in bytes.
    pub const fn size(self) -> u32 {
        self.range().length()
    }

    /// Returns whether this is an implemented MMIO device page.
    pub const fn is_device(self) -> bool {
        !matches!(self, Self::BootRom | Self::SystemRom | Self::Ram)
    }
}

impl From<PhysicalAddress> for Option<MemRegion> {
    fn from(address: PhysicalAddress) -> Self {
        [
            MemRegion::BootRom,
            MemRegion::SystemRom,
            MemRegion::InterruptController,
            MemRegion::SysTick,
            MemRegion::Dma,
            MemRegion::Rng,
            MemRegion::Uart0,
            MemRegion::Uart1,
            MemRegion::Trace,
            MemRegion::Ram,
        ]
        .into_iter()
        .find(|region| region.range().contains_address(address))
    }
}

/// Converts a physical RAM address into its higher-half direct-map alias.
pub fn direct_map_virtual(address: PhysicalAddress) -> Result<VirtualAddress> {
    if Option::<MemRegion>::from(address) != Some(MemRegion::Ram) {
        return Err(PlatformError::AddressOutsideRam(address.get()));
    }
    Ok(VirtualAddress::new(address.get() + 0x8000_0000))
}

/// Converts a higher-half direct-map virtual address into physical RAM.
pub fn direct_map_physical(address: VirtualAddress) -> Result<PhysicalAddress> {
    let value = address.get();
    if !(0xc000_0000..0xc400_0000).contains(&value) {
        return Err(PlatformError::AddressOutsideRam(value));
    }
    Ok(PhysicalAddress::new(value - 0x8000_0000))
}
