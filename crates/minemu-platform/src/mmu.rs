use crate::{
    Access, MemRegion, Permissions, PhysicalAddress, PhysicalRange, PlatformError, Result,
    VirtualAddress,
};

/// Valid PTE and PDE bit.
pub const PTE_VALID: u32 = 1 << 0;
/// Writable PTE bit.
pub const PTE_WRITABLE: u32 = Permissions::WRITE.bits() as u32;
/// User-accessible PTE bit.
pub const PTE_USER: u32 = Permissions::USER.bits() as u32;
/// Executable PTE bit.
pub const PTE_EXECUTABLE: u32 = Permissions::EXECUTE.bits() as u32;
/// Readable PTE bit.
pub const PTE_READABLE: u32 = Permissions::READ.bits() as u32;
/// MMU-managed successful-access bit.
pub const PTE_ACCESSED: u32 = 1 << 5;
/// MMU-managed successful-write bit.
pub const PTE_DIRTY: u32 = 1 << 6;
/// Kernel-owned replacement-policy metadata bits.
pub const PTE_SOFTWARE_MASK: u32 = 0x0000_0f80;

/// PTE and PDE physical page-address bits.
pub const PTE_PAGE_MASK: u32 = 0xffff_f000;
/// PTE access-permission bits.
pub const PTE_PERMISSION_MASK: u32 = PTE_WRITABLE | PTE_USER | PTE_EXECUTABLE | PTE_READABLE;
const PTE_SOFTWARE_SHIFT: u32 = 7;
const PDE_ALLOWED_MASK: u32 = PTE_VALID | PTE_PAGE_MASK;

/// A supported CP15 operation after instruction decoding and privilege checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cp15Operation {
    /// Install a 4 KiB-aligned physical RAM page directory.
    SetTtbr0(PhysicalAddress),
    /// Enable or disable MMU translation through SCTLR.M.
    SetMmuEnabled(bool),
    /// Invalidate all cached MMU translations.
    InvalidateAll,
    /// Set the 32-byte-aligned exception vector base.
    SetVectorBase(VirtualAddress),
    /// Read most recent fault status through DFSR.
    ReadFaultStatus,
    /// Read most recent fault address through DFAR.
    ReadFaultAddress,
}

impl Cp15Operation {
    /// Validates a TTBR0 value and produces its CP15 operation.
    pub fn set_ttbr0(value: u32) -> Result<Self> {
        let address = PhysicalAddress::new(value);
        validate_page_table_page(address)?;
        Ok(Self::SetTtbr0(address))
    }

    /// Produces the platform-defined SCTLR operation from a raw register value.
    pub const fn set_mmu_enabled(value: u32) -> Self {
        Self::SetMmuEnabled(value & 1 != 0)
    }

    /// Validates a VBAR value and produces its CP15 operation.
    pub fn set_vector_base(value: u32) -> Result<Self> {
        if value & 0x1f != 0 {
            return Err(PlatformError::UnalignedVectorBase(value));
        }
        Ok(Self::SetVectorBase(VirtualAddress::new(value)))
    }
}

/// Validates a physical RAM page used as a page directory or page table.
pub fn validate_page_table_page(address: PhysicalAddress) -> Result<()> {
    if !address.is_page_aligned() {
        return Err(PlatformError::UnalignedPage(address.get()));
    }
    let page = PhysicalRange::new(address, crate::PAGE_SIZE)?;
    if !MemRegion::Ram.range().contains_range(page) {
        return Err(PlatformError::AddressOutsideRam(address.get()));
    }
    Ok(())
}

/// Returns whether an aligned physical page can be a valid MMU mapping target.
pub fn is_mmu_target_page(address: PhysicalAddress) -> bool {
    if !address.is_page_aligned() {
        return false;
    }
    let Ok(page) = PhysicalRange::new(address, crate::PAGE_SIZE) else {
        return false;
    };
    let Some(region) = Option::<MemRegion>::from(address) else {
        return false;
    };
    region.range().contains_range(page)
}

/// A raw page-directory entry with ABI validation helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageDirectoryEntry(u32);

impl PageDirectoryEntry {
    /// Creates an entry after rejecting reserved bits.
    pub fn new(raw: u32) -> Result<Self> {
        if raw & !PDE_ALLOWED_MASK != 0 {
            return Err(PlatformError::InvalidPageDirectoryBits(raw));
        }
        Ok(Self(raw))
    }

    /// Returns the ABI-encoded entry.
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns whether this entry is valid.
    pub const fn is_valid(self) -> bool {
        self.0 & PTE_VALID != 0
    }

    /// Returns and validates the page-table physical address when valid.
    pub fn table_address(self) -> Result<Option<PhysicalAddress>> {
        if !self.is_valid() {
            return Ok(None);
        }
        let address = PhysicalAddress::new(self.0 & PTE_PAGE_MASK);
        validate_page_table_page(address)?;
        Ok(Some(address))
    }
}

/// A raw page-table entry with ABI validation and access-bit helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageTableEntry(u32);

impl PageTableEntry {
    /// Creates an entry from its ABI-encoded value.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the ABI-encoded entry.
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns whether this entry is valid.
    pub const fn is_valid(self) -> bool {
        self.0 & PTE_VALID != 0
    }

    /// Returns permissions represented by this entry.
    pub const fn permissions(self) -> Permissions {
        Permissions::from_bits_retain((self.0 & PTE_PERMISSION_MASK) as u8)
    }

    /// Returns the kernel-owned five-bit software metadata field.
    pub const fn software_metadata(self) -> u8 {
        ((self.0 & PTE_SOFTWARE_MASK) >> PTE_SOFTWARE_SHIFT) as u8
    }

    /// Replaces only the kernel-owned software metadata bits.
    pub const fn with_software_metadata(self, metadata: u8) -> Self {
        Self(
            (self.0 & !PTE_SOFTWARE_MASK)
                | (((metadata as u32) << PTE_SOFTWARE_SHIFT) & PTE_SOFTWARE_MASK),
        )
    }

    /// Returns the encoded physical page address without validating it.
    pub const fn page_address(self) -> PhysicalAddress {
        PhysicalAddress::new(self.0 & PTE_PAGE_MASK)
    }

    /// Returns and validates the physical target page when valid.
    pub fn target_address(self) -> Result<Option<PhysicalAddress>> {
        if !self.is_valid() {
            return Ok(None);
        }
        let address = self.page_address();
        if !is_mmu_target_page(address) {
            return Err(PlatformError::InvalidPageTableTarget(address.get()));
        }
        if Option::<MemRegion>::from(address).is_some_and(MemRegion::is_device)
            && self.permissions().contains(Permissions::USER)
        {
            return Err(PlatformError::UserDeviceMapping(address.get()));
        }
        Ok(Some(address))
    }

    /// Returns the failure cause for an attempted access, if any.
    pub const fn authorize(
        self,
        access: Access,
        user_mode: bool,
    ) -> std::result::Result<(), FaultCause> {
        if !self.is_valid() {
            return Err(FaultCause::Translation);
        }
        if self.permissions().permits(access, user_mode) {
            return Ok(());
        }
        Err(FaultCause::protection(access))
    }

    /// Sets MMU-managed access metadata after a successful access.
    pub const fn record_successful_access(self, access: Access) -> Self {
        let mut raw = self.0 | PTE_ACCESSED;
        if matches!(access, Access::Write) {
            raw |= PTE_DIRTY;
        }
        Self(raw)
    }

    /// Clears MMU-managed Accessed and Dirty metadata for a new sampling epoch.
    pub const fn clear_access_metadata(self) -> Self {
        Self(self.0 & !(PTE_ACCESSED | PTE_DIRTY))
    }
}

/// ABI fault causes encoded in the low byte of DFSR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FaultCause {
    Translation = 1,
    ReadProtection = 2,
    WriteProtection = 3,
    ExecuteProtection = 4,
    DeviceAccess = 5,
}

impl FaultCause {
    /// Returns the protection fault for a denied access.
    pub const fn protection(access: Access) -> Self {
        match access {
            Access::Fetch => Self::ExecuteProtection,
            Access::Read => Self::ReadProtection,
            Access::Write => Self::WriteProtection,
        }
    }

    const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            1 => Some(Self::Translation),
            2 => Some(Self::ReadProtection),
            3 => Some(Self::WriteProtection),
            4 => Some(Self::ExecuteProtection),
            5 => Some(Self::DeviceAccess),
            _ => None,
        }
    }
}

const FAULT_CAUSE_MASK: u32 = 0xff;
const FAULT_USER: u32 = 1 << 8;
const FAULT_WRITE: u32 = 1 << 9;
const FAULT_FETCH: u32 = 1 << 10;
const FAULT_ALLOWED_MASK: u32 = 0x0f | FAULT_USER | FAULT_WRITE | FAULT_FETCH;

/// Encoded fault status exposed through DFSR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FaultStatus(u32);

impl FaultStatus {
    /// Encodes a fault cause and attempted-access metadata.
    pub const fn new(cause: FaultCause, user_mode: bool, access: Access) -> Self {
        let mut raw = cause as u32;
        if user_mode {
            raw |= FAULT_USER;
        }
        match access {
            Access::Write => raw |= FAULT_WRITE,
            Access::Fetch => raw |= FAULT_FETCH,
            Access::Read => {}
        }
        Self(raw)
    }

    /// Decodes a fault status after checking cause and reserved bits.
    pub fn from_raw(raw: u32) -> Result<Self> {
        if raw & !FAULT_ALLOWED_MASK != 0 || Self(raw).cause().is_none() {
            return Err(PlatformError::InvalidFaultStatus(raw));
        }
        Ok(Self(raw))
    }

    /// Returns the ABI-encoded status word.
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns the encoded fault cause.
    pub const fn cause(self) -> Option<FaultCause> {
        FaultCause::from_raw(self.0 & FAULT_CAUSE_MASK)
    }

    /// Returns whether the faulting access came from USR mode.
    pub const fn from_user(self) -> bool {
        self.0 & FAULT_USER != 0
    }

    /// Returns whether the faulting access was a write.
    pub const fn is_write(self) -> bool {
        self.0 & FAULT_WRITE != 0
    }

    /// Returns whether the faulting access was an instruction fetch.
    pub const fn is_fetch(self) -> bool {
        self.0 & FAULT_FETCH != 0
    }
}
