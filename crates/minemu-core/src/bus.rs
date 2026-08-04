use std::sync::mpsc;

use minemu_platform::{
    Access, MmioRegister, MmioTransaction, Peripheral, PeripheralsInspection, decode_mmio,
    peripherals::{interrupt, trace},
};

use crate::{
    BlockDevice, InterruptController, InterruptUpdate, PhysicalMemoryAccess, Result, Rng,
    RngUpdate, SysTick, SysTickUpdate, TraceDevice, TraceEvent, TraceUpdate, Uart, UartUpdate,
};

/// Typed MMIO bus and all backend-independent peripheral state.
pub struct MmioBus {
    pub interrupts: InterruptController,
    pub systick: SysTick,
    pub block: BlockDevice,
    pub rng: Rng,
    pub uart0: Uart,
    pub uart1: Uart,
    pub trace: TraceDevice,
}

impl MmioBus {
    pub fn new() -> Self {
        let (interrupt_sender, interrupt_receiver) = mpsc::channel();
        Self {
            interrupts: InterruptController::new(interrupt_receiver),
            systick: SysTick::new(interrupt_sender.clone()),
            block: BlockDevice::new(interrupt_sender.clone()),
            rng: Rng::new(),
            uart0: Uart::new(
                4096,
                8192,
                interrupt::Source::Uart0,
                interrupt_sender.clone(),
            ),
            uart1: Uart::new(4096, 8192, interrupt::Source::Uart1, interrupt_sender),
            trace: TraceDevice::default(),
        }
    }

    /// Validates and executes one typed MMIO transaction.
    pub fn access(&mut self, transaction: MmioTransaction, now: u64) -> Result<Option<u32>> {
        let register = decode_mmio(transaction)?;
        let value = match transaction.access {
            Access::Read => Some(match register {
                MmioRegister::Interrupt(register) => self.interrupts.read(register)?,
                MmioRegister::SysTick(register) => self.systick.read(register)?,
                MmioRegister::Dma(register) => self.block.read(register)?,
                MmioRegister::Rng(register) => self.rng.read(register)?,
                MmioRegister::Uart0(register) => self.uart0.read(register)?,
                MmioRegister::Uart1(register) => self.uart1.read(register)?,
                MmioRegister::Trace(_) => unreachable!("platform decoder rejects trace reads"),
            }),
            Access::Write => {
                match register {
                    MmioRegister::Interrupt(register) => {
                        self.interrupts.update(InterruptUpdate::Write {
                            register,
                            value: transaction.value,
                        })?
                    }
                    MmioRegister::SysTick(register) => {
                        self.systick.update(SysTickUpdate::Write {
                            register,
                            value: transaction.value,
                            now,
                        })?
                    }
                    MmioRegister::Dma(register) => {
                        self.block.update(crate::BlockUpdate::Write {
                            register,
                            value: transaction.value,
                            now,
                        })?
                    }
                    MmioRegister::Rng(register) => self.rng.update(RngUpdate::Write {
                        register,
                        value: transaction.value,
                    })?,
                    MmioRegister::Uart0(register) => self.uart0.update(UartUpdate::Write {
                        register,
                        value: transaction.value,
                    })?,
                    MmioRegister::Uart1(register) => self.uart1.update(UartUpdate::Write {
                        register,
                        value: transaction.value,
                    })?,
                    MmioRegister::Trace(trace::Register::Event) => self
                        .trace
                        .update(TraceUpdate::Write {
                            register: trace::Register::Event,
                            value: transaction.value,
                        })
                        .expect("platform-decoded trace write"),
                }
                None
            }
            Access::Fetch => unreachable!("platform decoder rejects MMIO fetches"),
        };
        self.interrupts.process_signals();
        Ok(value)
    }

    /// Completes all deadlines at `now` before the next instruction begins.
    pub fn advance_to(&mut self, now: u64, memory: &mut dyn PhysicalMemoryAccess) {
        self.systick
            .update(SysTickUpdate::AdvanceTo(now))
            .expect("infallible timer update");
        self.block.advance_to(now, memory);
        self.interrupts.process_signals();
    }

    pub fn receive_uart(&mut self, index: u8, byte: u8) {
        match index {
            0 => self
                .uart0
                .update(UartUpdate::Receive(byte))
                .expect("infallible UART update"),
            1 => self
                .uart1
                .update(UartUpdate::Receive(byte))
                .expect("infallible UART update"),
            _ => (),
        }
        self.interrupts.process_signals();
    }

    pub fn retire_trace_events(&mut self, tick: u64) -> Vec<TraceEvent> {
        self.trace.retire(tick)
    }

    pub fn inspect(&mut self) -> PeripheralsInspection {
        self.interrupts.process_signals();
        PeripheralsInspection {
            interrupts: self.interrupts.inspect(),
            systick: self.systick.inspect(),
            block: self.block.inspect(),
            rng: self.rng.inspect(),
            uart0: self.uart0.inspect(),
            uart1: self.uart1.inspect(),
            trace: self.trace.inspect(),
        }
    }
}

impl Default for MmioBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use minemu_platform::{MmioTransaction, MmioWidth, PhysicalAddress};

    use super::MmioBus;

    #[test]
    fn typed_bus_delegates_mmio_validation() {
        let mut bus = MmioBus::new();
        assert!(
            bus.access(
                MmioTransaction::read(PhysicalAddress::new(0x1000_4004), MmioWidth::U32),
                0,
            )
            .is_err()
        );
        assert!(
            bus.access(
                MmioTransaction::read(PhysicalAddress::new(0x1000_4000), MmioWidth::U8),
                0,
            )
            .is_err()
        );
    }
}
