//! Backend-independent state and policy for the `minemu` teaching machine.
//!
//! This crate owns deterministic memory, device, MMU, virtual-time, and
//! observability behavior. CPU/backend adaptation belongs in `minemu-unicorn`.

mod block;
mod bus;
mod error;
mod exception;
mod interrupt;
mod machine;
mod memory;
mod mmu;
mod rng;
mod systick;
mod trace;
mod uart;

pub use block::{BlockDevice, BlockUpdate};
pub use bus::MmioBus;
pub use error::{CoreError, Result};
pub use exception::ExceptionPlan;
pub use interrupt::{InterruptController, InterruptSignal, InterruptUpdate};
pub use machine::{InstructionOutcome, Machine, MachineStatus};
pub use memory::{PhysicalMemory, PhysicalMemoryAccess};
pub use mmu::{Mmu, MmuFault};
pub use rng::{Rng, RngUpdate};
pub use systick::{SysTick, SysTickUpdate};
pub use trace::{TraceDevice, TraceEvent, TraceUpdate};
pub use uart::{Uart, UartUpdate};
