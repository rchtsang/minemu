use std::collections::VecDeque;

use minemu_platform::{
    ExceptionKind, InspectionRequest, InspectionResponse, MemRegion, ObservableEvent, Peripheral,
    TraceInspectionEvent, VirtualAddress,
    peripherals::block::{UNIT_FILESYSTEM, UNIT_SWAP},
};

use crate::{ExceptionPlan, MmioBus, Mmu, MmuFault, PhysicalMemory, PhysicalMemoryAccess, Result};

/// Result of a single backend instruction attempt supplied by the CPU adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionOutcome {
    Completed,
    SynchronousException(ExceptionKind, VirtualAddress),
    Fault(MmuFault, VirtualAddress),
}

/// Small continuously publishable machine status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineStatus {
    pub ticks: u64,
    pub mmu_enabled: bool,
    pub pending_interrupts: u32,
    pub systick_status: u32,
    pub block_status: u32,
    pub uart0_status: u32,
    pub uart1_status: u32,
}

/// The single-owner, backend-independent guest machine state.
pub struct Machine {
    pub memory: PhysicalMemory,
    pub bus: MmioBus,
    pub mmu: Mmu,
    ticks: u64,
    events: VecDeque<ObservableEvent>,
    event_capacity: usize,
}

impl Machine {
    pub fn new(memory: PhysicalMemory, event_capacity: usize) -> Self {
        Self {
            memory,
            bus: MmioBus::new(),
            mmu: Mmu::new(),
            ticks: 0,
            events: VecDeque::with_capacity(event_capacity),
            event_capacity,
        }
    }

    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Copies the physical bytes in one implemented region for backend mapping.
    pub fn copy_region(&self, region: MemRegion) -> Result<Vec<u8>> {
        let range = region.range();
        let mut bytes = vec![0; range.length() as usize];
        self.memory.read_range(range, &mut bytes)?;
        Ok(bytes)
    }

    /// Recreates reset machine state while preserving immutable ROM and attached media.
    pub fn reset_clone(&self) -> Result<Self> {
        let boot_rom = self.copy_region(MemRegion::BootRom)?;
        let system_rom = self.copy_region(MemRegion::SystemRom)?;
        let mut machine = Self::new(
            PhysicalMemory::with_roms(&boot_rom, &system_rom)?,
            self.event_capacity,
        );
        for unit in [UNIT_FILESYSTEM, UNIT_SWAP] {
            if let Some(media) = self.bus.block.media_clone(unit)? {
                machine
                    .bus
                    .block
                    .update(crate::BlockUpdate::Attach { unit, media })?;
            }
        }
        Ok(machine)
    }

    /// Applies the virtual-time contract after one attempted guest instruction.
    pub fn finish_instruction(&mut self, outcome: InstructionOutcome) -> Option<ExceptionPlan> {
        match outcome {
            InstructionOutcome::Completed => {
                self.advance_ticks(1);
                None
            }
            InstructionOutcome::SynchronousException(kind, pc) => {
                self.advance_ticks(1);
                self.advance_ticks(kind.entry_ticks());
                self.record_event(ObservableEvent::Exception(kind));
                Some(ExceptionPlan::synchronous(kind, pc))
            }
            InstructionOutcome::Fault(fault, pc) => {
                let plan = ExceptionPlan::fault(fault, pc);
                self.advance_ticks(plan.request.kind.entry_ticks());
                self.record_event(ObservableEvent::Exception(plan.request.kind));
                Some(plan)
            }
        }
    }

    /// Enters an exception at an instruction boundary after its instruction tick.
    pub fn enter_exception(&mut self, kind: ExceptionKind, pc: VirtualAddress) -> ExceptionPlan {
        self.advance_ticks(kind.entry_ticks());
        self.record_event(ObservableEvent::Exception(kind));
        ExceptionPlan::synchronous(kind, pc)
    }

    pub fn status(&mut self) -> MachineStatus {
        MachineStatus {
            ticks: self.ticks,
            mmu_enabled: self.mmu.enabled(),
            pending_interrupts: self.bus.interrupts.pending(),
            systick_status: self.bus.systick.status(),
            block_status: self.bus.block.status(),
            uart0_status: self.bus.uart0.status(),
            uart1_status: self.bus.uart1.status(),
        }
    }

    pub fn inspect(&mut self, request: InspectionRequest) -> Result<InspectionResponse<'_>> {
        match request {
            InspectionRequest::Memory(range) => Ok(InspectionResponse::Memory(
                self.memory.inspect_range(range)?,
            )),
            InspectionRequest::Mmu => Ok(InspectionResponse::Mmu(self.mmu.inspect())),
            InspectionRequest::Peripherals => {
                Ok(InspectionResponse::Peripherals(self.bus.inspect()))
            }
            InspectionRequest::Events => Ok(InspectionResponse::Events(
                self.events.iter().copied().collect(),
            )),
        }
    }

    fn advance_ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.ticks += 1;
            self.bus.advance_to(self.ticks, &mut self.memory);
        }
        for trace in self.bus.retire_trace_events(self.ticks) {
            self.record_event(ObservableEvent::Trace(TraceInspectionEvent {
                tick: trace.tick,
                value: trace.value,
            }));
        }
    }

    fn record_event(&mut self, event: ObservableEvent) {
        if self.event_capacity == 0 {
            return;
        }
        if self.events.len() == self.event_capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new(PhysicalMemory::default(), 4096)
    }
}

#[cfg(test)]
mod tests {
    use minemu_platform::{
        ExceptionKind, InspectionRequest, InspectionResponse, MemRegion, MmioTransaction,
        MmioWidth, ObservableEvent, Peripheral, PhysicalAddress, TraceInspectionEvent,
        VirtualAddress,
    };

    use crate::{BlockUpdate, InstructionOutcome, Machine, PhysicalMemory, PhysicalMemoryAccess};

    #[test]
    fn virtual_time_distinguishes_traps_and_faults() {
        let mut machine = Machine::default();
        machine.finish_instruction(InstructionOutcome::Completed);
        machine.finish_instruction(InstructionOutcome::SynchronousException(
            ExceptionKind::SupervisorCall,
            VirtualAddress::new(0),
        ));
        assert_eq!(machine.ticks(), 3);
        machine.finish_instruction(InstructionOutcome::Fault(
            crate::MmuFault {
                address: VirtualAddress::new(4),
                status: minemu_platform::FaultStatus::new(
                    minemu_platform::FaultCause::Translation,
                    true,
                    minemu_platform::Access::Read,
                ),
            },
            VirtualAddress::new(0x1000),
        ));
        assert_eq!(machine.ticks(), 4);
    }

    #[test]
    fn device_deadlines_include_exception_entry_but_not_faulting_access() {
        let mut machine = Machine::default();
        let systick = MemRegion::SysTick.base().get();
        machine
            .bus
            .access(
                MmioTransaction::write(PhysicalAddress::new(systick), MmioWidth::U32, 3),
                machine.ticks(),
            )
            .unwrap();
        machine
            .bus
            .access(
                MmioTransaction::write(PhysicalAddress::new(systick + 4), MmioWidth::U32, 1),
                machine.ticks(),
            )
            .unwrap();

        machine.finish_instruction(InstructionOutcome::Completed);
        machine.finish_instruction(InstructionOutcome::SynchronousException(
            ExceptionKind::SupervisorCall,
            VirtualAddress::new(0),
        ));
        assert_eq!(machine.ticks(), 3);
        assert_eq!(machine.bus.systick.status(), 1);

        machine
            .bus
            .access(
                MmioTransaction::write(PhysicalAddress::new(systick), MmioWidth::U32, 3),
                machine.ticks(),
            )
            .unwrap();
        machine
            .bus
            .access(
                MmioTransaction::write(PhysicalAddress::new(systick + 4), MmioWidth::U32, 1),
                machine.ticks(),
            )
            .unwrap();
        machine
            .bus
            .access(
                MmioTransaction::write(PhysicalAddress::new(systick + 12), MmioWidth::U32, 1),
                machine.ticks(),
            )
            .unwrap();

        machine.finish_instruction(InstructionOutcome::Fault(
            crate::MmuFault {
                address: VirtualAddress::new(4),
                status: minemu_platform::FaultStatus::new(
                    minemu_platform::FaultCause::Translation,
                    true,
                    minemu_platform::Access::Read,
                ),
            },
            VirtualAddress::new(0x1000),
        ));
        assert_eq!(machine.ticks(), 4);
        assert_eq!(machine.bus.systick.status(), 0);
        machine.finish_instruction(InstructionOutcome::Completed);
        assert_eq!(machine.bus.systick.status(), 0);
        machine.finish_instruction(InstructionOutcome::Completed);
        assert_eq!(machine.ticks(), 6);
        assert_eq!(machine.bus.systick.status(), 1);
    }

    #[test]
    fn trace_events_receive_the_completed_instruction_tick() {
        let mut machine = Machine::default();
        machine
            .bus
            .access(
                MmioTransaction::write(
                    PhysicalAddress::new(MemRegion::Trace.base().get()),
                    MmioWidth::U32,
                    7,
                ),
                machine.ticks(),
            )
            .unwrap();
        machine.finish_instruction(InstructionOutcome::Completed);
        assert_eq!(machine.status().ticks, 1);
        assert_eq!(
            machine.inspect(InspectionRequest::Events).unwrap(),
            InspectionResponse::Events(vec![ObservableEvent::Trace(TraceInspectionEvent {
                tick: 1,
                value: 7,
            })])
        );
    }

    #[test]
    fn reset_clone_preserves_rom_and_clears_ram() {
        let mut machine = Machine::new(PhysicalMemory::with_roms(&[1], &[2]).unwrap(), 4);
        machine
            .memory
            .write_u8(PhysicalAddress::new(MemRegion::Ram.base().get()), 7)
            .unwrap();
        machine
            .bus
            .block
            .update(BlockUpdate::Attach {
                unit: 0,
                media: vec![3; 512],
            })
            .unwrap();
        machine
            .bus
            .block
            .update(BlockUpdate::Attach {
                unit: 1,
                media: vec![4; 512],
            })
            .unwrap();
        let reset = machine.reset_clone().unwrap();
        assert_eq!(reset.memory.read_u8(MemRegion::BootRom.base()).unwrap(), 1);
        assert_eq!(
            reset.memory.read_u8(MemRegion::SystemRom.base()).unwrap(),
            2
        );
        assert_eq!(
            reset
                .memory
                .read_u8(PhysicalAddress::new(MemRegion::Ram.base().get()))
                .unwrap(),
            0
        );
        assert_eq!(reset.bus.block.media_clone(0).unwrap().unwrap()[0], 3);
        assert_eq!(reset.bus.block.media_clone(1).unwrap().unwrap()[0], 4);
        assert_eq!(reset.bus.block.unit(), 0);
        assert_eq!(reset.bus.block.status(), 0);
    }
}
