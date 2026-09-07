# System-Image Format Version 1

> **Status: Normative.** This document defines the exact version-1 System ROM
> image wire format and validity rules.

## Encoding

An image is a byte sequence loaded at System ROM offset `0`, corresponding to PA
`0x0800_0000`. Record offsets are ROM offsets from the beginning of that byte
sequence. They are not physical addresses, virtual addresses, or host-file
offsets, even when the host file contains exactly the image bytes.

**MINEMU-IMG1-ENC-001:** Every `u16` and `u32` field MUST be serialized
little-endian at its listed offset. Records MUST be encoded field by field and
MUST NOT depend on a compiler's structure layout.

**MINEMU-IMG1-ENC-002:** The numeric image magic is `0x4d45_4d55`; its exact wire
bytes at ROM offsets `[0, 4)` are `55 4d 45 4d`.

**MINEMU-IMG1-ENC-003:** Every field marked reserved and every alignment byte
inserted by a canonical producer MUST be zero.

## Image Header

The image header occupies ROM offsets `[0x00, 0x40)`.

| Offset | Type | Field | Value or meaning |
|---:|---|---|---|
| `0x00` | `u32` | Magic | `0x4d45_4d55`; bytes `55 4d 45 4d` |
| `0x04` | `u16` | ABI version | `1` |
| `0x06` | `u16` | Header size | `64` (`0x0040`) |
| `0x08` | `u32` | Image size | Exact byte length |
| `0x0c` | `u32` | Kernel-segment-table ROM offset | Start of 32-byte records |
| `0x10` | `u32` | Kernel segment count | Number of records |
| `0x14` | `u32` | Module-table ROM offset | Start of 32-byte records |
| `0x18` | `u32` | Module count | Number of records |
| `0x1c` | `u32` | Bootstrap entry PA | `0x4000_8000` |
| `0x20` | `u32` | Kernel entry VA | Direct-map VA containing initialized executable code |
| `0x24` | `u32` | Boot-info PA | `0x4000_7000` |
| `0x28` | `u32` | Flags | `0` |
| `0x2c` | 20 bytes | Reserved | All zero |

**MINEMU-IMG1-HDR-001:** A valid image MUST contain the exact header layout and
fixed values above. `image_size` MUST be in `[64, 0x0100_0000]` and MUST equal
the complete input byte length presented to validation.

**MINEMU-IMG1-HDR-002:** Both top-level table ranges, computed as
`[offset, offset + count * 32)`, MUST fit within `[0, image_size)` without
arithmetic overflow.

**MINEMU-IMG1-HDR-003:** `kernel_entry_vaddr` MUST be four-byte aligned, lie in
VA `[0xc000_0000, 0xc400_0000)`, and identify a complete four-byte instruction
inside the initialized `file_size` extent of an executable kernel segment.

## Kernel-Segment Record

Each kernel-segment record is exactly 32 bytes.

| Offset | Type | Field | Meaning |
|---:|---|---|---|
| `0x00` | `u32` | Data ROM offset | Start of initialized bytes |
| `0x04` | `u32` | Physical address | RAM load destination |
| `0x08` | `u32` | Virtual address | Linked VA |
| `0x0c` | `u32` | File size | Initialized byte count |
| `0x10` | `u32` | Memory size | Complete in-memory byte count |
| `0x14` | `u32` | Flags | Bit 0 readable, bit 1 writable, bit 2 executable |
| `0x18` | 8 bytes | Reserved | All zero |

**MINEMU-IMG1-KSEG-001:** For every kernel segment, `memory_size` MUST be
nonzero, `file_size` MUST be no greater than `memory_size`, and no flag outside
mask `0x0000_0007` may be set.

**MINEMU-IMG1-KSEG-002:** The half-open PA load range
`[physical_address, physical_address + memory_size)` MUST fit completely in RAM
`[0x4000_0000, 0x4400_0000)` and MUST NOT intersect boot workspace
`[0x4000_6000, 0x4000_7040)`.

**MINEMU-IMG1-KSEG-003:** `virtual_address` MUST either equal
`physical_address` or equal `physical_address + 0x8000_0000` in the required RAM
direct map. Each segment's VA and PA ranges MUST be representable without
32-bit-address-space wraparound.

**MINEMU-IMG1-KSEG-004:** Distinct kernel segments MUST have non-overlapping
half-open PA memory ranges and non-overlapping half-open VA memory ranges.

**MINEMU-IMG1-KSEG-005:** At least one executable kernel segment's initialized
PA extent MUST contain the complete A32 instruction at PA `0x4000_8000`.

## Module Record

Each module record is exactly 32 bytes.

| Offset | Type | Field | Meaning |
|---:|---|---|---|
| `0x00` | `u32` | Name ROM offset | Start of UTF-8 bytes, no terminator |
| `0x04` | `u32` | Name length | Name byte count |
| `0x08` | `u32` | Segment-table ROM offset | Start of this module's 32-byte records |
| `0x0c` | `u32` | Segment count | Number of module-segment records |
| `0x10` | `u32` | Entry VA | Initial module entry point |
| `0x14` | `u32` | Flags | `0` |
| `0x18` | 8 bytes | Reserved | All zero |

**MINEMU-IMG1-MOD-001:** A module name MUST be non-empty, valid UTF-8, and
unique by exact byte sequence within the image. It is length-delimited and MUST
NOT include an implicit terminator.

**MINEMU-IMG1-MOD-002:** A module segment-table range, computed as
`[segment_table_offset, segment_table_offset + segment_count * 32)`, MUST fit in
the image without arithmetic overflow.

**MINEMU-IMG1-MOD-003:** `entry_virtual_address` MUST be four-byte aligned and
identify a complete four-byte instruction inside the initialized `file_size`
extent of one executable segment belonging to that module.

Module records describe fixed virtual layouts but no physical placement.
Allocating frames and constructing user address spaces is kernel policy; see
[Module format and loading](../student/module-format-and-loading.md).

## Module-Segment Record

Each module-segment record is exactly 32 bytes.

| Offset | Type | Field | Meaning |
|---:|---|---|---|
| `0x00` | `u32` | Data ROM offset | Start of initialized bytes |
| `0x04` | `u32` | Virtual address | Fixed linked VA |
| `0x08` | `u32` | File size | Initialized byte count |
| `0x0c` | `u32` | Memory size | Complete in-memory byte count |
| `0x10` | `u32` | Flags | Bit 0 readable, bit 1 writable, bit 2 executable |
| `0x14` | 12 bytes | Reserved | All zero |

**MINEMU-IMG1-MSEG-001:** For every module segment, `memory_size` MUST be
nonzero, `file_size` MUST be no greater than `memory_size`, no flag outside mask
`0x0000_0007` may be set, and the VA memory range MUST not wrap the 32-bit
address space.

**MINEMU-IMG1-MSEG-002:** No module segment VA memory range may intersect the
kernel direct-map VA range `[0xc000_0000, 0xc400_0000)`. Distinct segments in
one module MUST have non-overlapping half-open VA memory ranges.

## Whole-Image Validity

For these rules, an occupied ROM-offset range is the header, each table, each
non-empty name, or each non-empty initialized segment payload. BSS bytes are not
stored and therefore occupy no image range.

**MINEMU-IMG1-VALID-001:** Every occupied ROM-offset range MUST fit within
`[0, image_size)` and all occupied ranges MUST be pairwise non-overlapping.
Empty ranges occupy no bytes.

**MINEMU-IMG1-VALID-002:** The kernel-segment table, module table, module-segment
tables, names, and payloads MAY appear at any offsets satisfying this
specification. A consumer MUST follow offsets rather than assume canonical
producer ordering.

**MINEMU-IMG1-VALID-003:** Bytes not included in an occupied range are padding
and have no semantic meaning to a consumer. A canonical producer MUST write
such bytes as zero.

## Canonical Production

The canonical `minemu-pack` producer accepts ELF32, little-endian ARM `ET_EXEC`
inputs with at least one `PT_LOAD` segment and A32-aligned entry point. It uses
only non-empty load segments, maps ELF `PF_R`, `PF_W`, and `PF_X` to image flag
bits 0, 1, and 2, sorts kernel and per-module segments by VA, sorts modules by
name bytes, and emits tables, names, and payloads in deterministic order.

**MINEMU-IMG1-PROD-001:** Conformance of an image consumer is determined by the
wire-format and validity requirements above, not by reproducing canonical
producer placement choices.
