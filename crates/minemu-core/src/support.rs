//! Small building blocks shared by the core peripherals.

use std::{collections::VecDeque, sync::mpsc::Sender};

use minemu_platform::peripherals::interrupt::Source;

use crate::InterruptSignal;

/// Returns `bit` when `set`, otherwise zero.
pub(crate) const fn flag(set: bool, bit: u32) -> u32 {
    if set { bit } else { 0 }
}

/// A FIFO that keeps at most `capacity` items by discarding the oldest.
pub(crate) struct BoundedQueue<T> {
    items: VecDeque<T>,
    capacity: usize,
}

impl<T> BoundedQueue<T> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            items: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Appends `item`, evicting the oldest item when full.
    ///
    /// Returns `false` without storing anything when the capacity is zero.
    pub(crate) fn push(&mut self, item: T) -> bool {
        if self.capacity == 0 {
            return false;
        }
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(item);
        true
    }

    pub(crate) fn pop_front(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> + '_ {
        self.items.iter()
    }

    pub(crate) const fn capacity(&self) -> usize {
        self.capacity
    }
}

/// One peripheral's level-triggered connection to the interrupt controller.
pub(crate) struct IrqLine {
    source: Source,
    sender: Sender<InterruptSignal>,
}

impl IrqLine {
    pub(crate) const fn new(source: Source, sender: Sender<InterruptSignal>) -> Self {
        Self { source, sender }
    }

    /// Reports the current interrupt level. A missing controller is ignored.
    pub(crate) fn set(&self, pending: bool) {
        let _ = self.sender.send(InterruptSignal {
            source: self.source,
            pending,
        });
    }
}
