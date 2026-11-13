//! Host-side packing and validation for versioned system-ROM images.

mod boot;
mod elf;
mod format;
mod validation;

use minemu_platform::{
    KernelSegment, MemRegion, ModuleSegment, PhysicalAddress, PlatformError, VirtualAddress,
    direct_map_physical,
};
use thiserror::Error;

use crate::{
    elf::ElfSegment,
    elf::parse_elf,
    format::{PackedModule, build_image},
    validation::{validate_kernel_records, validate_module_records},
};

pub use boot::{BootCopy, BootPlan};
pub use format::{ImageModule, SystemImage};

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
        let (kernel_entry, kernel_segments) = parse_elf(self.kernel)?;
        let kernel_entry = VirtualAddress::new(kernel_entry);
        let kernel_segments = pack_kernel_segments(kernel_segments)?;
        validate_kernel_records(&records(&kernel_segments), kernel_entry)?;

        let mut modules = self
            .modules
            .into_iter()
            .map(pack_module)
            .collect::<Result<Vec<_>>>()?;
        modules.sort_by(|left, right| left.name.cmp(&right.name));
        if modules
            .windows(2)
            .any(|modules| modules[0].name == modules[1].name)
        {
            return Err(ImageError::InvalidImage("duplicate module name"));
        }

        build_image(kernel_entry, kernel_segments, modules)
    }
}

/// Converts kernel ELF segments into address-sorted, individually valid records.
fn pack_kernel_segments(segments: Vec<ElfSegment>) -> Result<Vec<(KernelSegment, Vec<u8>)>> {
    let mut packed = segments
        .into_iter()
        .map(|segment| {
            let virtual_address = VirtualAddress::new(segment.address);
            // Kernel segments are linked either identity-mapped in RAM or in the direct map.
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
                file_size: u32::try_from(segment.bytes.len())
                    .map_err(|_| ImageError::InvalidElf("kernel segment size"))?,
                memory_size: segment.memory_size,
                flags: segment.flags,
            };
            record.validate()?;
            Ok((record, segment.bytes))
        })
        .collect::<Result<Vec<_>>>()?;
    packed.sort_by_key(|(segment, _)| segment.virtual_address.get());
    Ok(packed)
}

/// Parses and validates one user module.
fn pack_module(input: ModuleInput<'_>) -> Result<PackedModule> {
    if input.name.is_empty() {
        return Err(ImageError::InvalidImage("module name"));
    }
    let (entry, segments) = parse_elf(input.elf)?;
    let entry = VirtualAddress::new(entry);
    let mut segments = segments
        .into_iter()
        .map(|segment| {
            let record = ModuleSegment {
                data_offset: 0,
                virtual_address: VirtualAddress::new(segment.address),
                file_size: u32::try_from(segment.bytes.len())
                    .map_err(|_| ImageError::InvalidElf("module segment size"))?,
                memory_size: segment.memory_size,
                flags: segment.flags,
            };
            record.validate()?;
            Ok((record, segment.bytes))
        })
        .collect::<Result<Vec<_>>>()?;
    segments.sort_by_key(|(segment, _)| segment.virtual_address.get());
    validate_module_records(&records(&segments), entry)?;
    Ok(PackedModule {
        name: input.name.as_bytes().to_vec(),
        entry,
        segments,
    })
}

fn records<T: Copy>(packed: &[(T, Vec<u8>)]) -> Vec<T> {
    packed.iter().map(|(record, _)| *record).collect()
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

#[cfg(test)]
mod tests;
