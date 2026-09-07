# Boot ABI Version 1

> **Status: Normative.** This document defines reset firmware, kernel loading,
> bootstrap handoff, and boot-info behavior for platform ABI version 1.

This specification uses the address and range terminology from
[Platform ABI v1](abi-v1.md#terminology) and the serialized records from
[System-image format v1](system-image-v1.md).

## Fixed Locations

| Name | PA range or address | Corresponding VA |
|---|---|---|
| Reset vector | `0x0000_0000` | Translation disabled |
| Firmware stack workspace | `[0x4000_6000, 0x4000_7000)` | None required |
| Boot-info record | `[0x4000_7000, 0x4000_7040)` | `0xc000_7000` after direct-map installation |
| Physical bootstrap entry | `0x4000_8000` | Implementation-defined until bootstrap establishes mappings |
| RAM direct map | PA `[0x4000_0000, 0x4400_0000)` | VA `[0xc000_0000, 0xc400_0000)` |

**MINEMU-BOOT1-ADDR-001:** Reset firmware MUST begin at PA `0x0000_0000` and
MAY use `[0x4000_6000, 0x4000_7000)` as a descending temporary stack with
initial `SP = 0x4000_7000`.

**MINEMU-BOOT1-ADDR-002:** Kernel physical load ranges MUST NOT intersect the
reserved boot workspace `[0x4000_6000, 0x4000_7040)`.

**MINEMU-BOOT1-ADDR-003:** The image header MUST name PA `0x4000_8000` as its
bootstrap entry and PA `0x4000_7000` as its boot-info destination.

The fixed bootstrap entry is not the lowest PA at which kernel data may be
loaded. Other kernel segments may occupy any valid, non-overlapping RAM range
that satisfies the image specification.

## Host Preconditions

**MINEMU-BOOT1-HOST-001:** Before execution, the host MUST validate the complete
system image against [System-image format v1](system-image-v1.md) and place it at
System ROM offset `0`, corresponding to PA `0x0800_0000`.

**MINEMU-BOOT1-HOST-002:** The CLI boot interface MUST require exactly
`0x0001_0000` bytes of Boot ROM input. The image MUST fit in the 16-MiB System
ROM, and bytes after `image_size` in that ROM MUST be zero.

**MINEMU-BOOT1-HOST-003:** Version-1 reset firmware MAY trust host validation.
It is not required to revalidate image magic, bounds, flags, overlap, or
reserved fields. Version 1 defines no firmware-time digest or signature.

## Reset Firmware Sequence

**MINEMU-BOOT1-LOAD-001:** Reset firmware MUST perform this sequence in order:

1. Read the validated image header and kernel-segment table from System ROM.
2. Visit every kernel-segment record in table order.
3. For each segment, copy exactly `file_size` bytes from System ROM offset
   `data_offset` to PA `physical_address`.
4. Zero `[physical_address + file_size, physical_address + memory_size)`.
5. Write the complete boot-info record at PA `0x4000_7000`.
6. Keep address translation disabled.
7. Set `r0 = 0xc000_7000` and branch in A32 state to PA `0x4000_8000`.

**MINEMU-BOOT1-LOAD-002:** Segment initialized and BSS extents MUST be interpreted
as half-open PA ranges. Firmware MUST neither omit nor write beyond those
extents.

## Bootstrap Handoff

At the instruction entered at PA `0x4000_8000`, machine state is:

| State | Required value |
|---|---|
| `PC` | `0x4000_8000` |
| `r0` | `0xc000_7000` |
| Instruction state | A32 |
| CPU mode | SVC |
| Address translation | Disabled |
| IRQ delivery | Masked |
| Kernel segment memory | Initialized bytes copied and BSS bytes zero |
| Boot info | Complete at PA `0x4000_7000` |
| Other general-purpose registers | Unspecified |

**MINEMU-BOOT1-HANDOFF-001:** `r0 = 0xc000_7000` is a future VA. Bootstrap code
MUST NOT dereference it before installing the RAM direct map.

**MINEMU-BOOT1-HANDOFF-002:** Bootstrap code MUST install a valid TTBR0, execute
TLBIALL, enable `SCTLR.M`, and establish
`VA [0xc000_0000, 0xc400_0000) -> PA [0x4000_0000, 0x4400_0000)` before using
the boot-info VA.

**MINEMU-BOOT1-HANDOFF-003:** Before enabling exception delivery, bootstrap code
MUST establish an executable vector mapping and set VBAR to its VA.

**MINEMU-BOOT1-HANDOFF-004:** Bootstrap code MUST eventually transfer control to
the four-byte-aligned `kernel_entry_vaddr` recorded by the validated image. How
the bootstrap implementation retains or obtains that value is not prescribed.

Temporary identity mappings, page-table placement, initial MMIO mappings, and
banked-stack placement are reference-template policy described in
[Template memory layout](../student/template-memory-layout.md), not platform ABI.

## Boot-Info Record

The boot-info record is exactly 64 bytes at PA `[0x4000_7000, 0x4000_7040)`.
All integers are unsigned and little-endian.

| Offset | Type | Field | Version-1 value or meaning |
|---:|---|---|---|
| `0x00` | `u32` | Magic | `0x4d42_4f4f`; bytes `4f 4f 42 4d` |
| `0x04` | `u16` | ABI version | `1` |
| `0x06` | `u16` | Record size | `64` (`0x0040`) |
| `0x08` | `u32` | System ROM PA | `0x0800_0000` |
| `0x0c` | `u32` | Image size | Validated image byte length |
| `0x10` | `u32` | Module-table ROM offset | Copied from image header |
| `0x14` | `u32` | Module count | Copied from image header |
| `0x18` | `u32` | Direct-map VA base | `0xc000_0000` |
| `0x1c` | `u32` | Direct-map PA base | `0x4000_0000` |
| `0x20` | `u32` | Direct-map size | `0x0400_0000` |
| `0x24` | `u32` | Flags | `0` |
| `0x28` | 24 bytes | Reserved | All zero |

**MINEMU-BOOT1-INFO-001:** Firmware MUST write the exact boot-info layout above.
The numeric magic is authoritative; the displayed bytes are its exact wire
representation.

**MINEMU-BOOT1-INFO-002:** `image_size`, `module_table_offset`, and `module_count`
MUST describe the same validated image mapped at System ROM PA `0x0800_0000`.

**MINEMU-BOOT1-INFO-003:** After the required direct map is active, the record
MUST be accessible at VA `0xc000_7000` without changing its physical storage.
