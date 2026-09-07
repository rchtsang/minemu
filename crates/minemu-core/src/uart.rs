use std::{
    collections::VecDeque,
    sync::mpsc::{self, Sender},
};

use minemu_platform::{
    Peripheral, UartInspection,
    peripherals::{interrupt::Source, uart::Register},
};

use crate::{InterruptSignal, Result};

/// One state-changing UART input.
pub enum UartUpdate {
    Receive(u8),
    Write { register: Register, value: u32 },
}

/// Independent UART state with bounded receive and transmit histories.
pub struct Uart {
    rx: VecDeque<u8>,
    tx: VecDeque<u8>,
    rx_capacity: usize,
    tx_capacity: usize,
    rx_irq_enabled: bool,
    interrupt_source: Source,
    interrupt_sender: Sender<InterruptSignal>,
}

impl Uart {
    pub fn new(
        rx_capacity: usize,
        tx_capacity: usize,
        interrupt_source: Source,
        interrupt_sender: Sender<InterruptSignal>,
    ) -> Self {
        Self {
            rx: VecDeque::with_capacity(rx_capacity),
            tx: VecDeque::with_capacity(tx_capacity),
            rx_capacity,
            tx_capacity,
            rx_irq_enabled: false,
            interrupt_source,
            interrupt_sender,
        }
    }

    fn receive(&mut self, byte: u8) {
        if self.rx_capacity == 0 {
            return;
        }
        if self.rx.len() == self.rx_capacity {
            self.rx.pop_front();
        }
        self.rx.push_back(byte);
        self.signal_interrupt();
    }

    fn read_rx(&mut self) -> u8 {
        let byte = self.rx.pop_front().unwrap_or(0);
        self.signal_interrupt();
        byte
    }

    fn write_tx(&mut self, byte: u8) {
        if self.tx_capacity == 0 {
            return;
        }
        if self.tx.len() == self.tx_capacity {
            self.tx.pop_front();
        }
        self.tx.push_back(byte);
    }

    pub fn status(&self) -> u32 {
        (if self.rx.is_empty() { 0 } else { 1 }) | 2
    }

    pub const fn control(&self) -> u32 {
        self.rx_irq_enabled as u32
    }

    fn set_control(&mut self, value: u32) {
        self.rx_irq_enabled = value & 1 != 0;
        self.signal_interrupt();
    }

    pub fn irq_pending(&self) -> bool {
        self.rx_irq_enabled && !self.rx.is_empty()
    }

    pub fn tx_history(&self) -> impl Iterator<Item = u8> + '_ {
        self.tx.iter().copied()
    }

    pub fn inspect(&self) -> UartInspection {
        UartInspection {
            status: self.status(),
            control: self.control(),
            rx_queued: self.rx.len(),
            rx_irq_enabled: self.rx_irq_enabled,
            tx_history: self.tx_history().collect(),
        }
    }

    fn signal_interrupt(&self) {
        let _ = self.interrupt_sender.send(InterruptSignal {
            source: self.interrupt_source,
            pending: self.irq_pending(),
        });
    }
}

impl Default for Uart {
    fn default() -> Self {
        let (sender, _) = mpsc::channel();
        Self::new(4096, 8192, Source::Uart0, sender)
    }
}

impl Peripheral for Uart {
    type Register = Register;
    type Update = UartUpdate;
    type Inspection = UartInspection;
    type Error = crate::CoreError;

    fn read(&mut self, register: Self::Register) -> Result<u32> {
        Ok(match register {
            Register::ReceiveData => self.read_rx() as u32,
            Register::Status => self.status(),
            Register::Control => self.control(),
            Register::TransmitData => 0,
        })
    }

    fn update(&mut self, update: Self::Update) -> Result<()> {
        match update {
            UartUpdate::Receive(byte) => self.receive(byte),
            UartUpdate::Write {
                register: Register::TransmitData,
                value,
            } => self.write_tx(value as u8),
            UartUpdate::Write {
                register: Register::Control,
                value,
            } => self.set_control(value),
            UartUpdate::Write { .. } => {}
        }
        Ok(())
    }

    fn inspect(&self) -> Self::Inspection {
        Uart::inspect(self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use minemu_platform::{
        Peripheral,
        peripherals::{interrupt::Source, uart::Register},
    };

    use super::Uart;

    #[test]
    fn receive_state_drives_level_irq() {
        let (sender, receiver) = mpsc::channel();
        let mut uart = Uart::new(2, 2, Source::Uart0, sender);
        uart.receive(b'a');
        assert_eq!(uart.status(), 3);
        assert!(!uart.irq_pending());
        uart.set_control(1);
        assert!(uart.irq_pending());
        assert_eq!(uart.read(Register::Control).unwrap(), 1);
        assert!(!receiver.try_recv().unwrap().pending);
        assert!(receiver.try_recv().unwrap().pending);
        assert_eq!(uart.read_rx(), b'a');
        assert!(!uart.irq_pending());
    }
}
