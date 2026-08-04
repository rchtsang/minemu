//! Backend-independent definitions for the `minemu` platform ABI.
//!
//! The normative ABI is documented in `docs/dev/emulator.md`. This crate owns
//! the Rust representation and validation of that contract; it intentionally
//! has no Unicorn, filesystem, or host-runtime dependency.

mod access;
mod address;
mod error;
mod exception;
mod image;
mod mmap;
mod mmio;
mod mmu;
mod observability;
mod peripheral;
pub mod peripherals;

pub use access::{Access, Permissions};
pub use address::{PhysicalAddress, PhysicalRange, VirtualAddress};
pub use error::{PlatformError, Result};
pub use exception::{CpuMode, ExceptionKind, ExceptionRequest};
pub use image::{
    ABI_VERSION, BOOT_INFO_MAGIC, BOOT_INFO_SIZE, BootInfo, IMAGE_HEADER_SIZE, IMAGE_MAGIC,
    ImageHeader, KERNEL_SEGMENT_SIZE, KernelSegment, MODULE_RECORD_SIZE, MODULE_SEGMENT_SIZE,
    ModuleRecord, ModuleSegment,
};
pub use mmap::{
    BOOT_ROM_BASE, BOOT_ROM_SIZE, MemRegion, PAGE_SIZE, RAM_BASE, RAM_SIZE, SYSTEM_ROM_BASE,
    SYSTEM_ROM_SIZE, direct_map_physical, direct_map_virtual,
};
pub use mmio::{MmioRegister, MmioTransaction, MmioWidth, decode_mmio};
pub use mmu::{
    Cp15Operation, FaultCause, FaultStatus, PTE_ACCESSED, PTE_DIRTY, PTE_EXECUTABLE, PTE_READABLE,
    PTE_SOFTWARE_MASK, PTE_USER, PTE_VALID, PTE_WRITABLE, PageDirectoryEntry, PageTableEntry,
    is_mmu_target_page, validate_page_table_page,
};
pub use observability::{
    BlockInspection, InspectionRequest, InspectionResponse, InterruptInspection, MmuInspection,
    ObservableEvent, PeripheralsInspection, RngInspection, SysTickInspection, TraceInspectionEvent,
    UartInspection,
};
pub use peripheral::Peripheral;
