use minemu_platform::{
    BOOT_INFO_SIZE, BOOT_STACK_BASE, BOOT_STACK_SIZE, BOOTSTRAP_ENTRY_PADDR, DIRECT_MAP_BASE,
    KernelSegment, ModuleSegment, PhysicalAddress, PhysicalRange, RAM_SIZE, SEGMENT_EXECUTABLE,
    VirtualAddress,
};

use crate::{ImageError, Result};

const A32_INSTRUCTION_SIZE: u32 = 4;
const KERNEL_DIRECT_MAP_START: u32 = DIRECT_MAP_BASE;
const KERNEL_DIRECT_MAP_END: u64 = DIRECT_MAP_BASE as u64 + RAM_SIZE as u64;

pub(crate) fn validate_kernel_records(
    segments: &[KernelSegment],
    entry: VirtualAddress,
) -> Result<()> {
    validate_a32_entry(entry, "kernel entry")?;
    if !segments.iter().any(|segment| {
        initialized_executable_virtual_range(*segment).is_some_and(|range| range.contains(entry))
    }) {
        return Err(ImageError::InvalidImage(
            "kernel entry is not initialized executable code",
        ));
    }
    let bootstrap = PhysicalAddress::new(BOOTSTRAP_ENTRY_PADDR);
    let bootstrap_instruction = PhysicalRange::new(bootstrap, A32_INSTRUCTION_SIZE)
        .expect("fixed bootstrap instruction range is valid");
    if !segments.iter().any(|segment| {
        initialized_executable_physical_range(*segment)
            .is_some_and(|range| range.contains_range(bootstrap_instruction))
    }) {
        return Err(ImageError::InvalidImage(
            "missing initialized executable bootstrap segment",
        ));
    }
    // The boot ROM stack is immediately followed by the boot-info record.
    let boot_workspace = PhysicalRange::new(
        PhysicalAddress::new(BOOT_STACK_BASE),
        BOOT_STACK_SIZE + BOOT_INFO_SIZE as u32,
    )
    .expect("fixed boot workspace range is valid");
    for (index, segment) in segments.iter().enumerate() {
        let physical = physical_range(*segment)?;
        if physical.overlaps(boot_workspace) {
            return Err(ImageError::InvalidImage(
                "kernel segment overlaps boot ROM workspace",
            ));
        }
        let virtual_range = AddressRange::new(segment.virtual_address, segment.memory_size)?;
        for other in &segments[index + 1..] {
            if physical.overlaps(physical_range(*other)?)
                || virtual_range
                    .overlaps(AddressRange::new(other.virtual_address, other.memory_size)?)
            {
                return Err(ImageError::InvalidImage("overlapping kernel segments"));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_module_records(
    segments: &[ModuleSegment],
    entry: VirtualAddress,
) -> Result<()> {
    validate_a32_entry(entry, "module entry")?;
    if !segments.iter().any(|segment| {
        initialized_executable_module_range(*segment).is_some_and(|range| range.contains(entry))
    }) {
        return Err(ImageError::InvalidImage(
            "module entry is not initialized executable code",
        ));
    }
    for (index, segment) in segments.iter().enumerate() {
        let range = AddressRange::new(segment.virtual_address, segment.memory_size)?;
        if range.overlaps_kernel_direct_map() {
            return Err(ImageError::InvalidImage(
                "module segment is in the kernel direct map",
            ));
        }
        for other in &segments[index + 1..] {
            if range.overlaps(AddressRange::new(other.virtual_address, other.memory_size)?) {
                return Err(ImageError::InvalidImage("overlapping module segments"));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct AddressRange {
    start: u32,
    end: u64,
}

impl AddressRange {
    fn new(start: VirtualAddress, length: u32) -> Result<Self> {
        let end = u64::from(start.get()) + u64::from(length);
        if end > u64::from(u32::MAX) + 1 {
            return Err(ImageError::InvalidImage("segment range"));
        }
        Ok(Self {
            start: start.get(),
            end,
        })
    }

    fn contains(self, address: VirtualAddress) -> bool {
        u64::from(self.start) <= u64::from(address.get())
            && u64::from(address.get()) + u64::from(A32_INSTRUCTION_SIZE) <= self.end
    }

    fn overlaps(self, other: Self) -> bool {
        u64::from(self.start) < other.end && u64::from(other.start) < self.end
    }

    fn overlaps_kernel_direct_map(self) -> bool {
        u64::from(self.start) < KERNEL_DIRECT_MAP_END
            && u64::from(KERNEL_DIRECT_MAP_START) < self.end
    }
}

fn validate_a32_entry(entry: VirtualAddress, detail: &'static str) -> Result<()> {
    if entry.get() & 3 != 0 {
        return Err(ImageError::InvalidImage(detail));
    }
    Ok(())
}

fn initialized_executable_virtual_range(segment: KernelSegment) -> Option<AddressRange> {
    (segment.flags & SEGMENT_EXECUTABLE != 0)
        .then(|| AddressRange::new(segment.virtual_address, segment.file_size).ok())
        .flatten()
}

fn initialized_executable_physical_range(segment: KernelSegment) -> Option<PhysicalRange> {
    (segment.flags & SEGMENT_EXECUTABLE != 0)
        .then(|| PhysicalRange::new(segment.physical_address, segment.file_size).ok())
        .flatten()
}

fn initialized_executable_module_range(segment: ModuleSegment) -> Option<AddressRange> {
    (segment.flags & SEGMENT_EXECUTABLE != 0)
        .then(|| AddressRange::new(segment.virtual_address, segment.file_size).ok())
        .flatten()
}

fn physical_range(segment: KernelSegment) -> Result<PhysicalRange> {
    PhysicalRange::new(segment.physical_address, segment.memory_size).map_err(ImageError::from)
}
