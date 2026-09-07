use std::sync::mpsc::Receiver;

use minemu_platform::peripherals::interrupt::{
    DEFAULT_PRIORITY_BLOCK, DEFAULT_PRIORITY_SYSTICK, DEFAULT_PRIORITY_UART0,
    DEFAULT_PRIORITY_UART1, Source,
};
use minemu_platform::{InterruptInspection, Peripheral, peripherals::interrupt::Register};

use crate::{CoreError, Result};

/// One state-changing interrupt-controller input.
pub enum InterruptUpdate {
    Write { register: Register, value: u32 },
}

/// A level transition sent from a peripheral to the interrupt controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptSignal {
    pub source: Source,
    pub pending: bool,
}

/// Deterministic interrupt-controller state independent of CPU delivery.
pub struct InterruptController {
    receiver: Receiver<InterruptSignal>,
    pending: u32,
    enabled: u32,
    priorities: [u8; 4],
    claim: Option<Source>,
}

impl InterruptController {
    pub fn new(receiver: Receiver<InterruptSignal>) -> Self {
        Self {
            receiver,
            pending: 0,
            enabled: 0,
            priorities: [
                DEFAULT_PRIORITY_SYSTICK,
                DEFAULT_PRIORITY_UART0,
                DEFAULT_PRIORITY_UART1,
                DEFAULT_PRIORITY_BLOCK,
            ],
            claim: None,
        }
    }

    pub const fn enabled(&self) -> u32 {
        self.enabled
    }

    /// Drains queued peripheral level transitions into the controller-owned bitmap.
    pub fn process_signals(&mut self) {
        while let Ok(signal) = self.receiver.try_recv() {
            if signal.pending {
                self.pending |= signal.source.bit();
            } else {
                self.pending &= !signal.source.bit();
            }
        }
    }

    pub fn pending(&mut self) -> u32 {
        self.process_signals();
        self.pending
    }

    fn set_enabled(&mut self, enabled: u32) {
        self.enabled = enabled & 0x0f;
    }

    pub const fn priority(&self, source: Source) -> u8 {
        self.priorities[source as usize]
    }

    fn set_priority(&mut self, source: Source, priority: u8) {
        self.priorities[source as usize] = priority;
    }

    /// Returns and retains the active source until matching EOI.
    pub fn claim(&mut self) -> Option<Source> {
        self.process_signals();
        if self.claim.is_none() {
            self.claim = [Source::SysTick, Source::Uart0, Source::Uart1, Source::Block]
                .into_iter()
                .filter(|source| self.pending & self.enabled & source.bit() != 0)
                .min_by_key(|source| (self.priority(*source), *source as u32));
        }
        self.claim
    }

    pub const fn claimed(&self) -> Option<Source> {
        self.claim
    }

    fn eoi(&mut self, source: u32) -> Result<()> {
        let expected = self
            .claim
            .map(|claim| claim as u32)
            .ok_or(CoreError::InvalidEoi {
                expected: u32::MAX,
                actual: source,
            })?;
        if source != expected {
            return Err(CoreError::InvalidEoi {
                expected,
                actual: source,
            });
        }
        self.claim = None;
        Ok(())
    }
}

impl Peripheral for InterruptController {
    type Register = Register;
    type Update = InterruptUpdate;
    type Inspection = InterruptInspection;
    type Error = crate::CoreError;

    fn read(&mut self, register: Self::Register) -> Result<u32> {
        Ok(match register {
            Register::Pending => self.pending(),
            Register::Enable => self.enabled(),
            Register::Claim => self.claim().map(|source| source as u32).unwrap_or(u32::MAX),
            Register::PrioritySysTick => self.priority(Source::SysTick) as u32,
            Register::PriorityUart0 => self.priority(Source::Uart0) as u32,
            Register::PriorityUart1 => self.priority(Source::Uart1) as u32,
            Register::PriorityBlock => self.priority(Source::Block) as u32,
            Register::Eoi => 0,
        })
    }

    fn update(&mut self, update: Self::Update) -> Result<()> {
        match update {
            InterruptUpdate::Write {
                register: Register::Enable,
                value,
            } => self.set_enabled(value),
            InterruptUpdate::Write {
                register: Register::Eoi,
                value,
            } => self.eoi(value)?,
            InterruptUpdate::Write {
                register: Register::PrioritySysTick,
                value,
            } => self.set_priority(Source::SysTick, value as u8),
            InterruptUpdate::Write {
                register: Register::PriorityUart0,
                value,
            } => self.set_priority(Source::Uart0, value as u8),
            InterruptUpdate::Write {
                register: Register::PriorityUart1,
                value,
            } => self.set_priority(Source::Uart1, value as u8),
            InterruptUpdate::Write {
                register: Register::PriorityBlock,
                value,
            } => self.set_priority(Source::Block, value as u8),
            InterruptUpdate::Write { .. } => {}
        }
        Ok(())
    }

    fn inspect(&self) -> Self::Inspection {
        InterruptInspection {
            pending: self.pending,
            enabled: self.enabled,
            claim: self.claim.map(|source| source as u32),
            priorities: self.priorities,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use minemu_platform::peripherals::interrupt::Source;

    use super::{InterruptController, InterruptSignal};

    #[test]
    fn claim_is_retained_and_prioritized() {
        let (sender, receiver) = mpsc::channel();
        let mut controller = InterruptController::new(receiver);
        controller.set_enabled(0x0f);
        sender
            .send(InterruptSignal {
                source: Source::Block,
                pending: true,
            })
            .unwrap();
        sender
            .send(InterruptSignal {
                source: Source::Uart1,
                pending: true,
            })
            .unwrap();
        assert_eq!(controller.claim(), Some(Source::Uart1));
        sender
            .send(InterruptSignal {
                source: Source::SysTick,
                pending: true,
            })
            .unwrap();
        assert_eq!(controller.claim(), Some(Source::Uart1));
        controller.eoi(Source::Uart1 as u32).unwrap();
        assert_eq!(controller.claim(), Some(Source::SysTick));
    }

    #[test]
    fn eoi_requires_an_active_claim() {
        let (_, receiver) = mpsc::channel();
        let mut controller = InterruptController::new(receiver);
        assert!(controller.eoi(u32::MAX).is_err());
    }
}
