use object::{
    Architecture, BinaryFormat, Object, ObjectKind, ObjectSection, ObjectSegment, Permissions,
};

use crate::{ImageError, Result};

/// A loadable ELF segment normalized for image construction.
#[derive(Clone, Debug)]
pub(crate) struct ElfSegment {
    pub(crate) address: u32,
    pub(crate) bytes: Vec<u8>,
    pub(crate) memory_size: u32,
    pub(crate) flags: u32,
}

pub(crate) fn parse_elf(bytes: &[u8]) -> Result<(u32, Vec<ElfSegment>)> {
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
        let address = u32::try_from(segment.address())
            .map_err(|_| ImageError::InvalidElf("segment address"))?;
        let memory_size = u32::try_from(segment.size())
            .map_err(|_| ImageError::InvalidElf("segment memory size"))?;
        if u64::from(address) + u64::from(memory_size) > u64::from(u32::MAX) + 1 {
            return Err(ImageError::InvalidElf("segment range"));
        }
        let data = segment.data()?;
        if data.len() > memory_size as usize {
            return Err(ImageError::InvalidElf("segment file size"));
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
    let entry = u32::try_from(file.entry()).map_err(|_| ImageError::InvalidElf("entry"))?;
    Ok((entry, segments))
}

fn flags(permissions: Permissions) -> u32 {
    u32::from(permissions.readable())
        | (u32::from(permissions.writable()) << 1)
        | (u32::from(permissions.executable()) << 2)
}
