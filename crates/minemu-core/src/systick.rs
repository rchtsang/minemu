use std::sync::mpsc::{self, Sender};

use minemu_platform::{
    Peripheral, SysTickInspection,
    peripherals::{interrupt::Source, systick::Register},
};

use crate::{InterruptSignal, Result};

/// One state-changing SysTick input.
pub enum SysTickUpdate {
    Write {
        register: Register,
        value: u32,
        now: u64,
    },
    AdvanceTo(u64),
}

/// SysTick state and exact virtual deadline scheduling.
pub struct SysTick {
    period: u32,
    enabled: bool,
    periodic: bool,
    irq_enabled: bool,
    pending: bool,
    next_deadline: Option<u64>,
    interrupt_sender: Sender<InterruptSignal>,
}

impl SysTick {
    pub fn new(interrupt_sender: Sender<InterruptSignal>) -> Self {
        Self {
            period: 0,
            enabled: false,
            periodic: false,
            irq_enabled: false,
            pending: false,
            next_deadline: None,
            interrupt_sender,
        }
    }
    pub const fn period(&self) -> u32 {
        self.period
    }

    pub const fn control(&self) -> u32 {
        (self.enabled as u32) | ((self.periodic as u32) << 1) | ((self.irq_enabled as u32) << 2)
    }

    pub const fn status(&self) -> u32 {
        self.pending as u32
    }

    pub const fn irq_pending(&self) -> bool {
        self.pending && self.irq_enabled
    }

    fn set_period(&mut self, period: u32, now: u64) {
        self.period = period;
        if self.enabled {
            self.next_deadline = Some(now + u64::from(period));
        }
    }

    fn set_control(&mut self, control: u32, now: u64) {
        let was_enabled = self.enabled;
        self.enabled = control & 1 != 0;
        self.periodic = control & 2 != 0;
        self.irq_enabled = control & 4 != 0;
        if self.enabled && (!was_enabled || self.next_deadline.is_none()) && self.period != 0 {
            self.next_deadline = Some(now + u64::from(self.period));
        }
        if !self.enabled {
            self.next_deadline = None;
        }
    }

    fn ack(&mut self) {
        self.pending = false;
    }

    pub const fn inspect(&self) -> SysTickInspection {
        SysTickInspection {
            period: self.period,
            control: self.control(),
            status: self.status(),
        }
    }

    /// Processes all deadlines no later than `now` and returns whether any expired.
    fn advance_to(&mut self, now: u64) -> bool {
        let Some(mut deadline) = self.next_deadline else {
            return false;
        };
        if deadline > now {
            return false;
        }
        self.pending = true;
        if self.periodic && self.period != 0 {
            while deadline <= now {
                deadline += u64::from(self.period);
            }
            self.next_deadline = Some(deadline);
        } else {
            self.enabled = false;
            self.next_deadline = None;
        }
        true
    }

    fn signal_interrupt(&self) {
        let _ = self.interrupt_sender.send(InterruptSignal {
            source: Source::SysTick,
            pending: self.irq_pending(),
        });
    }
}

impl Default for SysTick {
    fn default() -> Self {
        let (sender, _) = mpsc::channel();
        Self::new(sender)
    }
}

impl Peripheral for SysTick {
    type Register = Register;
    type Update = SysTickUpdate;
    type Inspection = SysTickInspection;
    type Error = crate::CoreError;

    fn read(&mut self, register: Self::Register) -> Result<u32> {
        Ok(match register {
            Register::Period => self.period(),
            Register::Control => self.control(),
            Register::Status => self.status(),
            Register::Ack => 0,
        })
    }

    fn update(&mut self, update: Self::Update) -> Result<()> {
        match update {
            SysTickUpdate::Write {
                register: Register::Period,
                value,
                now,
            } => self.set_period(value, now),
            SysTickUpdate::Write {
                register: Register::Control,
                value,
                now,
            } => self.set_control(value, now),
            SysTickUpdate::Write {
                register: Register::Ack,
                ..
            } => self.ack(),
            SysTickUpdate::Write { .. } => {}
            SysTickUpdate::AdvanceTo(now) => {
                self.advance_to(now);
            }
        }
        self.signal_interrupt();
        Ok(())
    }

    fn inspect(&self) -> Self::Inspection {
        SysTick::inspect(self)
    }
}

#[cfg(test)]
mod tests {
    use super::SysTick;

    #[test]
    fn periodic_deadlines_keep_their_phase() {
        let mut timer = SysTick::default();
        timer.set_period(3, 10);
        timer.set_control(0b111, 10);
        assert!(!timer.advance_to(12));
        assert!(timer.advance_to(13));
        timer.ack();
        assert!(timer.advance_to(20));
        assert!(timer.irq_pending());
    }

    #[test]
    fn period_write_rephases_an_enabled_timer() {
        let mut timer = SysTick::default();
        timer.set_period(3, 0);
        timer.set_control(0b011, 10);
        timer.set_period(5, 11);
        assert!(!timer.advance_to(15));
        assert!(timer.advance_to(16));
    }
}
