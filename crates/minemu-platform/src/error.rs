use thiserror::Error;

/// Errors returned while validating a platform ABI value.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PlatformError {
    #[error("range must not be empty")]
    EmptyRange,
    #[error("range {start:#010x} with length {length:#010x} exceeds the 32-bit address space")]
    AddressRangeOverflow { start: u32, length: u32 },
    #[error("address {0:#010x} is outside physical RAM")]
    AddressOutsideRam(u32),
    #[error("address {0:#010x} is not a RAM, ROM, or implemented device page")]
    AddressOutsideMmuTarget(u32),
    #[error("address {0:#010x} is not 4 KiB aligned")]
    UnalignedPage(u32),
    #[error("vector base {0:#010x} is not 32-byte aligned")]
    UnalignedVectorBase(u32),
    #[error("address {0:#010x} is not an implemented MMIO register")]
    InvalidMmioAddress(u32),
    #[error("MMIO width {0} is not the required 32-bit width")]
    InvalidMmioWidth(u8),
    #[error("MMIO address {0:#010x} is not four-byte aligned")]
    UnalignedMmioAddress(u32),
    #[error("MMIO register access direction is invalid")]
    InvalidMmioDirection,
    #[error("value {value:#010x} is invalid for {register}")]
    InvalidMmioValue { register: &'static str, value: u32 },
    #[error("page-directory entry {0:#010x} sets reserved bits")]
    InvalidPageDirectoryBits(u32),
    #[error("page-table target {0:#010x} is invalid")]
    InvalidPageTableTarget(u32),
    #[error("user mapping may not target device page {0:#010x}")]
    UserDeviceMapping(u32),
    #[error("fault status {0:#010x} is not ABI-valid")]
    InvalidFaultStatus(u32),
    #[error("image record needs {expected} bytes, received {actual}")]
    InvalidImageLength { expected: usize, actual: usize },
    #[error("image field {0} is invalid")]
    InvalidImageField(&'static str),
    #[error("image table lies outside image bounds")]
    ImageTableOutsideImage,
}

/// Result type used by platform ABI validation.
pub type Result<T> = std::result::Result<T, PlatformError>;
