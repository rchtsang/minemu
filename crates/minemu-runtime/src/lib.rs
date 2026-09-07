//! Thread-owned runtime service for the concrete Unicorn machine.

mod service;
mod types;

pub use minemu_unicorn::ExecutionInspection;
pub use service::RuntimeHandle;
pub use types::{
    LifecycleState, RuntimeConfig, RuntimeError, RuntimeInspection, RuntimeInspectionRequest,
    RuntimeStatus, ScheduledUartInput, UartPort,
};

#[cfg(test)]
mod tests;
