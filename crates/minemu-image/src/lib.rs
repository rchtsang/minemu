//! Host-side packing and validation for versioned system-ROM images.

use std::collections::BTreeSet;

use minemu_platform::{
    BOOT_INFO_SIZE, BootInfo, IMAGE_HEADER_SIZE, ImageHeader, KERNEL_SEGMENT_SIZE, KernelSegment,
    MODULE_RECORD_SIZE, MODULE_SEGMENT_SIZE, MemRegion, ModuleRecord, ModuleSegment,
    PhysicalAddress, PhysicalRange, PlatformError, VirtualAddress, direct_map_physical,
};
use object::{
    Architecture, BinaryFormat, Object, ObjectKind, ObjectSection, ObjectSegment, Permissions,
};
use thiserror::Error;

/// Errors from parsing ELF inputs or the system-ROM wire format.
#[derive(Debug, Error)]
pub enum ImageError {
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error("unable to parse ELF input: {0}")]
    Elf(#[from] object::Error),
    #[error("invalid ELF input: {0}")]
    InvalidElf(&'static str),
    #[error("invalid system image: {0}")]
    InvalidImage(&'static str),
    #[error("system ROM image exceeds its 16 MiB capacity")]
    RomOverflow,
}

/// Result type returned by this crate.
pub type Result<T> = std::result::Result<T, ImageError>;

/// One independently linked fixed-address user ELF supplied to the packer.
#[derive(Clone, Copy, Debug)]
pub struct ModuleInput<'a> {
    pub name: &'a str,
    pub elf: &'a [u8],
}

/// A canonical system-ROM image produced by [`ImageBuilder`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemImage {
    bytes: Vec<u8>,
    header: ImageHeader,
    kernel_segments: Vec<KernelSegment>,
    modules: Vec<ImageModule>,
}

/// A parsed user module and its table record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageModule {
    pub name: String,
    pub record: ModuleRecord,
    pub segments: Vec<ModuleSegment>,
}

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

/// Canonically packages a kernel ELF and zero or more user-module ELFs.
pub struct ImageBuilder<'a> {
    kernel: &'a [u8],
    modules: Vec<ModuleInput<'a>>,
}

impl<'a> ImageBuilder<'a> {
    pub fn new(kernel: &'a [u8]) -> Self {
        Self {
            kernel,
            modules: Vec::new(),
        }
    }

    pub fn add_module(mut self, module: ModuleInput<'a>) -> Self {
        self.modules.push(module);
        self
    }

    pub fn build(self) -> Result<SystemImage> {
        let (kernel_entry, mut kernel_segments) = parse_elf(self.kernel)?;
        let kernel_entry = address32(kernel_entry, "kernel entry")?;
        if kernel_entry & 1 != 0 {
            return Err(ImageError::InvalidElf("kernel entry is Thumb"));
        }

        let mut packed_kernel = Vec::new();
        for segment in kernel_segments.drain(..) {
            let virtual_address = VirtualAddress::new(segment.address);
            let physical_address = if MemRegion::Ram
                .range()
                .contains_address(PhysicalAddress::new(segment.address))
            {
                PhysicalAddress::new(segment.address)
            } else {
                direct_map_physical(virtual_address)?
            };
            let record = KernelSegment {
                data_offset: 0,
                physical_address,
                virtual_address,
                file_size: size32(segment.bytes.len(), "kernel segment size")?,
                memory_size: segment.memory_size,
                flags: segment.flags,
            };
            record.validate()?;
            packed_kernel.push((record, segment.bytes));
        }
        packed_kernel.sort_by_key(|(segment, _)| segment.virtual_address.get());
        validate_kernel_segments(&packed_kernel, kernel_entry)?;

        let mut packed_modules = Vec::new();
        for input in self.modules {
            if input.name.is_empty() || !input.name.is_ascii() {
                return Err(ImageError::InvalidImage("module name"));
            }
            let (entry, segments) = parse_elf(input.elf)?;
            let entry = address32(entry, "module entry")?;
            if entry & 1 != 0 {
                return Err(ImageError::InvalidElf("module entry is Thumb"));
            }
            if direct_map_physical(VirtualAddress::new(entry)).is_ok() {
                return Err(ImageError::InvalidElf(
                    "module entry is in the kernel direct map",
                ));
            }
            let mut records = Vec::new();
            for segment in segments {
                if direct_map_physical(VirtualAddress::new(segment.address)).is_ok() {
                    return Err(ImageError::InvalidElf(
                        "module segment is in the kernel direct map",
                    ));
                }
                let record = ModuleSegment {
                    data_offset: 0,
                    virtual_address: VirtualAddress::new(segment.address),
                    file_size: size32(segment.bytes.len(), "module segment size")?,
                    memory_size: segment.memory_size,
                    flags: segment.flags,
                };
                record.validate()?;
                records.push((record, segment.bytes));
            }
            records.sort_by_key(|(segment, _)| segment.virtual_address.get());
            validate_module_segments(&records, entry)?;
            packed_modules.push(PackedModule {
                name: input.name.as_bytes().to_vec(),
                entry: VirtualAddress::new(entry),
                segments: records,
            });
        }
        packed_modules.sort_by(|left, right| left.name.cmp(&right.name));
        if packed_modules
            .windows(2)
            .any(|modules| modules[0].name == modules[1].name)
        {
            return Err(ImageError::InvalidImage("duplicate module name"));
        }

        build_image(kernel_entry, packed_kernel, packed_modules)
    }
}

/// Packages a kernel ELF and user modules without retaining builder state.
pub fn package(kernel: &[u8], modules: &[ModuleInput<'_>]) -> Result<SystemImage> {
    modules
        .iter()
        .copied()
        .fold(ImageBuilder::new(kernel), |builder, module| {
            builder.add_module(module)
        })
        .build()
}

impl SystemImage {
    /// Parses and fully validates one exact system-ROM image.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let header = ImageHeader::decode(bytes)?;
        if bytes.len() != header.image_size as usize {
            return Err(ImageError::InvalidImage("image size"));
        }
        let mut occupied = vec![Span::new(0, IMAGE_HEADER_SIZE)?];
        let kernel_table = span(
            header.kernel_segment_table_offset,
            header.kernel_segment_count,
            KERNEL_SEGMENT_SIZE,
        )?;
        let module_table = span(
            header.module_table_offset,
            header.module_count,
            MODULE_RECORD_SIZE,
        )?;
        occupy(
            &mut occupied,
            kernel_table,
            bytes.len(),
            "overlapping image tables",
        )?;
        occupy(
            &mut occupied,
            module_table,
            bytes.len(),
            "overlapping image tables",
        )?;

        let mut kernel_segments = Vec::new();
        for index in 0..header.kernel_segment_count as usize {
            let offset = header.kernel_segment_table_offset as usize + index * KERNEL_SEGMENT_SIZE;
            let segment = KernelSegment::decode(&bytes[offset..offset + KERNEL_SEGMENT_SIZE])?;
            occupy(
                &mut occupied,
                Span::from_offset_length(segment.data_offset, segment.file_size)?,
                bytes.len(),
                "overlapping image data",
            )?;
            kernel_segments.push(segment);
        }
        validate_kernel_records(&kernel_segments, header.kernel_entry_vaddr)?;

        let mut modules = Vec::new();
        let mut names = BTreeSet::new();
        for index in 0..header.module_count as usize {
            let offset = header.module_table_offset as usize + index * MODULE_RECORD_SIZE;
            let record = ModuleRecord::decode(&bytes[offset..offset + MODULE_RECORD_SIZE])?;
            let name_span = Span::from_offset_length(record.name_offset, record.name_length)?;
            let segment_table = span(
                record.segment_table_offset,
                record.segment_count,
                MODULE_SEGMENT_SIZE,
            )?;
            occupy(
                &mut occupied,
                name_span,
                bytes.len(),
                "overlapping image data",
            )?;
            occupy(
                &mut occupied,
                segment_table,
                bytes.len(),
                "overlapping image tables",
            )?;
            let name = std::str::from_utf8(&bytes[name_span.start..name_span.end])
                .map_err(|_| ImageError::InvalidImage("module name is not UTF-8"))?;
            if name.is_empty() || !names.insert(name.to_owned()) {
                return Err(ImageError::InvalidImage("module name"));
            }
            let mut segments = Vec::new();
            for segment_index in 0..record.segment_count as usize {
                let segment_offset =
                    record.segment_table_offset as usize + segment_index * MODULE_SEGMENT_SIZE;
                let segment = ModuleSegment::decode(
                    &bytes[segment_offset..segment_offset + MODULE_SEGMENT_SIZE],
                )?;
                occupy(
                    &mut occupied,
                    Span::from_offset_length(segment.data_offset, segment.file_size)?,
                    bytes.len(),
                    "overlapping image data",
                )?;
                segments.push(segment);
            }
            validate_module_records(&segments, record.entry_virtual_address)?;
            modules.push(ImageModule {
                name: name.to_owned(),
                record,
                segments,
            });
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            header,
            kernel_segments,
            modules,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn header(&self) -> ImageHeader {
        self.header
    }

    pub fn kernel_segments(&self) -> &[KernelSegment] {
        &self.kernel_segments
    }

    pub fn modules(&self) -> &[ImageModule] {
        &self.modules
    }

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

#[derive(Clone, Debug)]
struct ElfSegment {
    address: u32,
    bytes: Vec<u8>,
    memory_size: u32,
    flags: u32,
}

#[derive(Clone, Debug)]
struct PackedModule {
    name: Vec<u8>,
    entry: VirtualAddress,
    segments: Vec<(ModuleSegment, Vec<u8>)>,
}

#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    fn new(start: usize, length: usize) -> Result<Self> {
        Ok(Self {
            start,
            end: start
                .checked_add(length)
                .ok_or(ImageError::InvalidImage("offset overflow"))?,
        })
    }

    fn from_offset_length(offset: u32, length: u32) -> Result<Self> {
        Self::new(offset as usize, length as usize)
    }

    fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

fn parse_elf(bytes: &[u8]) -> Result<(u64, Vec<ElfSegment>)> {
    let file = object::File::parse(bytes)?;
    if file.format() != BinaryFormat::Elf
        || file.architecture() != Architecture::Arm
        || file.kind() != ObjectKind::Executable
        || !file.is_little_endian()
    {
        return Err(ImageError::InvalidElf(
            "expected a little-endian ARM executable ELF",
        ));
    }
    if file.entry() & 1 != 0 {
        return Err(ImageError::InvalidElf("entry is Thumb"));
    }
    if file
        .sections()
        .any(|section| section.relocations().next().is_some())
        || file
            .dynamic_relocations()
            .is_some_and(|mut relocations| relocations.next().is_some())
    {
        return Err(ImageError::InvalidElf("relocations are unsupported"));
    }
    let mut segments = Vec::new();
    for segment in file.segments() {
        if segment.size() == 0 {
            continue;
        }
        let address = address32(segment.address(), "segment address")?;
        let memory_size = size64(segment.size(), "segment memory size")?;
        let data = segment.data()?;
        if data.len() > memory_size as usize {
            return Err(ImageError::InvalidElf("segment file size"));
        }
        let range = VirtualRange::new(address, memory_size)?;
        if segments.iter().any(|other: &ElfSegment| {
            VirtualRange::new(other.address, other.memory_size)
                .expect("validated segment")
                .overlaps(range)
        }) {
            return Err(ImageError::InvalidElf("overlapping load segments"));
        }
        segments.push(ElfSegment {
            address,
            bytes: data.to_vec(),
            memory_size,
            flags: flags(segment.permissions()),
        });
    }
    if segments.is_empty() {
        return Err(ImageError::InvalidElf("no loadable segments"));
    }
    Ok((file.entry(), segments))
}

fn build_image(
    kernel_entry: u32,
    mut kernel_segments: Vec<(KernelSegment, Vec<u8>)>,
    mut modules: Vec<PackedModule>,
) -> Result<SystemImage> {
    let kernel_table_offset = IMAGE_HEADER_SIZE;
    let module_table_offset = kernel_table_offset + kernel_segments.len() * KERNEL_SEGMENT_SIZE;
    let mut cursor = module_table_offset + modules.len() * MODULE_RECORD_SIZE;
    let mut module_segment_offsets = Vec::with_capacity(modules.len());
    for module in &modules {
        cursor = align4(cursor)?;
        module_segment_offsets.push(size32(cursor, "module segment table offset")?);
        cursor = checked_add(cursor, module.segments.len() * MODULE_SEGMENT_SIZE)?;
    }
    cursor = align4(cursor)?;
    let mut module_name_offsets = Vec::new();
    for module in &modules {
        module_name_offsets.push(size32(cursor, "module name offset")?);
        cursor = checked_add(cursor, module.name.len())?;
    }
    cursor = align4(cursor)?;
    for (record, data) in &mut kernel_segments {
        if !data.is_empty() {
            record.data_offset = size32(cursor, "kernel data offset")?;
            cursor = checked_add(cursor, data.len())?;
        }
    }
    for module in &mut modules {
        for (record, data) in &mut module.segments {
            if !data.is_empty() {
                record.data_offset = size32(cursor, "module data offset")?;
                cursor = checked_add(cursor, data.len())?;
            }
        }
    }
    if cursor > MemRegion::SystemRom.size() as usize {
        return Err(ImageError::RomOverflow);
    }
    let header = ImageHeader {
        image_size: size32(cursor, "image size")?,
        kernel_segment_table_offset: size32(kernel_table_offset, "kernel table offset")?,
        kernel_segment_count: size32(kernel_segments.len(), "kernel segment count")?,
        module_table_offset: size32(module_table_offset, "module table offset")?,
        module_count: size32(modules.len(), "module count")?,
        bootstrap_entry_paddr: PhysicalAddress::new(MemRegion::Ram.base().get() + 0x8000),
        kernel_entry_vaddr: VirtualAddress::new(kernel_entry),
        boot_info_paddr: PhysicalAddress::new(MemRegion::Ram.base().get() + 0x7000),
    };
    let mut bytes = vec![0; cursor];
    bytes[..IMAGE_HEADER_SIZE].copy_from_slice(&header.encode()?);
    for (index, (segment, data)) in kernel_segments.iter().enumerate() {
        let offset = kernel_table_offset + index * KERNEL_SEGMENT_SIZE;
        bytes[offset..offset + KERNEL_SEGMENT_SIZE].copy_from_slice(&segment.encode()?);
        copy_data(&mut bytes, segment.data_offset, data)?;
    }
    let mut image_modules = Vec::new();
    for (index, module) in modules.iter().enumerate() {
        let record = ModuleRecord {
            name_offset: module_name_offsets[index],
            name_length: size32(module.name.len(), "module name length")?,
            segment_table_offset: module_segment_offsets[index],
            segment_count: size32(module.segments.len(), "module segment count")?,
            entry_virtual_address: module.entry,
        };
        let offset = module_table_offset + index * MODULE_RECORD_SIZE;
        bytes[offset..offset + MODULE_RECORD_SIZE].copy_from_slice(&record.encode());
        copy_data(&mut bytes, record.name_offset, &module.name)?;
        let mut image_segments = Vec::new();
        for (segment_index, (segment, data)) in module.segments.iter().enumerate() {
            let segment_offset =
                record.segment_table_offset as usize + segment_index * MODULE_SEGMENT_SIZE;
            bytes[segment_offset..segment_offset + MODULE_SEGMENT_SIZE]
                .copy_from_slice(&segment.encode()?);
            copy_data(&mut bytes, segment.data_offset, data)?;
            image_segments.push(*segment);
        }
        image_modules.push(ImageModule {
            name: String::from_utf8(module.name.clone()).expect("validated ASCII module name"),
            record,
            segments: image_segments,
        });
    }
    let image = SystemImage::parse(&bytes)?;
    debug_assert_eq!(image.header, header);
    Ok(image)
}

fn validate_kernel_segments(segments: &[(KernelSegment, Vec<u8>)], entry: u32) -> Result<()> {
    validate_kernel_records(
        &segments
            .iter()
            .map(|(segment, _)| *segment)
            .collect::<Vec<_>>(),
        VirtualAddress::new(entry),
    )
}

fn validate_kernel_records(segments: &[KernelSegment], entry: VirtualAddress) -> Result<()> {
    if !segments.iter().any(|segment| {
        segment.virtual_address == entry && segment.flags & 4 != 0
            || segment.virtual_address.get() <= entry.get()
                && entry.get() < segment.virtual_address.get() + segment.memory_size
                && segment.flags & 4 != 0
    }) {
        return Err(ImageError::InvalidImage("kernel entry is not executable"));
    }
    if !segments.iter().any(|segment| {
        segment.physical_address.get() == MemRegion::Ram.base().get() + 0x8000
            && segment.flags & 4 != 0
    }) {
        return Err(ImageError::InvalidImage(
            "missing executable bootstrap segment",
        ));
    }
    for (index, segment) in segments.iter().enumerate() {
        for other in &segments[index + 1..] {
            if physical_ranges_overlap(
                physical_segment_range(*segment)?,
                physical_segment_range(*other)?,
            ) || virtual_segment_range(segment.virtual_address, segment.memory_size)?.overlaps(
                virtual_segment_range(other.virtual_address, other.memory_size)?,
            ) {
                return Err(ImageError::InvalidImage("overlapping kernel segments"));
            }
        }
    }
    Ok(())
}

fn validate_module_segments(segments: &[(ModuleSegment, Vec<u8>)], entry: u32) -> Result<()> {
    validate_module_records(
        &segments
            .iter()
            .map(|(segment, _)| *segment)
            .collect::<Vec<_>>(),
        VirtualAddress::new(entry),
    )
}

fn validate_module_records(segments: &[ModuleSegment], entry: VirtualAddress) -> Result<()> {
    if !segments.iter().any(|segment| {
        segment.virtual_address.get() <= entry.get()
            && entry.get() < segment.virtual_address.get() + segment.memory_size
            && segment.flags & 4 != 0
    }) {
        return Err(ImageError::InvalidImage("module entry is not executable"));
    }
    for (index, segment) in segments.iter().enumerate() {
        for other in &segments[index + 1..] {
            if virtual_segment_range(segment.virtual_address, segment.memory_size)?.overlaps(
                virtual_segment_range(other.virtual_address, other.memory_size)?,
            ) {
                return Err(ImageError::InvalidImage("overlapping module segments"));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct VirtualRange {
    start: u32,
    end: u64,
}

impl VirtualRange {
    fn new(start: u32, length: u32) -> Result<Self> {
        let end = u64::from(start) + u64::from(length);
        if end > u64::from(u32::MAX) + 1 {
            return Err(ImageError::InvalidImage("segment range"));
        }
        Ok(Self { start, end })
    }

    fn overlaps(self, other: Self) -> bool {
        u64::from(self.start) < other.end && u64::from(other.start) < self.end
    }
}

fn physical_segment_range(segment: KernelSegment) -> Result<PhysicalRange> {
    PhysicalRange::new(segment.physical_address, segment.memory_size).map_err(ImageError::from)
}

fn physical_ranges_overlap(left: PhysicalRange, right: PhysicalRange) -> bool {
    u64::from(left.start().get()) < right.end_exclusive()
        && u64::from(right.start().get()) < left.end_exclusive()
}

fn virtual_segment_range(address: VirtualAddress, length: u32) -> Result<VirtualRange> {
    VirtualRange::new(address.get(), length)
}

fn span(offset: u32, count: u32, record_size: usize) -> Result<Span> {
    let length = (count as usize)
        .checked_mul(record_size)
        .ok_or(ImageError::InvalidImage("table size overflow"))?;
    Span::new(offset as usize, length)
}

fn occupy(
    occupied: &mut Vec<Span>,
    candidate: Span,
    image_size: usize,
    detail: &'static str,
) -> Result<()> {
    if candidate.end > image_size {
        return Err(ImageError::InvalidImage("record outside image"));
    }
    if occupied.iter().any(|span| span.overlaps(candidate)) {
        return Err(ImageError::InvalidImage(detail));
    }
    occupied.push(candidate);
    Ok(())
}

fn flags(permissions: Permissions) -> u32 {
    u32::from(permissions.readable())
        | (u32::from(permissions.writable()) << 1)
        | (u32::from(permissions.executable()) << 2)
}

fn address32(value: u64, detail: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| ImageError::InvalidElf(detail))
}

fn size64(value: u64, detail: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| ImageError::InvalidElf(detail))
}

fn size32(value: usize, detail: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| ImageError::InvalidImage(detail))
}

fn checked_add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right)
        .ok_or(ImageError::InvalidImage("image size overflow"))
}

fn align4(value: usize) -> Result<usize> {
    checked_add(value, 3).map(|value| value & !3)
}

fn copy_data(destination: &mut [u8], offset: u32, data: &[u8]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let start = offset as usize;
    let end = checked_add(start, data.len())?;
    destination
        .get_mut(start..end)
        .ok_or(ImageError::InvalidImage("data outside image"))?
        .copy_from_slice(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ImageBuilder, ImageError, ModuleInput, SystemImage};

    #[test]
    fn packer_is_deterministic_and_boot_plan_covers_bss() {
        let kernel = elf(
            0xc000_9000,
            &[
                (0x4000_8000, &[1, 2, 3, 4], 8),
                (0xc000_9000, &[5, 6, 7, 8], 4),
            ],
        );
        let module = elf(0x0040_0000, &[(0x0040_0000, &[9, 10, 11, 12], 4)]);
        let image = ImageBuilder::new(&kernel)
            .add_module(ModuleInput {
                name: "shell",
                elf: &module,
            })
            .build()
            .unwrap();
        let repeated = ImageBuilder::new(&kernel)
            .add_module(ModuleInput {
                name: "shell",
                elf: &module,
            })
            .build()
            .unwrap();
        assert_eq!(image.bytes(), repeated.bytes());
        assert_eq!(SystemImage::parse(image.bytes()).unwrap(), image);
        let plan = image.boot_plan().unwrap();
        assert_eq!(plan.copies.len(), 2);
        assert_eq!(plan.copies[0].zeroed_length, 4);
        assert_eq!(plan.boot_info_vaddr.get(), 0xc000_7000);
        assert_eq!(plan.kernel_entry_vaddr.get(), 0xc000_9000);
        let mut writes = Vec::new();
        plan.apply(|address, bytes| {
            writes.push((address.get(), bytes.to_vec()));
            Ok::<(), ()>(())
        })
        .unwrap();
        assert_eq!(writes[0], (0x4000_8000, vec![1, 2, 3, 4]));
        assert_eq!(writes[1], (0x4000_8004, vec![0; 4]));
        assert_eq!(writes.last().unwrap().0, 0x4000_7000);
    }

    #[test]
    fn parser_rejects_truncated_and_version_mismatched_images() {
        let kernel = elf(
            0xc000_9000,
            &[
                (0x4000_8000, &[1, 2, 3, 4], 4),
                (0xc000_9000, &[5, 6, 7, 8], 4),
            ],
        );
        let image = ImageBuilder::new(&kernel).build().unwrap();
        assert!(SystemImage::parse(&image.bytes()[..63]).is_err());
        let mut version = image.bytes().to_vec();
        version[4] = 2;
        assert!(matches!(
            SystemImage::parse(&version),
            Err(ImageError::Platform(_))
        ));

        let module = elf(0x0040_0000, &[(0x0040_0000, &[9, 10, 11, 12], 4)]);
        let image = ImageBuilder::new(&kernel)
            .add_module(ModuleInput {
                name: "shell",
                elf: &module,
            })
            .build()
            .unwrap();
        let mut corrupt_table = image.bytes().to_vec();
        let module_offset = image.header().module_table_offset as usize;
        corrupt_table[module_offset + 8..module_offset + 12]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(SystemImage::parse(&corrupt_table).is_err());
    }

    #[test]
    fn packer_rejects_thumb_entries_and_duplicate_names() {
        let kernel = elf(
            0xc000_9000,
            &[
                (0x4000_8000, &[1, 2, 3, 4], 4),
                (0xc000_9000, &[5, 6, 7, 8], 4),
            ],
        );
        let thumb = elf(0x0040_0001, &[(0x0040_0000, &[1, 2, 3, 4], 4)]);
        assert!(
            ImageBuilder::new(&kernel)
                .add_module(ModuleInput {
                    name: "a",
                    elf: &thumb
                })
                .build()
                .is_err()
        );
        let module = elf(0x0040_0000, &[(0x0040_0000, &[1, 2, 3, 4], 4)]);
        assert!(
            ImageBuilder::new(&kernel)
                .add_module(ModuleInput {
                    name: "a",
                    elf: &module
                })
                .add_module(ModuleInput {
                    name: "a",
                    elf: &module
                })
                .build()
                .is_err()
        );
    }

    fn elf(entry: u32, segments: &[(u32, &[u8], u32)]) -> Vec<u8> {
        const ELF_HEADER_SIZE: usize = 52;
        const PROGRAM_HEADER_SIZE: usize = 32;
        let program_headers = segments.len() * PROGRAM_HEADER_SIZE;
        let mut bytes = vec![
            0;
            0x100
                + segments
                    .iter()
                    .map(|(_, data, _)| data.len())
                    .sum::<usize>()
        ];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 1;
        bytes[5] = 1;
        bytes[6] = 1;
        write_u16(&mut bytes, 16, 2);
        write_u16(&mut bytes, 18, 40);
        write_u32(&mut bytes, 20, 1);
        write_u32(&mut bytes, 24, entry);
        write_u32(&mut bytes, 28, ELF_HEADER_SIZE as u32);
        write_u16(&mut bytes, 40, ELF_HEADER_SIZE as u16);
        write_u16(&mut bytes, 42, PROGRAM_HEADER_SIZE as u16);
        write_u16(&mut bytes, 44, segments.len() as u16);
        let mut data_offset = 0x100;
        for (index, (address, data, memory_size)) in segments.iter().enumerate() {
            let offset = ELF_HEADER_SIZE + index * PROGRAM_HEADER_SIZE;
            write_u32(&mut bytes, offset, 1);
            write_u32(&mut bytes, offset + 4, data_offset as u32);
            write_u32(&mut bytes, offset + 8, *address);
            write_u32(&mut bytes, offset + 12, *address);
            write_u32(&mut bytes, offset + 16, data.len() as u32);
            write_u32(&mut bytes, offset + 20, *memory_size);
            write_u32(&mut bytes, offset + 24, 5);
            write_u32(&mut bytes, offset + 28, 4);
            bytes[data_offset..data_offset + data.len()].copy_from_slice(data);
            data_offset += data.len();
        }
        bytes.truncate(data_offset.max(ELF_HEADER_SIZE + program_headers));
        bytes
    }

    fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
