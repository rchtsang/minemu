//! Thread-owned runtime service for the concrete Unicorn machine.

mod service;
mod types;

pub use service::RuntimeHandle;
pub use types::{
    LifecycleState, RuntimeConfig, RuntimeError, RuntimeInspection, RuntimeStatus, UartPort,
};

#[cfg(test)]
mod tests;
