//! Read-only inspection of live CPU and memory state at execution boundaries.

use minemu_platform::{
    Access, MemRegion, PAGE_SIZE, PhysicalAddress, PhysicalRange, VirtualAddress,
};
use tracing::{debug, debug_span, trace, warn};
use unicorn_engine::RegisterARM;

use super::{BackendError, ExecutionInspection, MEMORY_SEARCH_CHUNK_SIZE, Result, UnicornBackend};

/// Index of the program counter in [`super::CpuState::registers`].
const PC_INDEX: usize = 15;

impl UnicornBackend {
    /// Reads the authoritative physical bytes currently mapped by Unicorn.
    pub fn read_physical_memory(&self, address: PhysicalAddress, length: usize) -> Result<Vec<u8>> {
        trace!(
            physical_address = address.get(),
            length, "reading physical Unicorn memory for inspection"
        );
        check_inspection_range(address.get(), length)?;
        let mut bytes = vec![0; length];
        match self.engine.mem_read(u64::from(address.get()), &mut bytes) {
            Ok(()) => Ok(bytes),
            Err(error) => {
                warn!(
                    physical_address = address.get(),
                    length,
                    error = ?error,
                    "Unicorn physical inspection read failed"
                );
                Err(BackendError::Unicorn(error))
            }
        }
    }

    /// Reads virtual instruction bytes through the current MMU state.
    pub fn read_virtual_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
    ) -> Result<Vec<u8>> {
        self.read_translated_memory(address, length, Access::Fetch)
    }

    /// Reads virtual data bytes without allowing inspection to trigger MMIO reads.
    pub fn inspect_virtual_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
    ) -> Result<Vec<u8>> {
        self.read_translated_memory(address, length, Access::Read)
    }

    /// Translates one virtual address using read permissions and current MMU state.
    pub fn translate_virtual_address(
        &mut self,
        address: VirtualAddress,
    ) -> Result<PhysicalAddress> {
        self.probe(address, Access::Read)
    }

    /// Translates for inspection in the current CPU mode. Unlike guest
    /// translation, this records no fault, sets no Accessed/Dirty bits, and
    /// fills no Unicorn TLB entry.
    fn probe(&self, address: VirtualAddress, access: Access) -> Result<PhysicalAddress> {
        let user_mode = self
            .register(RegisterARM::CPSR)
            .is_ok_and(super::arm::is_user_mode);
        let machine = self.machine();
        machine
            .mmu
            .probe(&machine.memory, address, access, user_mode)
            .map_err(|cause| BackendError::InspectionTranslation {
                address: address.get(),
                cause,
            })
    }

    fn read_translated_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
        access: Access,
    ) -> Result<Vec<u8>> {
        trace!(
            virtual_address = address.get(),
            length,
            ?access,
            "reading translated memory for inspection"
        );
        check_inspection_range(address.get(), length)?;
        let mut bytes = vec![0; length];
        let mut offset = 0;
        while offset < length {
            let virtual_address = address.get() + offset as u32;
            let page_remaining = PAGE_SIZE - virtual_address % PAGE_SIZE;
            let count = (length - offset).min(page_remaining as usize);
            let physical = self.probe(VirtualAddress::new(virtual_address), access)?;
            // Inspection must never trigger device side effects.
            if Option::<MemRegion>::from(physical).is_some_and(MemRegion::is_device) {
                return Err(BackendError::VirtualInspectionDevice(physical.get()));
            }
            self.engine
                .mem_read(
                    u64::from(physical.get()),
                    &mut bytes[offset..offset + count],
                )
                .map_err(|error| {
                    warn!(
                        virtual_address,
                        physical_address = physical.get(),
                        length = count,
                        ?access,
                        error = ?error,
                        "translated physical inspection read failed"
                    );
                    BackendError::Unicorn(error)
                })?;
            offset += count;
        }
        Ok(bytes)
    }

    /// Captures the authoritative physical bytes currently mapped by Unicorn.
    pub fn inspect_live_memory(&self, range: PhysicalRange) -> Result<Vec<u8>> {
        debug!(
            physical_address = range.start().get(),
            length = range.length(),
            "capturing Unicorn live-memory inspection snapshot"
        );
        self.read_physical_memory(range.start(), range.length() as usize)
    }

    /// Searches the authoritative physical RAM mapping in bounded, overlapping chunks.
    pub fn search_memory(&self, pattern: &[u8]) -> Result<Option<PhysicalAddress>> {
        if pattern.is_empty() {
            warn!("cannot search Unicorn RAM for an empty pattern");
            return Err(BackendError::EmptySearchPattern);
        }
        if pattern.len() > MEMORY_SEARCH_CHUNK_SIZE {
            warn!(
                pattern_length = pattern.len(),
                maximum = MEMORY_SEARCH_CHUNK_SIZE,
                "Unicorn RAM search pattern is too large"
            );
            return Err(BackendError::SearchPatternTooLarge {
                length: pattern.len(),
                maximum: MEMORY_SEARCH_CHUNK_SIZE,
            });
        }

        let ram = MemRegion::Ram.range();
        let ram_length = ram.length() as usize;
        let overlap = pattern.len() - 1;
        debug!(
            physical_address = ram.start().get(),
            length = ram.length(),
            pattern_length = pattern.len(),
            chunk_size = MEMORY_SEARCH_CHUNK_SIZE,
            "searching authoritative Unicorn RAM"
        );

        // TODO: this can probably be optimized
        for offset in (0..ram_length).step_by(MEMORY_SEARCH_CHUNK_SIZE) {
            let length = (MEMORY_SEARCH_CHUNK_SIZE + overlap).min(ram_length - offset);
            let address = PhysicalAddress::new(ram.start().get() + offset as u32);
            let bytes = self.read_physical_memory(address, length)?;
            if let Some(index) = bytes
                .windows(pattern.len())
                .position(|window| window == pattern)
            {
                let address = PhysicalAddress::new(address.get() + index as u32);
                debug!(
                    physical_address = address.get(),
                    pattern_length = pattern.len(),
                    "found byte pattern in Unicorn RAM"
                );
                return Ok(Some(address));
            }
        }

        debug!(
            pattern_length = pattern.len(),
            "byte pattern was not found in Unicorn RAM"
        );
        Ok(None)
    }

    /// Captures CPU state and instruction bytes at one execution boundary.
    pub fn inspect_execution(
        &mut self,
        address: Option<VirtualAddress>,
        before: usize,
        after: usize,
    ) -> Result<ExecutionInspection> {
        let cpu = self.cpu_state().map_err(|error| {
            warn!(before, after, error = %error, "Unicorn CPU inspection failed");
            error
        })?;
        let mmu_enabled = self.machine().mmu.enabled();
        let start = address
            .map(VirtualAddress::get)
            .unwrap_or(cpu.registers[PC_INDEX])
            .saturating_sub(before as u32);
        let Some(length) = before.checked_add(after) else {
            warn!(
                pc = cpu.registers[PC_INDEX],
                cpsr = cpu.cpsr,
                before,
                after,
                "Unicorn execution inspection range overflow"
            );
            return Err(BackendError::InspectionRange);
        };
        let span = debug_span!(
            "unicorn_execution_inspection",
            pc = cpu.registers[PC_INDEX],
            cpsr = cpu.cpsr,
            virtual_address = start,
            length,
            access = "execute",
        );
        let _entered = span.enter();
        debug!("capturing Unicorn execution inspection snapshot");
        let (instruction_bytes, instruction_error) =
            match self.read_virtual_memory(VirtualAddress::new(start), length) {
                Ok(bytes) => (bytes, None),
                Err(error) => {
                    warn!(error = %error, "execution inspection has no readable instruction bytes");
                    (Vec::new(), Some(error.to_string()))
                }
            };
        Ok(ExecutionInspection {
            registers: cpu.registers,
            cpsr: cpu.cpsr,
            spsr: cpu.spsr,
            mmu_enabled,
            instruction_address: VirtualAddress::new(start),
            instruction_bytes,
            instruction_error,
        })
    }
}

/// Rejects a byte range that extends past the 32-bit address space before
/// anything is allocated for it.
fn check_inspection_range(start: u32, length: usize) -> Result<()> {
    u64::try_from(length)
        .ok()
        .and_then(|length| u64::from(start).checked_add(length))
        .filter(|end| *end <= 1 << 32)
        .map(drop)
        .ok_or(BackendError::InspectionRange)
}
