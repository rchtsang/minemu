//! Read-only inspection of live CPU and memory state at execution boundaries.

use minemu_platform::{MemRegion, PAGE_SIZE, PhysicalAddress, PhysicalRange, VirtualAddress};
use tracing::{debug, debug_span, trace, warn};
use unicorn_engine::unicorn_const::Prot;

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

    /// Reads virtual instruction bytes through the active Unicorn translation state.
    pub fn read_virtual_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
    ) -> Result<Vec<u8>> {
        self.read_translated_memory(address, length, Prot::EXEC, false)
    }

    /// Reads virtual data bytes without allowing inspection to trigger MMIO reads.
    pub fn inspect_virtual_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
    ) -> Result<Vec<u8>> {
        self.read_translated_memory(address, length, Prot::READ, true)
    }

    /// Translates one virtual address using read permissions and current MMU state.
    pub fn translate_virtual_address(
        &mut self,
        address: VirtualAddress,
    ) -> Result<PhysicalAddress> {
        let physical = self
            .engine
            .vmem_translate(u64::from(address.get()), Prot::READ)?;
        let physical = u32::try_from(physical).map_err(|_| BackendError::InspectionRange)?;
        Ok(PhysicalAddress::new(physical))
    }

    fn read_translated_memory(
        &mut self,
        address: VirtualAddress,
        length: usize,
        protection: Prot,
        reject_devices: bool,
    ) -> Result<Vec<u8>> {
        let access = if protection == Prot::EXEC {
            "execute"
        } else {
            "read"
        };
        trace!(
            virtual_address = address.get(),
            length, access, "reading translated Unicorn memory for inspection"
        );
        let mut bytes = vec![0; length];
        let mut offset = 0;
        while offset < length {
            let offset_u32 = u32::try_from(offset).map_err(|_| BackendError::InspectionRange)?;
            let virtual_address = address
                .get()
                .checked_add(offset_u32)
                .ok_or(BackendError::InspectionRange)?;
            let page_remaining = PAGE_SIZE - virtual_address % PAGE_SIZE;
            let count = (length - offset).min(page_remaining as usize);
            let physical_address = self
                .engine
                .vmem_translate(u64::from(virtual_address), protection)
                .map_err(|error| {
                    warn!(
                        virtual_address,
                        length = count,
                        access,
                        error = ?error,
                        "Unicorn virtual inspection translation failed"
                    );
                    BackendError::Unicorn(error)
                })?;
            let physical_u32 =
                u32::try_from(physical_address).map_err(|_| BackendError::InspectionRange)?;
            if reject_devices
                && Option::<MemRegion>::from(PhysicalAddress::new(physical_u32))
                    .is_some_and(MemRegion::is_device)
            {
                return Err(BackendError::VirtualInspectionDevice(physical_u32));
            }
            self.engine
                .mem_read(physical_address, &mut bytes[offset..offset + count])
                .map_err(|error| {
                    warn!(
                        virtual_address,
                        physical_address,
                        length = count,
                        access,
                        error = ?error,
                        "Unicorn translated physical inspection read failed"
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
