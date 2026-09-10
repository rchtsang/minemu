use minemu_platform::PlatformError;
use thiserror::Error;

/// Errors produced by the backend-independent machine state.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error("physical address {0:#010x} is unmapped")]
    UnmappedPhysicalAddress(u32),
    #[error("physical address {0:#010x} is immutable")]
    ImmutablePhysicalAddress(u32),
    #[error("physical access at {address:#010x} needs {needed} bytes")]
    PhysicalAccessOutOfBounds { address: u32, needed: usize },
    #[error("interrupt controller EOI {actual} does not match claimed source {expected}")]
    InvalidEoi { expected: u32, actual: u32 },
    #[error("block unit {0} is not supported")]
    InvalidBlockUnit(u32),
    #[error("block unit {unit} media must contain a nonzero whole number of sectors")]
    InvalidBlockMedia { unit: u32 },
    #[error("block command is already active")]
    BlockBusy,
    #[error("block unit {unit} media flush failed")]
    BlockFlush {
        unit: u32,
        #[source]
        source: std::io::Error,
    },
}

/// Result type used by `minemu-core`.
pub type Result<T> = std::result::Result<T, CoreError>;
