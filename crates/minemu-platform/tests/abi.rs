use minemu_platform::{
    Access, BootInfo, Cp15Operation, FaultCause, FaultStatus, IMAGE_HEADER_SIZE, ImageHeader,
    KernelSegment, MemRegion, MmioRegister, MmioTransaction, MmioWidth, PAGE_SIZE, PTE_ACCESSED,
    PTE_DIRTY, PTE_EXECUTABLE, PTE_READABLE, PTE_USER, PTE_VALID, PTE_WRITABLE, PageDirectoryEntry,
    PageTableEntry, PhysicalAddress, PhysicalRange, PlatformError, VirtualAddress, decode_mmio,
    direct_map_physical, direct_map_virtual,
    peripherals::{block, interrupt, rng},
};

#[test]
fn ranges_and_direct_map_are_checked() {
    assert_eq!(
        PhysicalRange::new(PhysicalAddress::new(0), 0),
        Err(PlatformError::EmptyRange)
    );
    assert!(matches!(
        PhysicalRange::new(PhysicalAddress::new(u32::MAX), 2),
        Err(PlatformError::AddressRangeOverflow { .. })
    ));

    let ram_page = PhysicalRange::new(MemRegion::Ram.base(), PAGE_SIZE).unwrap();
    assert!(MemRegion::Ram.range().contains_range(ram_page));
    assert_eq!(
        direct_map_virtual(PhysicalAddress::new(MemRegion::Ram.base().get() + 0x8000))
            .unwrap()
            .get(),
        0xc000_8000
    );
    assert_eq!(
        direct_map_physical(VirtualAddress::new(0xc000_8000))
            .unwrap()
            .get(),
        MemRegion::Ram.base().get() + 0x8000
    );
}

#[test]
fn mmio_decoding_enforces_width_direction_and_reserved_bits() {
    let priority = MmioTransaction::write(
        PhysicalAddress::new(0x1000_0010),
        MmioWidth::U32,
        interrupt::DEFAULT_PRIORITY_SYSTICK as u32,
    );
    assert_eq!(
        decode_mmio(priority),
        Ok(MmioRegister::Interrupt(
            interrupt::Register::PrioritySysTick
        ))
    );
    assert!(matches!(
        decode_mmio(MmioTransaction::write(
            PhysicalAddress::new(0x1000_0010),
            MmioWidth::U32,
            0x100,
        )),
        Err(PlatformError::InvalidMmioValue { .. })
    ));
    assert!(matches!(
        decode_mmio(MmioTransaction::read(
            PhysicalAddress::new(0x1000_4004),
            MmioWidth::U32,
        )),
        Err(PlatformError::InvalidMmioDirection)
    ));
    assert!(matches!(
        decode_mmio(MmioTransaction::read(
            PhysicalAddress::new(0x1000_5000),
            MmioWidth::U8,
        )),
        Err(PlatformError::InvalidMmioWidth(1))
    ));
}

#[test]
fn peripheral_constants_are_stable() {
    assert_eq!(interrupt::Source::SysTick.bit(), 1);
    assert_eq!(interrupt::Source::Uart0.bit(), 2);
    assert_eq!(interrupt::Source::Uart1.bit(), 4);
    assert_eq!(interrupt::Source::Block.bit(), 8);
    assert_eq!(block::STATUS_BUSY, 1);
    assert_eq!(block::Error::DeferredPersistence as u32, 6);
    assert_eq!(rng::DEFAULT_SEED, 0x4d45_4d55);
}

#[test]
fn page_entries_validate_permissions_and_access_metadata() {
    assert!(matches!(
        PageDirectoryEntry::new(0x4000_0002),
        Err(PlatformError::InvalidPageDirectoryBits(_))
    ));
    let directory = PageDirectoryEntry::new(MemRegion::Ram.base().get() | PTE_VALID).unwrap();
    assert_eq!(
        directory.table_address().unwrap(),
        Some(MemRegion::Ram.base())
    );
    assert_eq!(
        PageDirectoryEntry::new(MemRegion::SystemRom.base().get() | PTE_VALID)
            .unwrap()
            .table_address(),
        Err(PlatformError::AddressOutsideRam(
            MemRegion::SystemRom.base().get()
        ))
    );

    let entry = PageTableEntry::new(
        (MemRegion::Ram.base().get() + PAGE_SIZE)
            | PTE_VALID
            | PTE_READABLE
            | PTE_WRITABLE
            | PTE_EXECUTABLE
            | PTE_USER
            | (0b10101 << 7),
    );
    assert_eq!(
        entry.target_address().unwrap(),
        Some(PhysicalAddress::new(
            MemRegion::Ram.base().get() + PAGE_SIZE
        ))
    );
    assert_eq!(entry.software_metadata(), 0b10101);
    assert_eq!(entry.authorize(Access::Fetch, true), Ok(()));
    assert_eq!(entry.authorize(Access::Read, true), Ok(()));
    let updated = entry.record_successful_access(Access::Write);
    assert_ne!(updated.raw() & PTE_ACCESSED, 0);
    assert_ne!(updated.raw() & PTE_DIRTY, 0);
    assert_eq!(updated.clear_access_metadata().software_metadata(), 0b10101);

    let device = PageTableEntry::new(0x1000_0000 | PTE_VALID | PTE_READABLE | PTE_USER);
    assert_eq!(
        device.target_address(),
        Err(PlatformError::UserDeviceMapping(0x1000_0000))
    );
}

#[test]
fn fault_statuses_round_trip_for_every_access_shape() {
    for cause in [
        FaultCause::Translation,
        FaultCause::ReadProtection,
        FaultCause::WriteProtection,
        FaultCause::ExecuteProtection,
        FaultCause::DeviceAccess,
    ] {
        for from_user in [false, true] {
            for access in [Access::Fetch, Access::Read, Access::Write] {
                let status = FaultStatus::new(cause, from_user, access);
                assert_eq!(FaultStatus::from_raw(status.raw()), Ok(status));
                assert_eq!(status.cause(), Some(cause));
                assert_eq!(status.from_user(), from_user);
                assert_eq!(status.is_fetch(), access == Access::Fetch);
                assert_eq!(status.is_write(), access == Access::Write);
            }
        }
    }
    assert!(matches!(
        FaultStatus::from_raw(0x8000_0001),
        Err(PlatformError::InvalidFaultStatus(_))
    ));
}

#[test]
fn cp15_validation_uses_the_abi_rules() {
    assert_eq!(
        Cp15Operation::set_ttbr0(MemRegion::Ram.base().get()).unwrap(),
        Cp15Operation::SetTtbr0(MemRegion::Ram.base())
    );
    assert!(matches!(
        Cp15Operation::set_ttbr0(MemRegion::Ram.base().get() + 1),
        Err(PlatformError::UnalignedPage(_))
    ));
    assert_eq!(
        Cp15Operation::set_mmu_enabled(3),
        Cp15Operation::SetMmuEnabled(true)
    );
    assert!(matches!(
        Cp15Operation::set_vector_base(0xc000_8001),
        Err(PlatformError::UnalignedVectorBase(_))
    ));
}

#[test]
fn image_records_are_explicit_little_endian_codecs() {
    let header = ImageHeader {
        image_size: 256,
        kernel_segment_table_offset: IMAGE_HEADER_SIZE as u32,
        kernel_segment_count: 1,
        module_table_offset: 96,
        module_count: 1,
        bootstrap_entry_paddr: PhysicalAddress::new(MemRegion::Ram.base().get() + 0x8000),
        kernel_entry_vaddr: VirtualAddress::new(0xc000_8000),
        boot_info_paddr: PhysicalAddress::new(MemRegion::Ram.base().get() + 0x7000),
    };
    let bytes = header.encode().unwrap();
    assert_eq!(ImageHeader::decode(&bytes), Ok(header));

    let segment = KernelSegment {
        data_offset: 128,
        physical_address: PhysicalAddress::new(MemRegion::Ram.base().get() + 0x8000),
        virtual_address: VirtualAddress::new(0xc000_8000),
        file_size: 16,
        memory_size: 32,
        flags: 0b101,
    };
    assert_eq!(
        KernelSegment::decode(&segment.encode().unwrap()),
        Ok(segment)
    );

    let boot_info = BootInfo {
        system_rom_base: MemRegion::SystemRom.base(),
        image_size: 256,
        module_table_offset: 96,
        module_count: 1,
    };
    assert_eq!(
        BootInfo::decode(&boot_info.encode().unwrap()),
        Ok(boot_info)
    );
}
