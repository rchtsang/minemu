use minemu_platform::{BOOT_INFO_SIZE, BootInfo, MemRegion, PhysicalAddress, VirtualAddress};

use crate::{Result, SystemImage};

/// A boot-ROM action plan derived from a validated system-ROM image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootPlan {
    pub copies: Vec<BootCopy>,
    pub boot_info: [u8; BOOT_INFO_SIZE],
    pub boot_info_paddr: PhysicalAddress,
    pub bootstrap_entry_paddr: PhysicalAddress,
    pub boot_info_vaddr: VirtualAddress,
    pub kernel_entry_vaddr: VirtualAddress,
}

/// One initialized kernel copy plus its trailing BSS zeroing requirement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootCopy {
    pub physical_address: PhysicalAddress,
    pub bytes: Vec<u8>,
    pub zeroed_length: u32,
}

impl SystemImage {
    /// Produces the data movements and handoff values performed by boot ROM.
    pub fn boot_plan(&self) -> Result<BootPlan> {
        let mut copies = Vec::with_capacity(self.kernel_segments.len());
        for segment in &self.kernel_segments {
            let start = segment.data_offset as usize;
            let end = start + segment.file_size as usize;
            copies.push(BootCopy {
                physical_address: segment.physical_address,
                bytes: self.bytes[start..end].to_vec(),
                zeroed_length: segment.memory_size - segment.file_size,
            });
        }
        let boot_info = BootInfo {
            system_rom_base: MemRegion::SystemRom.base(),
            image_size: self.header.image_size,
            module_table_offset: self.header.module_table_offset,
            module_count: self.header.module_count,
        }
        .encode()?;
        Ok(BootPlan {
            copies,
            boot_info,
            boot_info_paddr: self.header.boot_info_paddr,
            bootstrap_entry_paddr: self.header.bootstrap_entry_paddr,
            boot_info_vaddr: VirtualAddress::new(0xc000_7000),
            kernel_entry_vaddr: self.header.kernel_entry_vaddr,
        })
    }
}

impl BootPlan {
    /// Applies the boot ROM's kernel copies, BSS clearing, and boot-info write.
    /// The caller performs the subsequent branch to `bootstrap_entry_paddr`.
    pub fn apply<E>(
        &self,
        mut write: impl FnMut(PhysicalAddress, &[u8]) -> std::result::Result<(), E>,
    ) -> std::result::Result<(), E> {
        const ZERO_PAGE: [u8; 4096] = [0; 4096];

        for copy in &self.copies {
            write(copy.physical_address, &copy.bytes)?;
            let mut address = copy.physical_address.get() + copy.bytes.len() as u32;
            let mut remaining = copy.zeroed_length as usize;
            while remaining != 0 {
                let count = remaining.min(ZERO_PAGE.len());
                write(PhysicalAddress::new(address), &ZERO_PAGE[..count])?;
                address += count as u32;
                remaining -= count;
            }
        }
        write(self.boot_info_paddr, &self.boot_info)
    }
}
