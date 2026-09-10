use std::{
    collections::BTreeSet,
    io,
    sync::mpsc::{self, Sender},
};

use minemu_platform::{
    BlockInspection, BlockUnitInspection, MemRegion, Peripheral, PhysicalAddress, PhysicalRange,
    peripherals::{
        block::{Error, STATUS_BUSY, STATUS_COMPLETE, STATUS_ERROR, UNIT_COUNT},
        interrupt::Source,
    },
};

use crate::{CoreError, InterruptSignal, PhysicalMemoryAccess, Result};

const SECTOR_SIZE: usize = 512;
const COMPLETION_DELAY: u64 = 32;

/// One state-changing block-device input.
pub enum BlockUpdate {
    Attach {
        unit: u32,
        media: Vec<u8>,
    },
    Detach {
        unit: u32,
    },
    Write {
        register: minemu_platform::peripherals::block::Register,
        value: u32,
        now: u64,
    },
    FlushFailed {
        unit: u32,
    },
}

#[derive(Clone, Copy)]
struct Request {
    command: u32,
    lba: u32,
    sector_count: u32,
    dma_address: PhysicalAddress,
    unit: u32,
    deadline: u64,
}

#[derive(Default)]
struct MediaUnit {
    media: Option<Vec<u8>>,
    dirty_sectors: BTreeSet<u64>,
}

/// Write-back block media and its guest-visible command state.
pub struct BlockDevice {
    lba: u32,
    sector_count: u32,
    dma_address: PhysicalAddress,
    unit: u32,
    completion_irq_enabled: bool,
    complete: bool,
    error: Error,
    active: Option<Request>,
    units: [MediaUnit; UNIT_COUNT],
    interrupt_sender: Sender<InterruptSignal>,
}

impl BlockDevice {
    pub fn new(interrupt_sender: Sender<InterruptSignal>) -> Self {
        Self {
            lba: 0,
            sector_count: 0,
            dma_address: PhysicalAddress::new(0),
            unit: 0,
            completion_irq_enabled: false,
            complete: false,
            error: Error::None,
            active: None,
            units: std::array::from_fn(|_| MediaUnit::default()),
            interrupt_sender,
        }
    }

    fn unit_index(unit: u32) -> Result<usize> {
        usize::try_from(unit)
            .ok()
            .filter(|index| *index < UNIT_COUNT)
            .ok_or(CoreError::InvalidBlockUnit(unit))
    }

    fn attach(&mut self, unit: u32, media: Vec<u8>) -> Result<()> {
        if media.is_empty() || !media.len().is_multiple_of(SECTOR_SIZE) {
            return Err(CoreError::InvalidBlockMedia { unit });
        }
        let state = &mut self.units[Self::unit_index(unit)?];
        state.media = Some(media);
        state.dirty_sectors.clear();
        Ok(())
    }

    fn detach(&mut self, unit: u32) -> Result<()> {
        let state = &mut self.units[Self::unit_index(unit)?];
        state.media = None;
        state.dirty_sectors.clear();
        Ok(())
    }

    pub const fn status(&self) -> u32 {
        (if self.active.is_some() {
            STATUS_BUSY
        } else {
            0
        }) | (if self.complete { STATUS_COMPLETE } else { 0 })
            | (if matches!(self.error, Error::None) {
                0
            } else {
                STATUS_ERROR
            })
    }

    pub const fn error(&self) -> Error {
        self.error
    }

    pub const fn lba(&self) -> u32 {
        self.lba
    }

    pub const fn sector_count(&self) -> u32 {
        self.sector_count
    }

    pub const fn dma_address(&self) -> PhysicalAddress {
        self.dma_address
    }

    pub const fn unit(&self) -> u32 {
        self.unit
    }

    pub const fn irq_pending(&self) -> bool {
        self.completion_irq_enabled && self.complete
    }

    pub const fn control(&self) -> u32 {
        self.completion_irq_enabled as u32
    }

    fn set_lba(&mut self, lba: u32) {
        self.lba = lba;
    }

    fn set_sector_count(&mut self, sector_count: u32) {
        self.sector_count = sector_count;
    }

    fn set_dma_address(&mut self, dma_address: PhysicalAddress) {
        self.dma_address = dma_address;
    }

    fn set_unit(&mut self, unit: u32) {
        self.unit = unit;
    }

    fn set_control(&mut self, control: u32) {
        self.completion_irq_enabled = control & 1 != 0;
    }

    /// Starts an accepted command, including one that will later report a guest error.
    fn command(&mut self, command: u32, now: u64) {
        if self.active.is_some() {
            self.complete = true;
            self.error = Error::Busy;
            return;
        }
        self.complete = false;
        self.error = Error::None;
        self.active = Some(Request {
            command,
            lba: self.lba,
            sector_count: self.sector_count,
            dma_address: self.dma_address,
            unit: self.unit,
            deadline: now + COMPLETION_DELAY,
        });
    }

    fn ack(&mut self) {
        self.complete = false;
        self.error = Error::None;
    }

    /// Completes a due request and returns whether the block IRQ level changed to pending.
    pub(crate) fn advance_to(&mut self, now: u64, memory: &mut dyn PhysicalMemoryAccess) -> bool {
        let Some(request) = self.active else {
            return false;
        };
        if request.deadline > now {
            return false;
        }
        self.active = None;
        self.error = self.execute(request, memory).unwrap_or_else(|error| error);
        self.complete = true;
        self.signal_interrupt();
        self.irq_pending()
    }

    pub fn flush(
        &mut self,
        unit: u32,
        mut write_media: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> Result<()> {
        let state = &mut self.units[Self::unit_index(unit)?];
        if state.dirty_sectors.is_empty() {
            return Ok(());
        }
        let media = state
            .media
            .as_deref()
            .ok_or(CoreError::InvalidBlockMedia { unit })?;
        if let Err(error) = write_media(media) {
            self.mark_flush_failed();
            return Err(CoreError::BlockFlush {
                unit,
                source: error,
            });
        }
        self.units[Self::unit_index(unit)?].dirty_sectors.clear();
        Ok(())
    }

    fn mark_flush_failed(&mut self) {
        self.error = Error::DeferredPersistence;
        self.complete = true;
        self.signal_interrupt();
    }

    pub fn dirty_sector_count(&self, unit: u32) -> Result<usize> {
        Ok(self.units[Self::unit_index(unit)?].dirty_sectors.len())
    }

    /// Clones attached write-back media for a machine reset.
    pub fn media_clone(&self, unit: u32) -> Result<Option<Vec<u8>>> {
        Ok(self.units[Self::unit_index(unit)?].media.clone())
    }

    pub fn inspect(&self) -> BlockInspection {
        BlockInspection {
            lba: self.lba,
            sector_count: self.sector_count,
            dma_address: self.dma_address.get(),
            control: self.control(),
            status: self.status(),
            error: self.error as u32,
            unit: self.unit,
            active_unit: self.active.map(|request| request.unit),
            units: self.units.each_ref().map(|state| BlockUnitInspection {
                dirty_sector_count: state.dirty_sectors.len(),
                media_attached: state.media.is_some(),
            }),
        }
    }

    fn execute(
        &mut self,
        request: Request,
        memory: &mut dyn PhysicalMemoryAccess,
    ) -> std::result::Result<Error, Error> {
        let unit = usize::try_from(request.unit)
            .ok()
            .filter(|index| *index < UNIT_COUNT)
            .ok_or(Error::InvalidUnit)?;
        if !matches!(request.command, 1 | 2) {
            return Err(Error::InvalidCommand);
        }
        let byte_count = usize::try_from(request.sector_count)
            .ok()
            .and_then(|count| count.checked_mul(SECTOR_SIZE))
            .ok_or(Error::InvalidLba)?;
        let byte_count_u32 = u32::try_from(byte_count).map_err(|_| Error::InvalidDma)?;
        let dma_range = PhysicalRange::new(request.dma_address, byte_count_u32)
            .map_err(|_| Error::InvalidDma)?;
        if request.dma_address.get() & (SECTOR_SIZE as u32 - 1) != 0
            || !MemRegion::Ram.range().contains_range(dma_range)
        {
            return Err(Error::InvalidDma);
        }
        let state = &mut self.units[unit];
        let media = state.media.as_mut().ok_or(Error::NoMedia)?;
        let start = usize::try_from(request.lba)
            .ok()
            .and_then(|lba| lba.checked_mul(SECTOR_SIZE))
            .ok_or(Error::InvalidLba)?;
        let end = start.checked_add(byte_count).ok_or(Error::InvalidLba)?;
        let sectors = media.get_mut(start..end).ok_or(Error::InvalidLba)?;
        match request.command {
            1 => memory
                .write_range(request.dma_address, sectors)
                .map_err(|_| Error::InvalidDma)?,
            2 => {
                memory
                    .read_range(dma_range, sectors)
                    .map_err(|_| Error::InvalidDma)?;
                for sector in request.lba..request.lba + request.sector_count {
                    state.dirty_sectors.insert(u64::from(sector));
                }
            }
            _ => unreachable!("command was validated above"),
        }
        Ok(Error::None)
    }

    fn signal_interrupt(&self) {
        let _ = self.interrupt_sender.send(InterruptSignal {
            source: Source::Block,
            pending: self.irq_pending(),
        });
    }
}

impl Default for BlockDevice {
    fn default() -> Self {
        let (sender, _) = mpsc::channel();
        Self::new(sender)
    }
}

impl Peripheral for BlockDevice {
    type Register = minemu_platform::peripherals::block::Register;
    type Update = BlockUpdate;
    type Inspection = BlockInspection;
    type Error = crate::CoreError;

    fn read(&mut self, register: Self::Register) -> Result<u32> {
        use minemu_platform::peripherals::block::Register;

        Ok(match register {
            Register::Lba => self.lba(),
            Register::SectorCount => self.sector_count(),
            Register::PhysicalAddress => self.dma_address().get(),
            Register::Status => self.status(),
            Register::Error => self.error() as u32,
            Register::Control => self.control(),
            Register::Unit => self.unit(),
            Register::Command | Register::Ack => 0,
        })
    }

    fn update(&mut self, update: Self::Update) -> Result<()> {
        use minemu_platform::peripherals::block::Register;

        match update {
            BlockUpdate::Attach { unit, media } => self.attach(unit, media)?,
            BlockUpdate::Detach { unit } => self.detach(unit)?,
            BlockUpdate::FlushFailed { unit } => {
                Self::unit_index(unit)?;
                self.mark_flush_failed();
            }
            BlockUpdate::Write {
                register: Register::Command,
                value,
                now,
            } => self.command(value, now),
            BlockUpdate::Write {
                register: Register::Lba,
                value,
                ..
            } => self.set_lba(value),
            BlockUpdate::Write {
                register: Register::SectorCount,
                value,
                ..
            } => self.set_sector_count(value),
            BlockUpdate::Write {
                register: Register::PhysicalAddress,
                value,
                ..
            } => self.set_dma_address(PhysicalAddress::new(value)),
            BlockUpdate::Write {
                register: Register::Ack,
                ..
            } => self.ack(),
            BlockUpdate::Write {
                register: Register::Control,
                value,
                ..
            } => self.set_control(value),
            BlockUpdate::Write {
                register: Register::Unit,
                value,
                ..
            } => self.set_unit(value),
            BlockUpdate::Write { .. } => {}
        }
        self.signal_interrupt();
        Ok(())
    }

    fn inspect(&self) -> Self::Inspection {
        BlockDevice::inspect(self)
    }
}

#[cfg(test)]
mod tests {
    use minemu_platform::{
        MemRegion, Peripheral, PhysicalAddress,
        peripherals::block::{Error, Register, STATUS_BUSY, STATUS_COMPLETE, STATUS_ERROR},
    };

    use crate::{PhysicalMemory, PhysicalMemoryAccess};

    use super::{BlockDevice, SECTOR_SIZE};

    #[test]
    fn write_completes_deterministically_and_marks_media_dirty() {
        let mut block = BlockDevice::default();
        block.attach(1, vec![0; SECTOR_SIZE]).unwrap();
        let mut memory = PhysicalMemory::default();
        let dma = PhysicalAddress::new(MemRegion::Ram.base().get() + 0x200);
        memory.write_u8(dma, 0x5a).unwrap();
        block.set_sector_count(1);
        block.set_dma_address(dma);
        block.set_unit(1);
        block.command(2, 10);
        assert!(!block.advance_to(41, &mut memory));
        block.advance_to(42, &mut memory);
        assert_eq!(block.dirty_sector_count(0).unwrap(), 0);
        assert_eq!(block.dirty_sector_count(1).unwrap(), 1);
        assert_eq!(block.media_clone(1).unwrap().unwrap()[0], 0x5a);
    }

    #[test]
    fn failed_flush_keeps_dirty_sectors_for_retry() {
        let mut block = BlockDevice::default();
        block.attach(0, vec![0; SECTOR_SIZE]).unwrap();
        let mut memory = PhysicalMemory::default();
        let dma = PhysicalAddress::new(MemRegion::Ram.base().get() + 0x200);
        block.set_sector_count(1);
        block.set_dma_address(dma);
        block.command(2, 0);
        block.advance_to(32, &mut memory);
        assert!(
            block
                .flush(0, |_| Err(std::io::Error::other("disk unavailable")))
                .is_err()
        );
        assert_eq!(block.dirty_sector_count(0).unwrap(), 1);
        assert!(block.flush(0, |_| Ok(())).is_ok());
        assert_eq!(block.dirty_sector_count(0).unwrap(), 0);
    }

    #[test]
    fn control_register_reads_back_irq_enable() {
        let mut block = BlockDevice::default();
        block.set_control(1);
        assert_eq!(block.read(Register::Control).unwrap(), 1);
    }

    #[test]
    fn command_snapshots_unit_and_keeps_one_controller_busy() {
        let mut block = BlockDevice::default();
        block.attach(0, vec![0x11; SECTOR_SIZE]).unwrap();
        block.attach(1, vec![0x22; SECTOR_SIZE]).unwrap();
        let mut memory = PhysicalMemory::default();
        let dma = PhysicalAddress::new(MemRegion::Ram.base().get() + 0x200);
        block.set_sector_count(1);
        block.set_dma_address(dma);
        block.command(1, 0);
        block.set_unit(1);
        assert_eq!(block.inspect().active_unit, Some(0));
        block.command(1, 1);
        assert_eq!(block.status() & STATUS_BUSY, STATUS_BUSY);
        assert_eq!(block.error(), Error::Busy);
        block.advance_to(32, &mut memory);
        assert_eq!(memory.read_u8(dma).unwrap(), 0x11);
        assert_eq!(block.unit(), 1);
        assert_eq!(block.inspect().active_unit, None);
    }

    #[test]
    fn invalid_and_unattached_units_fail_at_the_normal_deadline() {
        let mut block = BlockDevice::default();
        let mut memory = PhysicalMemory::default();
        let dma = PhysicalAddress::new(MemRegion::Ram.base().get() + 0x200);
        block.set_sector_count(1);
        block.set_dma_address(dma);
        block.set_unit(2);
        block.command(1, 5);
        assert!(!block.advance_to(36, &mut memory));
        assert_eq!(block.status(), STATUS_BUSY);
        block.advance_to(37, &mut memory);
        assert_eq!(block.error(), Error::InvalidUnit);
        assert_eq!(block.status(), STATUS_COMPLETE | STATUS_ERROR);

        block.ack();
        block.set_unit(1);
        block.command(1, 37);
        block.advance_to(69, &mut memory);
        assert_eq!(block.error(), Error::NoMedia);
    }

    #[test]
    fn media_and_dirty_state_are_independent_per_unit() {
        let mut block = BlockDevice::default();
        block.attach(0, vec![0; SECTOR_SIZE]).unwrap();
        block.attach(1, vec![0; SECTOR_SIZE]).unwrap();
        let mut memory = PhysicalMemory::default();
        let dma = PhysicalAddress::new(MemRegion::Ram.base().get() + 0x200);
        memory.write_u8(dma, 0x5a).unwrap();
        block.set_sector_count(1);
        block.set_dma_address(dma);
        block.set_unit(1);
        block.command(2, 0);
        block.advance_to(32, &mut memory);

        assert_eq!(block.media_clone(0).unwrap().unwrap()[0], 0);
        assert_eq!(block.media_clone(1).unwrap().unwrap()[0], 0x5a);
        assert_eq!(block.dirty_sector_count(0).unwrap(), 0);
        assert_eq!(block.dirty_sector_count(1).unwrap(), 1);
        let inspection = block.inspect();
        assert_eq!(inspection.units[0].dirty_sector_count, 0);
        assert_eq!(inspection.units[1].dirty_sector_count, 1);
    }
}
