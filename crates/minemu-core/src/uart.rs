use std::sync::mpsc::{self, Sender};

use minemu_platform::{
    Peripheral, UartInspection,
    peripherals::{
        interrupt::Source,
        uart::{CONTROL_RX_IRQ_ENABLE, Register, STATUS_RX_READY, STATUS_TX_READY},
    },
};

use crate::{
    InterruptSignal, Result,
    support::{BoundedQueue, IrqLine, flag},
};

/// Default number of received bytes a UART buffers before dropping the oldest.
pub const UART_RX_CAPACITY: usize = 4096;
/// Default number of transmitted bytes a UART retains for inspection.
pub const UART_TX_CAPACITY: usize = 8192;

/// One state-changing UART input.
pub enum UartUpdate {
    Receive(u8),
    Write { register: Register, value: u32 },
}

/// Independent UART state with bounded receive and transmit histories.
pub struct Uart {
    rx: BoundedQueue<u8>,
    tx: BoundedQueue<u8>,
    rx_irq_enabled: bool,
    irq: IrqLine,
}

impl Uart {
    pub fn new(
        rx_capacity: usize,
        tx_capacity: usize,
        interrupt_source: Source,
        interrupt_sender: Sender<InterruptSignal>,
    ) -> Self {
        Self {
            rx: BoundedQueue::new(rx_capacity),
            tx: BoundedQueue::new(tx_capacity),
            rx_irq_enabled: false,
            irq: IrqLine::new(interrupt_source, interrupt_sender),
        }
    }

    fn receive(&mut self, byte: u8) {
        if self.rx.push(byte) {
            self.signal_interrupt();
        }
    }

    fn read_rx(&mut self) -> u8 {
        let byte = self.rx.pop_front().unwrap_or(0);
        self.signal_interrupt();
        byte
    }

    fn write_tx(&mut self, byte: u8) {
        self.tx.push(byte);
    }

    pub fn status(&self) -> u32 {
        flag(!self.rx.is_empty(), STATUS_RX_READY) | STATUS_TX_READY
    }

    pub const fn control(&self) -> u32 {
        flag(self.rx_irq_enabled, CONTROL_RX_IRQ_ENABLE)
    }

    fn set_control(&mut self, value: u32) {
        self.rx_irq_enabled = value & CONTROL_RX_IRQ_ENABLE != 0;
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
        self.irq.set(self.irq_pending());
    }
}

impl Default for Uart {
    fn default() -> Self {
        Self::new(
            UART_RX_CAPACITY,
            UART_TX_CAPACITY,
            Source::Uart0,
            mpsc::channel().0,
        )
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
