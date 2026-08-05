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
    elf::parse_elf,
    format::{PackedModule, build_image},
    validation::{validate_kernel_segments, validate_module_segments},
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
        let mut packed_kernel = Vec::with_capacity(kernel_segments.len());
        for segment in kernel_segments {
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
                file_size: u32::try_from(segment.bytes.len())
                    .map_err(|_| ImageError::InvalidElf("kernel segment size"))?,
                memory_size: segment.memory_size,
                flags: segment.flags,
            };
            record.validate()?;
            packed_kernel.push((record, segment.bytes));
        }
        packed_kernel.sort_by_key(|(segment, _)| segment.virtual_address.get());
        validate_kernel_segments(&packed_kernel, VirtualAddress::new(kernel_entry))?;

        let mut packed_modules = Vec::with_capacity(self.modules.len());
        for input in self.modules {
            if input.name.is_empty() {
                return Err(ImageError::InvalidImage("module name"));
            }
            let (entry, segments) = parse_elf(input.elf)?;
            let mut records = Vec::with_capacity(segments.len());
            for segment in segments {
                let record = ModuleSegment {
                    data_offset: 0,
                    virtual_address: VirtualAddress::new(segment.address),
                    file_size: u32::try_from(segment.bytes.len())
                        .map_err(|_| ImageError::InvalidElf("module segment size"))?,
                    memory_size: segment.memory_size,
                    flags: segment.flags,
                };
                record.validate()?;
                records.push((record, segment.bytes));
            }
            records.sort_by_key(|(segment, _)| segment.virtual_address.get());
            validate_module_segments(&records, VirtualAddress::new(entry))?;
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

        build_image(
            VirtualAddress::new(kernel_entry),
            packed_kernel,
            packed_modules,
        )
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

#[cfg(test)]
mod tests;
