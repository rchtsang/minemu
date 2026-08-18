use std::cell::UnsafeCell;

use minemu_platform::{
    BOOT_ROM_SIZE, MemRegion, PhysicalAddress, PhysicalRange, RAM_SIZE, SYSTEM_ROM_SIZE,
};

use crate::{CoreError, Result};

/// Narrow physical-memory interface used by the MMU walker and DMA device.
pub trait PhysicalMemoryAccess {
    fn read_u8(&self, address: PhysicalAddress) -> Result<u8>;
    fn read_u32(&self, address: PhysicalAddress) -> Result<u32>;
    fn write_u8(&mut self, address: PhysicalAddress, value: u8) -> Result<()>;
    fn write_u32(&mut self, address: PhysicalAddress, value: u32) -> Result<()>;
    fn read_range(&self, range: PhysicalRange, destination: &mut [u8]) -> Result<()>;
    fn write_range(&mut self, address: PhysicalAddress, bytes: &[u8]) -> Result<()>;
}

/// Immutable boot/system ROM and mutable RAM backing for the physical map.
pub struct PhysicalMemory {
    boot_rom: Box<[u8; BOOT_ROM_SIZE as usize]>,
    system_rom: Box<[u8; SYSTEM_ROM_SIZE as usize]>,
    ram: UnsafeCell<Box<[u8; RAM_SIZE as usize]>>,
}

impl PhysicalMemory {
    /// Creates zero-filled ROM images and RAM with fixed ABI sizes.
    pub fn empty() -> Self {
        Self {
            boot_rom: zeroed(),
            system_rom: zeroed(),
            ram: UnsafeCell::new(zeroed()),
        }
    }

    /// Creates memory after copying supplied boot and system ROM contents.
    pub fn with_roms(boot_rom: &[u8], system_rom: &[u8]) -> Result<Self> {
        if boot_rom.len() > MemRegion::BootRom.size() as usize {
            return Err(CoreError::PhysicalAccessOutOfBounds {
                address: MemRegion::BootRom.base().get(),
                needed: boot_rom.len(),
            });
        }
        if system_rom.len() > MemRegion::SystemRom.size() as usize {
            return Err(CoreError::PhysicalAccessOutOfBounds {
                address: MemRegion::SystemRom.base().get(),
                needed: system_rom.len(),
            });
        }
        let mut memory = Self::empty();
        memory.boot_rom[..boot_rom.len()].copy_from_slice(boot_rom);
        memory.system_rom[..system_rom.len()].copy_from_slice(system_rom);
        Ok(memory)
    }

    /// Returns a read-only view of the current RAM image.
    pub fn ram(&self) -> &[u8] {
        // RAM may also be mapped into a single-threaded CPU backend. Callers
        // only inspect it while guest execution is stopped.
        unsafe { &(**self.ram.get())[..] }
    }

    /// Returns the stable RAM allocation used by an in-process CPU backend.
    pub fn ram_mut_ptr(&mut self) -> *mut u8 {
        self.ram.get_mut().as_mut_ptr()
    }

    /// Borrows one mapped physical range for synchronous zero-copy inspection.
    pub fn inspect_range(&self, range: PhysicalRange) -> Result<&[u8]> {
        self.region_slice(range.start(), range.length() as usize)
    }

    fn region_slice(&self, address: PhysicalAddress, length: usize) -> Result<&[u8]> {
        let (region, offset) = self.region_offset(address)?;
        let storage: &[u8] = match region {
            MemRegion::BootRom => &self.boot_rom[..],
            MemRegion::SystemRom => &self.system_rom[..],
            MemRegion::Ram => self.ram(),
            _ => return Err(CoreError::UnmappedPhysicalAddress(address.get())),
        };
        storage.get(offset..offset.saturating_add(length)).ok_or(
            CoreError::PhysicalAccessOutOfBounds {
                address: address.get(),
                needed: length,
            },
        )
    }

    fn region_slice_mut(&mut self, address: PhysicalAddress, length: usize) -> Result<&mut [u8]> {
        let (region, offset) = self.region_offset(address)?;
        if region != MemRegion::Ram {
            return Err(CoreError::ImmutablePhysicalAddress(address.get()));
        }
        self.ram
            .get_mut()
            .get_mut(offset..offset.saturating_add(length))
            .ok_or(CoreError::PhysicalAccessOutOfBounds {
                address: address.get(),
                needed: length,
            })
    }

    fn region_offset(&self, address: PhysicalAddress) -> Result<(MemRegion, usize)> {
        let region = Option::<MemRegion>::from(address)
            .ok_or(CoreError::UnmappedPhysicalAddress(address.get()))?;
        Ok((region, (address.get() - region.base().get()) as usize))
    }
}

impl Default for PhysicalMemory {
    fn default() -> Self {
        Self::empty()
    }
}

impl PhysicalMemoryAccess for PhysicalMemory {
    fn read_u8(&self, address: PhysicalAddress) -> Result<u8> {
        Ok(self.region_slice(address, 1)?[0])
    }

    fn read_u32(&self, address: PhysicalAddress) -> Result<u32> {
        let bytes = self.region_slice(address, 4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("checked length"),
        ))
    }

    fn write_u8(&mut self, address: PhysicalAddress, value: u8) -> Result<()> {
        self.region_slice_mut(address, 1)?[0] = value;
        Ok(())
    }

    fn write_u32(&mut self, address: PhysicalAddress, value: u32) -> Result<()> {
        self.region_slice_mut(address, 4)?
            .copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn read_range(&self, range: PhysicalRange, destination: &mut [u8]) -> Result<()> {
        if destination.len() != range.length() as usize {
            return Err(CoreError::PhysicalAccessOutOfBounds {
                address: range.start().get(),
                needed: destination.len(),
            });
        }
        destination.copy_from_slice(self.region_slice(range.start(), destination.len())?);
        Ok(())
    }

    fn write_range(&mut self, address: PhysicalAddress, bytes: &[u8]) -> Result<()> {
        self.region_slice_mut(address, bytes.len())
            .map(|slice| slice.copy_from_slice(bytes))
    }
}

fn zeroed<const N: usize>() -> Box<[u8; N]> {
    vec![0; N]
        .into_boxed_slice()
        .try_into()
        .expect("fixed-size vector has the requested length")
}

#[cfg(test)]
mod tests {
    use minemu_platform::{MemRegion, PhysicalAddress};

    use super::{PhysicalMemory, PhysicalMemoryAccess};

    #[test]
    fn rom_is_immutable_and_ram_is_little_endian() {
        let mut memory = PhysicalMemory::with_roms(&[1], &[2]).unwrap();
        assert_eq!(memory.read_u8(MemRegion::BootRom.base()).unwrap(), 1);
        assert!(memory.write_u8(MemRegion::BootRom.base(), 0).is_err());
        let address = PhysicalAddress::new(MemRegion::Ram.base().get() + 4);
        memory.write_u32(address, 0x4433_2211).unwrap();
        assert_eq!(memory.read_u32(address).unwrap(), 0x4433_2211);
    }

    #[test]
    fn inspection_borrows_the_requested_range() {
        let memory = PhysicalMemory::with_roms(&[1, 2], &[]).unwrap();
        let range = minemu_platform::PhysicalRange::new(MemRegion::BootRom.base(), 2).unwrap();
        assert_eq!(memory.inspect_range(range).unwrap(), &[1, 2]);
    }
}
