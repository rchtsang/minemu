use crate::{PlatformError, Result};

/// A 32-bit physical machine address.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhysicalAddress(u32);

impl PhysicalAddress {
    /// Creates an address from its ABI representation.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the ABI representation.
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Returns this address rounded down to its containing 4 KiB page.
    pub const fn page_base(self) -> Self {
        Self(self.0 & !0xfff)
    }

    /// Returns whether this address is 4 KiB aligned.
    pub const fn is_page_aligned(self) -> bool {
        self.0 & 0xfff == 0
    }
}

/// A 32-bit virtual machine address.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VirtualAddress(u32);

impl VirtualAddress {
    /// Creates an address from its ABI representation.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the ABI representation.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A nonempty physical range with checked 32-bit bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalRange {
    start: PhysicalAddress,
    length: u32,
}

impl PhysicalRange {
    /// Creates a checked nonempty physical range.
    pub fn new(start: PhysicalAddress, length: u32) -> Result<Self> {
        if length == 0 {
            return Err(PlatformError::EmptyRange);
        }
        if u64::from(start.get()) + u64::from(length) > (u64::from(u32::MAX) + 1) {
            return Err(PlatformError::AddressRangeOverflow {
                start: start.get(),
                length,
            });
        }
        Ok(Self { start, length })
    }

    pub(crate) const fn from_known_valid(start: PhysicalAddress, length: u32) -> Self {
        Self { start, length }
    }

    /// Returns the first address in this range.
    pub const fn start(self) -> PhysicalAddress {
        self.start
    }

    /// Returns the range length in bytes.
    pub const fn length(self) -> u32 {
        self.length
    }

    /// Returns the exclusive range end, which may equal `2^32`.
    pub const fn end_exclusive(self) -> u64 {
        self.start.get() as u64 + self.length as u64
    }

    /// Returns whether this range contains an address.
    pub const fn contains_address(self, address: PhysicalAddress) -> bool {
        (address.get() as u64) >= self.start.get() as u64
            && (address.get() as u64) < self.end_exclusive()
    }

    /// Returns whether this range fully contains another checked range.
    pub const fn contains_range(self, range: Self) -> bool {
        (range.start.get() as u64) >= self.start.get() as u64
            && range.end_exclusive() <= self.end_exclusive()
    }
}
