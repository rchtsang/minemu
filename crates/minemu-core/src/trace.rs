use minemu_platform::{Peripheral, TraceInspectionEvent, peripherals::trace::Register};

use crate::{Result, support::BoundedQueue};

/// Default number of committed trace events retained for inspection.
pub const TRACE_CAPACITY: usize = 4096;

/// One state-changing trace-device input.
pub enum TraceUpdate {
    Write { register: Register, value: u32 },
}

/// One committed guest trace event.
pub type TraceEvent = TraceInspectionEvent;

/// Bounded trace state. Writes are staged until the issuing instruction retires.
pub struct TraceDevice {
    staged: Vec<u32>,
    events: BoundedQueue<TraceEvent>,
}

impl TraceDevice {
    pub fn new(capacity: usize) -> Self {
        Self {
            staged: Vec::new(),
            events: BoundedQueue::new(capacity),
        }
    }

    fn stage(&mut self, value: u32) {
        self.staged.push(value);
    }

    /// Commits staged writes at `tick` and returns the events that were retained.
    pub(crate) fn retire(&mut self, tick: u64) -> Vec<TraceEvent> {
        let mut committed = Vec::new();
        for value in std::mem::take(&mut self.staged) {
            let event = TraceEvent { tick, value };
            if self.events.push(event) {
                committed.push(event);
            }
        }
        committed
    }

    pub fn events(&self) -> impl Iterator<Item = TraceEvent> + '_ {
        self.events.iter().copied()
    }

    pub fn inspect(&self) -> Vec<TraceInspectionEvent> {
        self.events().collect()
    }
}

impl Default for TraceDevice {
    fn default() -> Self {
        Self::new(TRACE_CAPACITY)
    }
}

impl Peripheral for TraceDevice {
    type Register = Register;
    type Update = TraceUpdate;
    type Inspection = Vec<TraceInspectionEvent>;
    type Error = crate::CoreError;

    fn read(&mut self, _register: Self::Register) -> Result<u32> {
        Ok(0)
    }

    fn update(&mut self, update: Self::Update) -> Result<()> {
        let TraceUpdate::Write { register, value } = update;
        if matches!(register, Register::Event) {
            self.stage(value);
        }
        Ok(())
    }

    fn inspect(&self) -> Self::Inspection {
        TraceDevice::inspect(self)
    }
}

#[cfg(test)]
mod tests {
    use super::TraceDevice;

    #[test]
    fn staged_events_commit_at_retirement_and_remain_bounded() {
        let mut trace = TraceDevice::new(1);
        trace.stage(1);
        assert_eq!(trace.events().count(), 0);
        trace.retire(5);
        trace.stage(2);
        trace.retire(7);
        assert_eq!(trace.events().collect::<Vec<_>>()[0].value, 2);
    }
}
