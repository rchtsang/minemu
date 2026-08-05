use std::collections::BTreeSet;
use std::ops::Deref;

use minemu_platform::{
    IMAGE_HEADER_SIZE, ImageHeader, KERNEL_SEGMENT_SIZE, KernelSegment, MODULE_RECORD_SIZE,
    MODULE_SEGMENT_SIZE, MemRegion, ModuleRecord, ModuleSegment, PhysicalAddress, VirtualAddress,
};

use crate::{
    ImageError, Result,
    validation::{validate_kernel_records, validate_module_records},
};

/// A canonical system-ROM image produced by [`crate::ImageBuilder`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemImage {
    pub(crate) bytes: Vec<u8>,
    pub(crate) header: ImageHeader,
    pub(crate) kernel_segments: Vec<KernelSegment>,
    pub(crate) modules: Vec<ImageModule>,
}

/// A parsed user module and its table record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageModule {
    pub name: String,
    pub record: ModuleRecord,
    pub segments: Vec<ModuleSegment>,
}

#[derive(Clone, Debug)]
pub(crate) struct PackedModule {
    pub(crate) name: Vec<u8>,
    pub(crate) entry: VirtualAddress,
    pub(crate) segments: Vec<(ModuleSegment, Vec<u8>)>,
}

impl SystemImage {
    /// Parses and fully validates one exact system-ROM image.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let header = ImageHeader::decode(bytes)?;
        if bytes.len() != header.image_size as usize {
            return Err(ImageError::InvalidImage("image size"));
        }
        let mut image_map = ImageMap::new()?;
        let kernel_table = Span::new_checked(
            header.kernel_segment_table_offset,
            header.kernel_segment_count,
            KERNEL_SEGMENT_SIZE,
        )?;
        let module_table = Span::new_checked(
            header.module_table_offset,
            header.module_count,
            MODULE_RECORD_SIZE,
        )?;
        image_map.occupy(kernel_table, bytes.len(), "overlapping image tables")?;
        image_map.occupy(module_table, bytes.len(), "overlapping image tables")?;

        let mut kernel_segments = Vec::new();
        for index in 0..header.kernel_segment_count as usize {
            let offset = header.kernel_segment_table_offset as usize + index * KERNEL_SEGMENT_SIZE;
            let segment = KernelSegment::decode(&bytes[offset..offset + KERNEL_SEGMENT_SIZE])?;
            image_map.occupy(
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
            let segment_table = Span::new_checked(
                record.segment_table_offset,
                record.segment_count,
                MODULE_SEGMENT_SIZE,
            )?;
            image_map.occupy(name_span, bytes.len(), "overlapping image data")?;
            image_map.occupy(segment_table, bytes.len(), "overlapping image tables")?;
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
                image_map.occupy(
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
}

pub(crate) fn build_image(
    kernel_entry: VirtualAddress,
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
    let mut module_name_offsets = Vec::with_capacity(modules.len());
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
        kernel_entry_vaddr: kernel_entry,
        boot_info_paddr: PhysicalAddress::new(MemRegion::Ram.base().get() + 0x7000),
    };
    let mut bytes = vec![0; cursor];
    bytes[..IMAGE_HEADER_SIZE].copy_from_slice(&header.encode()?);
    for (index, (segment, data)) in kernel_segments.iter().enumerate() {
        let offset = kernel_table_offset + index * KERNEL_SEGMENT_SIZE;
        bytes[offset..offset + KERNEL_SEGMENT_SIZE].copy_from_slice(&segment.encode()?);
        copy_data(&mut bytes, segment.data_offset, data)?;
    }
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
        for (segment_index, (segment, data)) in module.segments.iter().enumerate() {
            let segment_offset =
                record.segment_table_offset as usize + segment_index * MODULE_SEGMENT_SIZE;
            bytes[segment_offset..segment_offset + MODULE_SEGMENT_SIZE]
                .copy_from_slice(&segment.encode()?);
            copy_data(&mut bytes, segment.data_offset, data)?;
        }
    }
    SystemImage::parse(&bytes)
}

#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
}

struct ImageMap(Vec<Span>);

impl ImageMap {
    fn new() -> Result<Self> {
        Ok(Self(vec![Span::new(0, IMAGE_HEADER_SIZE)?]))
    }

    fn occupy(&mut self, candidate: Span, image_size: usize, detail: &'static str) -> Result<()> {
        if candidate.end > image_size {
            return Err(ImageError::InvalidImage("record outside image"));
        }
        if self.iter().any(|span| span.overlaps(candidate)) {
            return Err(ImageError::InvalidImage(detail));
        }
        self.0.push(candidate);
        Ok(())
    }
}

impl Deref for ImageMap {
    type Target = Vec<Span>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Span {
    fn new_checked(offset: u32, count: u32, record_size: usize) -> Result<Self> {
        let length = (count as usize)
            .checked_mul(record_size)
            .ok_or(ImageError::InvalidImage("table size overflow"))?;
        Self::new(offset as usize, length)
    }

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
