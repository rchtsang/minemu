use crate::{
    BOOT_INFO_PADDR, BOOTSTRAP_ENTRY_PADDR, MemRegion, PhysicalAddress, PhysicalRange,
    PlatformError, Result, VirtualAddress, direct_map_physical,
};

/// ABI version for system-ROM images and boot-info records.
pub const ABI_VERSION: u16 = 1;
/// System-ROM image magic, encoded as `MEMU` in big-endian notation.
pub const IMAGE_MAGIC: u32 = 0x4d45_4d55;
/// Boot-info magic, encoded as `MBOO` in big-endian notation.
pub const BOOT_INFO_MAGIC: u32 = 0x4d42_4f4f;
/// Serialized image-header size.
pub const IMAGE_HEADER_SIZE: usize = 64;
/// Serialized kernel-segment size.
pub const KERNEL_SEGMENT_SIZE: usize = 32;
/// Serialized module-record size.
pub const MODULE_RECORD_SIZE: usize = 32;
/// Serialized module-segment size.
pub const MODULE_SEGMENT_SIZE: usize = 32;
/// Serialized boot-info size.
pub const BOOT_INFO_SIZE: usize = 64;

const SEGMENT_FLAGS_MASK: u32 = 0x7;

/// Fixed-width system-ROM image header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageHeader {
    pub image_size: u32,
    pub kernel_segment_table_offset: u32,
    pub kernel_segment_count: u32,
    pub module_table_offset: u32,
    pub module_count: u32,
    pub bootstrap_entry_paddr: PhysicalAddress,
    pub kernel_entry_vaddr: VirtualAddress,
    pub boot_info_paddr: PhysicalAddress,
}

impl ImageHeader {
    /// Decodes and validates a fixed-width image header.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        require_length(bytes, IMAGE_HEADER_SIZE)?;
        if read_u32(bytes, 0)? != IMAGE_MAGIC {
            return Err(PlatformError::InvalidImageField("magic"));
        }
        if read_u16(bytes, 4)? != ABI_VERSION {
            return Err(PlatformError::InvalidImageField("version"));
        }
        if read_u16(bytes, 6)? != IMAGE_HEADER_SIZE as u16 {
            return Err(PlatformError::InvalidImageField("header size"));
        }
        if read_u32(bytes, 40)? != 0 || !reserved_zero(bytes, 44, 20)? {
            return Err(PlatformError::InvalidImageField("reserved header bits"));
        }
        let header = Self {
            image_size: read_u32(bytes, 8)?,
            kernel_segment_table_offset: read_u32(bytes, 12)?,
            kernel_segment_count: read_u32(bytes, 16)?,
            module_table_offset: read_u32(bytes, 20)?,
            module_count: read_u32(bytes, 24)?,
            bootstrap_entry_paddr: PhysicalAddress::new(read_u32(bytes, 28)?),
            kernel_entry_vaddr: VirtualAddress::new(read_u32(bytes, 32)?),
            boot_info_paddr: PhysicalAddress::new(read_u32(bytes, 36)?),
        };
        header.validate()?;
        Ok(header)
    }

    /// Encodes this header without relying on Rust struct layout.
    pub fn encode(self) -> Result<[u8; IMAGE_HEADER_SIZE]> {
        self.validate()?;
        let mut bytes = [0; IMAGE_HEADER_SIZE];
        write_u32(&mut bytes, 0, IMAGE_MAGIC);
        write_u16(&mut bytes, 4, ABI_VERSION);
        write_u16(&mut bytes, 6, IMAGE_HEADER_SIZE as u16);
        write_u32(&mut bytes, 8, self.image_size);
        write_u32(&mut bytes, 12, self.kernel_segment_table_offset);
        write_u32(&mut bytes, 16, self.kernel_segment_count);
        write_u32(&mut bytes, 20, self.module_table_offset);
        write_u32(&mut bytes, 24, self.module_count);
        write_u32(&mut bytes, 28, self.bootstrap_entry_paddr.get());
        write_u32(&mut bytes, 32, self.kernel_entry_vaddr.get());
        write_u32(&mut bytes, 36, self.boot_info_paddr.get());
        Ok(bytes)
    }

    /// Validates static header fields and table bounds.
    pub fn validate(self) -> Result<()> {
        if self.image_size < IMAGE_HEADER_SIZE as u32
            || self.image_size > MemRegion::SystemRom.size()
        {
            return Err(PlatformError::InvalidImageField("image size"));
        }
        if self.bootstrap_entry_paddr.get() != BOOTSTRAP_ENTRY_PADDR {
            return Err(PlatformError::InvalidImageField("bootstrap entry"));
        }
        if self.boot_info_paddr.get() != BOOT_INFO_PADDR {
            return Err(PlatformError::InvalidImageField("boot info address"));
        }
        direct_map_physical(self.kernel_entry_vaddr)?;
        validate_table(
            self.kernel_segment_table_offset,
            self.kernel_segment_count,
            KERNEL_SEGMENT_SIZE,
            self.image_size,
        )?;
        validate_table(
            self.module_table_offset,
            self.module_count,
            MODULE_RECORD_SIZE,
            self.image_size,
        )
    }
}

/// A kernel segment record stored in system ROM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSegment {
    pub data_offset: u32,
    pub physical_address: PhysicalAddress,
    pub virtual_address: VirtualAddress,
    pub file_size: u32,
    pub memory_size: u32,
    pub flags: u32,
}

impl KernelSegment {
    /// Decodes and validates one kernel-segment record.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        require_length(bytes, KERNEL_SEGMENT_SIZE)?;
        if !reserved_zero(bytes, 24, 8)? {
            return Err(PlatformError::InvalidImageField(
                "kernel segment reserved bits",
            ));
        }
        let segment = Self {
            data_offset: read_u32(bytes, 0)?,
            physical_address: PhysicalAddress::new(read_u32(bytes, 4)?),
            virtual_address: VirtualAddress::new(read_u32(bytes, 8)?),
            file_size: read_u32(bytes, 12)?,
            memory_size: read_u32(bytes, 16)?,
            flags: read_u32(bytes, 20)?,
        };
        segment.validate()?;
        Ok(segment)
    }

    /// Encodes this record without relying on Rust struct layout.
    pub fn encode(self) -> Result<[u8; KERNEL_SEGMENT_SIZE]> {
        self.validate()?;
        let mut bytes = [0; KERNEL_SEGMENT_SIZE];
        write_u32(&mut bytes, 0, self.data_offset);
        write_u32(&mut bytes, 4, self.physical_address.get());
        write_u32(&mut bytes, 8, self.virtual_address.get());
        write_u32(&mut bytes, 12, self.file_size);
        write_u32(&mut bytes, 16, self.memory_size);
        write_u32(&mut bytes, 20, self.flags);
        Ok(bytes)
    }

    /// Validates segment sizes and flags.
    pub fn validate(self) -> Result<()> {
        if self.memory_size == 0 || self.file_size > self.memory_size {
            return Err(PlatformError::InvalidImageField("kernel segment size"));
        }
        if self.flags & !SEGMENT_FLAGS_MASK != 0 {
            return Err(PlatformError::InvalidImageField("kernel segment flags"));
        }
        let physical_range = PhysicalRange::new(self.physical_address, self.memory_size)?;
        if !MemRegion::Ram.range().contains_range(physical_range) {
            return Err(PlatformError::InvalidImageField(
                "kernel segment physical range",
            ));
        }
        let high_half_matches = matches!(
            direct_map_physical(self.virtual_address),
            Ok(physical) if physical == self.physical_address
        );
        if self.virtual_address.get() != self.physical_address.get() && !high_half_matches {
            return Err(PlatformError::InvalidImageField(
                "kernel segment virtual mapping",
            ));
        }
        Ok(())
    }
}

/// A fixed-address user-module record stored in system ROM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRecord {
    pub name_offset: u32,
    pub name_length: u32,
    pub segment_table_offset: u32,
    pub segment_count: u32,
    pub entry_virtual_address: VirtualAddress,
}

impl ModuleRecord {
    /// Decodes and validates one module record.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        require_length(bytes, MODULE_RECORD_SIZE)?;
        if read_u32(bytes, 20)? != 0 || !reserved_zero(bytes, 24, 8)? {
            return Err(PlatformError::InvalidImageField("module reserved bits"));
        }
        Ok(Self {
            name_offset: read_u32(bytes, 0)?,
            name_length: read_u32(bytes, 4)?,
            segment_table_offset: read_u32(bytes, 8)?,
            segment_count: read_u32(bytes, 12)?,
            entry_virtual_address: VirtualAddress::new(read_u32(bytes, 16)?),
        })
    }

    /// Encodes this record without relying on Rust struct layout.
    pub fn encode(self) -> [u8; MODULE_RECORD_SIZE] {
        let mut bytes = [0; MODULE_RECORD_SIZE];
        write_u32(&mut bytes, 0, self.name_offset);
        write_u32(&mut bytes, 4, self.name_length);
        write_u32(&mut bytes, 8, self.segment_table_offset);
        write_u32(&mut bytes, 12, self.segment_count);
        write_u32(&mut bytes, 16, self.entry_virtual_address.get());
        bytes
    }
}

/// A module segment record whose physical frame is chosen by the kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleSegment {
    pub data_offset: u32,
    pub virtual_address: VirtualAddress,
    pub file_size: u32,
    pub memory_size: u32,
    pub flags: u32,
}

impl ModuleSegment {
    /// Decodes and validates one module-segment record.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        require_length(bytes, MODULE_SEGMENT_SIZE)?;
        if !reserved_zero(bytes, 20, 12)? {
            return Err(PlatformError::InvalidImageField(
                "module segment reserved bits",
            ));
        }
        let segment = Self {
            data_offset: read_u32(bytes, 0)?,
            virtual_address: VirtualAddress::new(read_u32(bytes, 4)?),
            file_size: read_u32(bytes, 8)?,
            memory_size: read_u32(bytes, 12)?,
            flags: read_u32(bytes, 16)?,
        };
        segment.validate()?;
        Ok(segment)
    }

    /// Encodes this record without relying on Rust struct layout.
    pub fn encode(self) -> Result<[u8; MODULE_SEGMENT_SIZE]> {
        self.validate()?;
        let mut bytes = [0; MODULE_SEGMENT_SIZE];
        write_u32(&mut bytes, 0, self.data_offset);
        write_u32(&mut bytes, 4, self.virtual_address.get());
        write_u32(&mut bytes, 8, self.file_size);
        write_u32(&mut bytes, 12, self.memory_size);
        write_u32(&mut bytes, 16, self.flags);
        Ok(bytes)
    }

    /// Validates segment sizes and flags.
    pub fn validate(self) -> Result<()> {
        if self.memory_size == 0
            || self.file_size > self.memory_size
            || self.flags & !SEGMENT_FLAGS_MASK != 0
        {
            return Err(PlatformError::InvalidImageField("module segment"));
        }
        Ok(())
    }
}

/// The fixed-width boot-info record written by boot ROM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootInfo {
    pub system_rom_base: PhysicalAddress,
    pub image_size: u32,
    pub module_table_offset: u32,
    pub module_count: u32,
}

impl BootInfo {
    /// Decodes and validates boot info.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        require_length(bytes, BOOT_INFO_SIZE)?;
        if read_u32(bytes, 0)? != BOOT_INFO_MAGIC
            || read_u16(bytes, 4)? != ABI_VERSION
            || read_u16(bytes, 6)? != BOOT_INFO_SIZE as u16
            || read_u32(bytes, 36)? != 0
            || !reserved_zero(bytes, 40, 24)?
        {
            return Err(PlatformError::InvalidImageField("boot info header"));
        }
        let info = Self {
            system_rom_base: PhysicalAddress::new(read_u32(bytes, 8)?),
            image_size: read_u32(bytes, 12)?,
            module_table_offset: read_u32(bytes, 16)?,
            module_count: read_u32(bytes, 20)?,
        };
        if info.system_rom_base != MemRegion::SystemRom.base()
            || read_u32(bytes, 24)? != 0xc000_0000
            || read_u32(bytes, 28)? != MemRegion::Ram.base().get()
            || read_u32(bytes, 32)? != MemRegion::Ram.size()
        {
            return Err(PlatformError::InvalidImageField("boot info mapping"));
        }
        Ok(info)
    }

    /// Encodes boot info without relying on Rust struct layout.
    pub fn encode(self) -> Result<[u8; BOOT_INFO_SIZE]> {
        if self.system_rom_base != MemRegion::SystemRom.base()
            || self.image_size > MemRegion::SystemRom.size()
        {
            return Err(PlatformError::InvalidImageField("boot info"));
        }
        let mut bytes = [0; BOOT_INFO_SIZE];
        write_u32(&mut bytes, 0, BOOT_INFO_MAGIC);
        write_u16(&mut bytes, 4, ABI_VERSION);
        write_u16(&mut bytes, 6, BOOT_INFO_SIZE as u16);
        write_u32(&mut bytes, 8, self.system_rom_base.get());
        write_u32(&mut bytes, 12, self.image_size);
        write_u32(&mut bytes, 16, self.module_table_offset);
        write_u32(&mut bytes, 20, self.module_count);
        write_u32(&mut bytes, 24, 0xc000_0000);
        write_u32(&mut bytes, 28, MemRegion::Ram.base().get());
        write_u32(&mut bytes, 32, MemRegion::Ram.size());
        Ok(bytes)
    }
}

fn require_length(bytes: &[u8], expected: usize) -> Result<()> {
    if bytes.len() < expected {
        return Err(PlatformError::InvalidImageLength {
            expected,
            actual: bytes.len(),
        });
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset + 2;
    let slice = bytes
        .get(offset..end)
        .ok_or(PlatformError::InvalidImageLength {
            expected: end,
            actual: bytes.len(),
        })?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset + 4;
    let slice = bytes
        .get(offset..end)
        .ok_or(PlatformError::InvalidImageLength {
            expected: end,
            actual: bytes.len(),
        })?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn reserved_zero(bytes: &[u8], offset: usize, length: usize) -> Result<bool> {
    let end = offset + length;
    let slice = bytes
        .get(offset..end)
        .ok_or(PlatformError::InvalidImageLength {
            expected: end,
            actual: bytes.len(),
        })?;
    Ok(slice.iter().all(|byte| *byte == 0))
}

fn validate_table(offset: u32, count: u32, record_size: usize, image_size: u32) -> Result<()> {
    let end = u64::from(offset) + u64::from(count) * record_size as u64;
    if end > u64::from(image_size) {
        return Err(PlatformError::ImageTableOutsideImage);
    }
    Ok(())
}
