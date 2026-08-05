use super::{ImageBuilder, ImageError, ModuleInput, SystemImage};

#[test]
fn packer_is_deterministic_and_boot_plan_covers_bss() {
    let kernel = elf(
        0xc000_9000,
        &[
            (0x4000_8000, &[1, 2, 3, 4], 8),
            (0xc000_9000, &[5, 6, 7, 8], 4),
        ],
    );
    let module = elf(0x0040_0000, &[(0x0040_0000, &[9, 10, 11, 12], 4)]);
    let image = ImageBuilder::new(&kernel)
        .add_module(ModuleInput {
            name: "shell",
            elf: &module,
        })
        .build()
        .unwrap();
    let repeated = ImageBuilder::new(&kernel)
        .add_module(ModuleInput {
            name: "shell",
            elf: &module,
        })
        .build()
        .unwrap();
    assert_eq!(image.bytes(), repeated.bytes());
    assert_eq!(SystemImage::parse(image.bytes()).unwrap(), image);
    let plan = image.boot_plan().unwrap();
    assert_eq!(plan.copies.len(), 2);
    assert_eq!(plan.copies[0].zeroed_length, 4);
    assert_eq!(plan.boot_info_vaddr.get(), 0xc000_7000);
    assert_eq!(plan.kernel_entry_vaddr.get(), 0xc000_9000);
    let mut writes = Vec::new();
    plan.apply(|address, bytes| {
        writes.push((address.get(), bytes.to_vec()));
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(writes[0], (0x4000_8000, vec![1, 2, 3, 4]));
    assert_eq!(writes[1], (0x4000_8004, vec![0; 4]));
    assert_eq!(writes.last().unwrap().0, 0x4000_7000);
}

#[test]
fn parser_rejects_truncated_version_mismatched_and_corrupt_images() {
    let kernel = kernel();
    let image = ImageBuilder::new(&kernel).build().unwrap();
    assert!(SystemImage::parse(&image.bytes()[..63]).is_err());
    let mut version = image.bytes().to_vec();
    version[4] = 2;
    assert!(matches!(
        SystemImage::parse(&version),
        Err(ImageError::Platform(_))
    ));

    let module = elf(0x0040_0000, &[(0x0040_0000, &[9, 10, 11, 12], 4)]);
    let image = ImageBuilder::new(&kernel)
        .add_module(ModuleInput {
            name: "shell",
            elf: &module,
        })
        .build()
        .unwrap();
    let mut corrupt_table = image.bytes().to_vec();
    let module_offset = image.header().module_table_offset as usize;
    corrupt_table[module_offset + 8..module_offset + 12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(SystemImage::parse(&corrupt_table).is_err());
}

#[test]
fn packer_rejects_invalid_a32_entries_bootstrap_and_boot_info_overlap() {
    let kernel = kernel();
    let unaligned = elf(0x0040_0002, &[(0x0040_0000, &[1, 2, 3, 4], 4)]);
    assert!(
        ImageBuilder::new(&kernel)
            .add_module(ModuleInput {
                name: "unaligned",
                elf: &unaligned,
            })
            .build()
            .is_err()
    );

    let bss_bootstrap = elf(
        0xc000_9000,
        &[(0x4000_8000, &[], 4), (0xc000_9000, &[5, 6, 7, 8], 4)],
    );
    assert!(ImageBuilder::new(&bss_bootstrap).build().is_err());

    let boot_info_overlap = elf(
        0xc000_9000,
        &[
            (0x4000_7000, &[1, 2, 3, 4], 4),
            (0x4000_8000, &[1, 2, 3, 4], 4),
            (0xc000_9000, &[5, 6, 7, 8], 4),
        ],
    );
    assert!(ImageBuilder::new(&boot_info_overlap).build().is_err());
}

#[test]
fn user_ranges_cannot_overlap_the_kernel_direct_map() {
    let kernel = kernel();
    let crossing = elf(0xbfff_f000, &[(0xbfff_f000, &[1, 2, 3, 4], 0x2000)]);
    assert!(
        ImageBuilder::new(&kernel)
            .add_module(ModuleInput {
                name: "crossing",
                elf: &crossing,
            })
            .build()
            .is_err()
    );

    let module = elf(0x0040_0000, &[(0x0040_0000, &[9, 10, 11, 12], 4)]);
    let image = ImageBuilder::new(&kernel)
        .add_module(ModuleInput {
            name: "shell",
            elf: &module,
        })
        .build()
        .unwrap();
    let mut direct_map_module = image.bytes().to_vec();
    let module_offset = image.header().module_table_offset as usize;
    let segment_table_offset = u32::from_le_bytes(
        direct_map_module[module_offset + 8..module_offset + 12]
            .try_into()
            .unwrap(),
    ) as usize;
    direct_map_module[module_offset + 16..module_offset + 20]
        .copy_from_slice(&0xc000_0000u32.to_le_bytes());
    direct_map_module[segment_table_offset + 4..segment_table_offset + 8]
        .copy_from_slice(&0xc000_0000u32.to_le_bytes());
    assert!(SystemImage::parse(&direct_map_module).is_err());
}

#[test]
fn packer_accepts_utf8_module_names() {
    let kernel = kernel();
    let module = elf(0x0040_0000, &[(0x0040_0000, &[9, 10, 11, 12], 4)]);
    let image = ImageBuilder::new(&kernel)
        .add_module(ModuleInput {
            name: "shel\u{00e9}",
            elf: &module,
        })
        .build()
        .unwrap();
    assert_eq!(image.modules()[0].name, "shel\u{00e9}");
}

fn kernel() -> Vec<u8> {
    elf(
        0xc000_9000,
        &[
            (0x4000_8000, &[1, 2, 3, 4], 4),
            (0xc000_9000, &[5, 6, 7, 8], 4),
        ],
    )
}

fn elf(entry: u32, segments: &[(u32, &[u8], u32)]) -> Vec<u8> {
    const ELF_HEADER_SIZE: usize = 52;
    const PROGRAM_HEADER_SIZE: usize = 32;
    let program_headers = segments.len() * PROGRAM_HEADER_SIZE;
    let mut bytes = vec![
        0;
        0x100
            + segments
                .iter()
                .map(|(_, data, _)| data.len())
                .sum::<usize>()
    ];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 1;
    bytes[5] = 1;
    bytes[6] = 1;
    write_u16(&mut bytes, 16, 2);
    write_u16(&mut bytes, 18, 40);
    write_u32(&mut bytes, 20, 1);
    write_u32(&mut bytes, 24, entry);
    write_u32(&mut bytes, 28, ELF_HEADER_SIZE as u32);
    write_u16(&mut bytes, 40, ELF_HEADER_SIZE as u16);
    write_u16(&mut bytes, 42, PROGRAM_HEADER_SIZE as u16);
    write_u16(&mut bytes, 44, segments.len() as u16);
    let mut data_offset = 0x100;
    for (index, (address, data, memory_size)) in segments.iter().enumerate() {
        let offset = ELF_HEADER_SIZE + index * PROGRAM_HEADER_SIZE;
        write_u32(&mut bytes, offset, 1);
        write_u32(&mut bytes, offset + 4, data_offset as u32);
        write_u32(&mut bytes, offset + 8, *address);
        write_u32(&mut bytes, offset + 12, *address);
        write_u32(&mut bytes, offset + 16, data.len() as u32);
        write_u32(&mut bytes, offset + 20, *memory_size);
        write_u32(&mut bytes, offset + 24, 5);
        write_u32(&mut bytes, offset + 28, 4);
        bytes[data_offset..data_offset + data.len()].copy_from_slice(data);
        data_offset += data.len();
    }
    bytes.truncate(data_offset.max(ELF_HEADER_SIZE + program_headers));
    bytes
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
