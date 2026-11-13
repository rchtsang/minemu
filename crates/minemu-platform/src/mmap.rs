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
/// Physical base and size of the window containing every MMIO device page.
pub const MMIO_BASE: u32 = 0x1000_0000;
pub const MMIO_SIZE: u32 = 16 * PAGE_SIZE;
/// Offset from physical RAM to its higher-half kernel direct-map alias.
pub const DIRECT_MAP_OFFSET: u32 = 0x8000_0000;
/// First virtual address of the kernel direct map.
pub const DIRECT_MAP_BASE: u32 = RAM_BASE + DIRECT_MAP_OFFSET;
/// Temporary stack reserved for platform boot-ROM C code.
pub const BOOT_STACK_BASE: u32 = RAM_BASE + 0x6000;
pub const BOOT_STACK_SIZE: u32 = PAGE_SIZE;
/// Physical and direct-mapped virtual addresses of the boot-info record.
pub const BOOT_INFO_PADDR: u32 = RAM_BASE + 0x7000;
pub const BOOT_INFO_VADDR: u32 = BOOT_INFO_PADDR + DIRECT_MAP_OFFSET;
/// Physical entry populated by the boot ROM before kernel handoff.
pub const BOOTSTRAP_ENTRY_PADDR: u32 = RAM_BASE + 0x8000;

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
    /// Every region, in ascending address order.
    pub const ALL: [Self; 10] = [
        Self::BootRom,
        Self::SystemRom,
        Self::InterruptController,
        Self::SysTick,
        Self::Dma,
        Self::Rng,
        Self::Uart0,
        Self::Uart1,
        Self::Trace,
        Self::Ram,
    ];

    /// Returns this region's fixed physical range.
    pub const fn range(self) -> PhysicalRange {
        let (base, size) = match self {
            Self::BootRom => (BOOT_ROM_BASE, BOOT_ROM_SIZE),
            Self::SystemRom => (SYSTEM_ROM_BASE, SYSTEM_ROM_SIZE),
            Self::InterruptController => (MMIO_BASE, PAGE_SIZE),
            Self::SysTick => (MMIO_BASE + 0x1000, PAGE_SIZE),
            Self::Dma => (MMIO_BASE + 0x2000, PAGE_SIZE),
            Self::Rng => (MMIO_BASE + 0x3000, PAGE_SIZE),
            Self::Uart0 => (MMIO_BASE + 0x4000, PAGE_SIZE),
            Self::Uart1 => (MMIO_BASE + 0x5000, PAGE_SIZE),
            Self::Trace => (MMIO_BASE + 0xf000, PAGE_SIZE),
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
        MemRegion::ALL
            .into_iter()
            .find(|region| region.range().contains_address(address))
    }
}

/// Converts a physical RAM address into its higher-half direct-map alias.
pub fn direct_map_virtual(address: PhysicalAddress) -> Result<VirtualAddress> {
    if Option::<MemRegion>::from(address) != Some(MemRegion::Ram) {
        return Err(PlatformError::AddressOutsideRam(address.get()));
    }
    Ok(VirtualAddress::new(address.get() + DIRECT_MAP_OFFSET))
}

/// Converts a higher-half direct-map virtual address into physical RAM.
pub fn direct_map_physical(address: VirtualAddress) -> Result<PhysicalAddress> {
    let value = address.get();
    if !(DIRECT_MAP_BASE..DIRECT_MAP_BASE + RAM_SIZE).contains(&value) {
        return Err(PlatformError::AddressOutsideRam(value));
    }
    Ok(PhysicalAddress::new(value - DIRECT_MAP_OFFSET))
}
