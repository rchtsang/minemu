use minemu_platform::{
    Access, Cp15Operation, FaultCause, FaultStatus, MemRegion, MmuInspection, PTE_USER, PTE_VALID,
    PageDirectoryEntry, PageTableEntry, PhysicalAddress, VirtualAddress,
};

use crate::PhysicalMemoryAccess;

/// A failed translation or protection check recorded for DFSR and DFAR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MmuFault {
    pub address: VirtualAddress,
    pub status: FaultStatus,
}

/// Backend-independent custom two-level MMU walker.
pub struct Mmu {
    enabled: bool,
    ttbr0: PhysicalAddress,
    last_fault: Option<MmuFault>,
}

impl Mmu {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            ttbr0: PhysicalAddress::new(0),
            last_fault: None,
        }
    }

    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    pub const fn ttbr0(&self) -> PhysicalAddress {
        self.ttbr0
    }

    pub const fn last_fault(&self) -> Option<MmuFault> {
        self.last_fault
    }

    /// Returns the backend-independent MMU inspection projection.
    pub const fn inspect(&self) -> MmuInspection {
        MmuInspection {
            enabled: self.enabled,
            ttbr0: self.ttbr0,
            last_fault_address: match self.last_fault {
                Some(fault) => Some(fault.address.get()),
                None => None,
            },
            last_fault_status: match self.last_fault {
                Some(fault) => Some(fault.status),
                None => None,
            },
        }
    }

    pub fn set_ttbr0(&mut self, ttbr0: PhysicalAddress) {
        self.ttbr0 = ttbr0;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn invalidate_all(&mut self) {
        // This walker deliberately caches no translations. The method represents TLBIALL.
    }

    /// Applies the CP15 operations that alter backend-independent MMU state.
    ///
    /// VBAR and fault-register operations belong to the CPU adapter because this
    /// core has no CPU register file or exception-entry mechanism.
    pub fn apply_cp15(&mut self, operation: Cp15Operation) {
        match operation {
            Cp15Operation::SetTtbr0(address) => self.set_ttbr0(address),
            Cp15Operation::SetMmuEnabled(enabled) => self.set_enabled(enabled),
            Cp15Operation::InvalidateAll => self.invalidate_all(),
            Cp15Operation::SetVectorBase(_)
            | Cp15Operation::ReadFaultStatus
            | Cp15Operation::ReadFaultAddress => {}
        }
    }

    pub fn translate(
        &mut self,
        memory: &mut dyn PhysicalMemoryAccess,
        address: VirtualAddress,
        access: Access,
        user_mode: bool,
    ) -> std::result::Result<PhysicalAddress, MmuFault> {
        if !self.enabled {
            return Ok(PhysicalAddress::new(address.get()));
        }
        let fault = |walker: &mut Self, cause| {
            let fault = MmuFault {
                address,
                status: FaultStatus::new(cause, user_mode, access),
            };
            walker.last_fault = Some(fault);
            fault
        };
        let directory_index = (address.get() >> 22) & 0x3ff;
        let directory_address = PhysicalAddress::new(self.ttbr0.get() + directory_index * 4);
        let directory = memory
            .read_u32(directory_address)
            .ok()
            .and_then(|raw| PageDirectoryEntry::new(raw).ok())
            .and_then(|entry| entry.table_address().ok().flatten())
            .ok_or_else(|| fault(self, FaultCause::Translation))?;
        let table_index = (address.get() >> 12) & 0x3ff;
        let entry_address = PhysicalAddress::new(directory.get() + table_index * 4);
        let entry = memory
            .read_u32(entry_address)
            .ok()
            .map(PageTableEntry::new)
            .ok_or_else(|| fault(self, FaultCause::Translation))?;
        if entry.raw() & PTE_VALID == 0 {
            return Err(fault(self, FaultCause::Translation));
        }
        if let Err(cause) = entry.authorize(access, user_mode) {
            return Err(fault(self, cause));
        }
        let raw_target = PhysicalAddress::new(entry.raw() & !0x0fff);
        if entry.raw() & PTE_USER != 0
            && Option::<MemRegion>::from(raw_target).is_some_and(MemRegion::is_device)
        {
            return Err(fault(
                self,
                match access {
                    Access::Fetch => FaultCause::ExecuteProtection,
                    Access::Read => FaultCause::ReadProtection,
                    Access::Write => FaultCause::WriteProtection,
                },
            ));
        }
        let target = entry
            .target_address()
            .ok()
            .flatten()
            .ok_or_else(|| fault(self, FaultCause::Translation))?;
        if matches!(access, Access::Write)
            && matches!(
                Option::<MemRegion>::from(target),
                Some(MemRegion::BootRom | MemRegion::SystemRom)
            )
        {
            return Err(fault(self, FaultCause::WriteProtection));
        }
        let updated = entry.record_successful_access(access);
        if updated.raw() != entry.raw() && memory.write_u32(entry_address, updated.raw()).is_err() {
            return Err(fault(self, FaultCause::Translation));
        }
        Ok(PhysicalAddress::new(
            target.get() | (address.get() & 0x0fff),
        ))
    }
}

impl Default for Mmu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use minemu_platform::{
        Access, FaultCause, MemRegion, PTE_EXECUTABLE, PTE_READABLE, PTE_USER, PTE_VALID,
        PhysicalAddress, VirtualAddress,
    };

    use crate::{Mmu, PhysicalMemory, PhysicalMemoryAccess};

    #[test]
    fn walk_updates_accessed_bit_after_successful_translation() {
        let mut memory = PhysicalMemory::default();
        let ram = MemRegion::Ram.base().get();
        memory
            .write_u32(PhysicalAddress::new(ram), (ram + 0x1000) | PTE_VALID)
            .unwrap();
        memory
            .write_u32(
                PhysicalAddress::new(ram + 0x1000),
                (ram + 0x2000) | PTE_VALID | PTE_READABLE | PTE_EXECUTABLE,
            )
            .unwrap();
        let mut mmu = Mmu::new();
        mmu.set_ttbr0(PhysicalAddress::new(ram));
        mmu.set_enabled(true);
        assert_eq!(
            mmu.translate(&mut memory, VirtualAddress::new(0), Access::Read, false)
                .unwrap(),
            PhysicalAddress::new(ram + 0x2000)
        );
        assert_ne!(
            memory.read_u32(PhysicalAddress::new(ram + 0x1000)).unwrap() & (1 << 5),
            0
        );
    }

    #[test]
    fn user_device_mapping_is_a_protection_fault() {
        let mut memory = PhysicalMemory::default();
        let ram = MemRegion::Ram.base().get();
        memory
            .write_u32(PhysicalAddress::new(ram), (ram + 0x1000) | PTE_VALID)
            .unwrap();
        memory
            .write_u32(
                PhysicalAddress::new(ram + 0x1000),
                MemRegion::Rng.base().get() | PTE_VALID | PTE_READABLE | PTE_USER,
            )
            .unwrap();
        let mut mmu = Mmu::new();
        mmu.set_ttbr0(PhysicalAddress::new(ram));
        mmu.set_enabled(true);
        assert_eq!(
            mmu.translate(&mut memory, VirtualAddress::new(0), Access::Read, true)
                .unwrap_err()
                .status
                .cause(),
            Some(FaultCause::ReadProtection)
        );
    }
}
