# Emulator ABI

## Status And Scope

This document is the normative ABI for the `minemu` A32 platform. Rust
implementation details, Unicorn behavior, and host operating-system behavior
are not part of the guest ABI unless specified here.

The platform is a deterministic, single-core ARMv7-A machine for operating
systems coursework. It is not a model of a production SoC.

## CPU

- One little-endian Cortex-A9 core.
- A32 instructions only. Thumb is unsupported.
- Supported modes are USR, SVC, IRQ, ABT, and UND.
- Banked SP/LR registers and SPSR are available for supported exception modes.
- VFP, NEON, FIQ, TrustZone, virtualization extensions, caches, DMA coherency,
  and SMP are out of scope.
- Student code is freestanding A32 C and assembly, built with
  `arm-none-eabi-gcc`, `-mcpu=cortex-a9`, `-marm`, and `-mfloat-abi=soft`.
- `libgcc` is an allowed static link dependency. Newlib and newlib-nano are not
  part of the supported platform.

## Physical Memory Map

| Physical range | Size | Purpose |
|---|---:|---|
| `0x0000_0000..0x0000_ffff` | 64 KiB | Immutable platform boot ROM |
| `0x0800_0000..0x08ff_ffff` | 16 MiB | Immutable system ROM |
| `0x1000_0000..0x1000_0fff` | 4 KiB | UART |
| `0x1000_1000..0x1000_1fff` | 4 KiB | Virtual timer |
| `0x1000_2000..0x1000_2fff` | 4 KiB | Interrupt controller |
| `0x1000_3000..0x1000_3fff` | 4 KiB | DMA block device |
| `0x1000_5000..0x1000_5fff` | 4 KiB | Deterministic RNG |
| `0x1000_f000..0x1000_ffff` | 4 KiB | Optional trace device |
| `0x4000_0000..0x43ff_ffff` | 64 MiB | Writable RAM |

`0x1000_4000..0x1000_4fff` is reserved. MMU control is CP15-only and has no
MMIO control page.

Unmapped physical accesses, accesses to reserved device pages, and invalid
MMIO transactions enter the data-abort path with fault cause
`DEVICE_ACCESS`.

## Bootstrap And Higher-Half Kernel

The reset and bootstrap contract uses these fixed physical addresses:

| Address | Meaning |
|---|---|
| `0x4000_7000` | Boot-info physical address |
| `0x4000_8000` | First permitted kernel bootstrap physical address |
| `0xc000_0000` | Kernel higher-half direct-map base |
| `0x8000_0000` | Kernel direct-map virtual-to-physical offset |

The initial kernel mapping is:

```text
VA 0xc000_0000..0xc3ff_ffff -> PA 0x4000_0000..0x43ff_ffff
```

The host packer requires `bootstrap_entry_paddr = 0x4000_8000`. The boot ROM
copies kernel segments to their declared physical addresses and starts that
entry with translation disabled. The bootstrap code
creates the initial page tables, installs TTBR0, enables SCTLR.M, sets VBAR to
the kernel vector base, and branches to `kernel_entry_vaddr` in the high-half
mapping. It passes `r0 = 0xc000_7000`, the high-half alias of boot info.

The bootstrap segment is physical code. Kernel high-half segments must satisfy
`physical_address = virtual_address - 0x8000_0000`. The boot ROM does not
validate image contents in ABI version 1; the host image packer validates all
inputs before constructing a ROM image.

## System-ROM Image Format

All image fields are little-endian and fixed-width. The packer writes ABI
version 1. The boot ROM trusts the header and tables in version 1; parser and
packer validation remains mandatory on the host.

### Image Header

The image header is 64 bytes at system-ROM offset zero.

| Offset | Type | Name |
|---:|---|---|
| `0x00` | `u32` | Magic, `0x4d45_4d55` (`MEMU`) |
| `0x04` | `u16` | Format version, `1` |
| `0x06` | `u16` | Header size, `64` |
| `0x08` | `u32` | Total image size in bytes |
| `0x0c` | `u32` | Kernel-segment table offset |
| `0x10` | `u32` | Kernel-segment count |
| `0x14` | `u32` | Module table offset |
| `0x18` | `u32` | Module count |
| `0x1c` | `u32` | Bootstrap entry physical address |
| `0x20` | `u32` | Kernel entry virtual address |
| `0x24` | `u32` | Boot-info physical address, `0x4000_7000` |
| `0x28` | `u32` | Flags, zero in version 1 |
| `0x2c..0x3f` | `u32[5]` | Reserved, zero |

### Kernel Segment Record

Each kernel-segment record is 32 bytes.

| Offset | Type | Name |
|---:|---|---|
| `0x00` | `u32` | Initialized-byte offset in system ROM |
| `0x04` | `u32` | Physical load address |
| `0x08` | `u32` | Virtual address |
| `0x0c` | `u32` | Initialized-byte size |
| `0x10` | `u32` | Memory size after BSS zeroing |
| `0x14` | `u32` | Flags: bit 0 readable, bit 1 writable, bit 2 executable |
| `0x18..0x1f` | `u32[2]` | Reserved, zero |

### Module Record

Each module record is 32 bytes. A module segment record is also 32 bytes, with
no physical load address. The kernel chooses physical frames when it creates a
process.

| Offset | Type | Name |
|---:|---|---|
| `0x00` | `u32` | UTF-8 module-name offset in system ROM |
| `0x04` | `u32` | Module-name byte length |
| `0x08` | `u32` | Module-segment table offset |
| `0x0c` | `u32` | Module-segment count |
| `0x10` | `u32` | Fixed user virtual entry address |
| `0x14` | `u32` | Module flags, zero in version 1 |
| `0x18..0x1f` | `u32[2]` | Reserved, zero |

| Offset | Type | Module-segment field |
|---:|---|---|
| `0x00` | `u32` | Initialized-byte offset in system ROM |
| `0x04` | `u32` | Fixed user virtual address |
| `0x08` | `u32` | Initialized-byte size |
| `0x0c` | `u32` | Memory size after BSS zeroing |
| `0x10` | `u32` | Flags: bit 0 readable, bit 1 writable, bit 2 executable |
| `0x14..0x1f` | `u32[3]` | Reserved, zero |

The host packer rejects overlapping records, out-of-ROM offsets, unsupported
ELF relocation models, duplicate names, Thumb entries, and nonzero reserved
fields. There is no image digest or boot-time integrity verification in version
1.

### Boot Info

Boot info is a 64-byte record written at physical `0x4000_7000` and exposed to
the higher-half kernel at `0xc000_7000`.

| Offset | Type | Name |
|---:|---|---|
| `0x00` | `u32` | Magic, `0x4d42_4f4f` (`MBOO`) |
| `0x04` | `u16` | ABI version, `1` |
| `0x06` | `u16` | Record size, `64` |
| `0x08` | `u32` | System-ROM physical base |
| `0x0c` | `u32` | System-ROM image size |
| `0x10` | `u32` | Module-table system-ROM offset |
| `0x14` | `u32` | Module count |
| `0x18` | `u32` | Kernel direct-map virtual base, `0xc000_0000` |
| `0x1c` | `u32` | Kernel direct-map physical base, `0x4000_0000` |
| `0x20` | `u32` | Kernel direct-map size, `0x0400_0000` |
| `0x24` | `u32` | Flags, zero in version 1 |
| `0x28..0x3f` | `u32[6]` | Reserved, zero |

## Exceptions

The vectors are at `VBAR + offset`.

| Offset | Exception | Mode | Banked LR on entry | Standard return |
|---:|---|---|---|---|
| `0x04` | Undefined instruction | UND | Faulting PC + 4 | `movs pc, lr` |
| `0x08` | SVC | SVC | SVC PC + 4 | `movs pc, lr` |
| `0x0c` | Prefetch abort | ABT | Faulting PC + 4 | `subs pc, lr, #4` |
| `0x10` | Data abort | ABT | Faulting PC + 8 | `subs pc, lr, #8` |
| `0x18` | IRQ | IRQ | Interrupted PC + 4 | `subs pc, lr, #4` |

On every supported exception entry, the platform copies the prior CPSR into
the destination mode's SPSR, enters the listed mode, clears the A32 Thumb bit,
sets the IRQ mask bit, writes the listed banked LR value, and branches to the
vector. The vector table and every exception handler must remain mapped as
supervisor-readable and executable while the MMU is enabled.

Undefined instructions include unsupported or unprivileged CP15 operations.
Prefetch aborts represent failed instruction fetches. Data aborts represent
failed data or MMIO accesses.

## CP15 Interface

CP15 is the sole MMU and fault-control interface. The following A32 operations
are supported only in privileged modes: SVC, IRQ, ABT, and UND.

| Instruction | Meaning |
|---|---|
| `MCR p15, 0, Rt, c2, c0, 0` | Set TTBR0 from `Rt` |
| `MCR p15, 0, Rt, c1, c0, 0` | Set SCTLR; bit 0 controls MMU enable |
| `MCR p15, 0, Rt, c8, c7, 0` | Invalidate all MMU translations; `Rt` ignored |
| `MCR p15, 0, Rt, c12, c0, 0` | Set VBAR from `Rt` |
| `MRC p15, 0, Rt, c5, c0, 0` | Read most recent fault status into `Rt` |
| `MRC p15, 0, Rt, c6, c0, 0` | Read most recent fault address into `Rt` |

The instruction condition is evaluated before CP15 access. A condition-false
instruction has no effect. A CP15 operation from USR mode, an unsupported CP15
operation, an unaligned VBAR, or an invalid TTBR0 produces an undefined
instruction exception.

TTBR0 must be a 4 KiB-aligned physical RAM address. SCTLR bit 0 is the only
defined writable bit in version 1; all other bits are ignored. VBAR must be
32-byte aligned. The platform applies a CP15 state change before executing the
following guest instruction. An explicit DSB or ISB is not required by this
ABI.

DFSR and DFAR reset to zero. They report the most recent translation,
protection, fetch, or invalid-MMIO fault until another fault replaces them.

## MMU

The MMU is a custom two-level 4 KiB paging model. Unicorn's native ARM VMSA
page-table format is not part of the ABI.

| Item | Definition |
|---|---|
| Virtual and physical addresses | 32-bit |
| Page size | 4 KiB |
| Directory | 1,024 entries indexed by VA bits `31:22` |
| Page table | 1,024 entries indexed by VA bits `21:12` |
| Root | TTBR0 physical RAM page |
| Invalidation | `MCR ... c8, c7, 0` TLBIALL |

Translation is disabled after reset. In that state, virtual addresses are
physical addresses. When SCTLR.M is enabled, every instruction fetch, data
read, and data write uses the current TTBR0 page directory.

### Directory And Page-Table Entries

Directory and page-table entries are little-endian `u32` values.

| Entry | Bit | Meaning |
|---|---:|---|
| PDE | 0 | Valid; bits `31:12` are physical page-table base |
| PTE | 0 | Valid |
| PTE | 1 | Writable |
| PTE | 2 | User accessible |
| PTE | 3 | Executable |
| PTE | 4 | Readable |
| PTE | `31:12` | Physical target page base |

All unspecified bits are reserved and must be zero. A valid PDE target must be
a 4 KiB page wholly inside physical RAM. A valid PTE target may be RAM, boot
ROM, system ROM, or an implemented MMIO page. Device pages are always
supervisor-only, regardless of PTE user bit.

A valid PTE grants no implied permissions. Reads require readable, writes
require writable, and instruction fetches require executable. User-mode access
also requires user. Supervisor code may access valid user pages. ROM pages are
never writable.

### Fault Status

The low byte identifies cause. Bits 8 through 10 describe the attempted
access.

| Value or bit | Meaning |
|---:|---|
| `1` | Translation fault |
| `2` | Read-protection fault |
| `3` | Write-protection fault |
| `4` | Execute-protection fault |
| `5` | Invalid or unmapped MMIO access |
| Bit 8 | Access originated in USR mode |
| Bit 9 | Access was a write |
| Bit 10 | Access was an instruction fetch |

## Virtual Time

Virtual time is a deterministic logical instruction-progress clock, not a
hardware-cycle model.

- A completed instruction advances time by one tick.
- SVC and undefined instructions advance time by one tick, then exception
  entry advances time by one tick.
- A data or prefetch fault does not advance time for the faulting instruction;
  exception entry advances time by one tick.
- Device commands consume their normal instruction tick. Successful and failed
  operations complete at the same configured deadline.
- Host or backend failures add no guest time after execution stops.
- Timer and device deadlines are processed after time advances and before the
  next guest instruction begins.
- Pending IRQs are delivered only at instruction boundaries when CPSR.I is
  clear. Exceptions and faults take priority over later IRQ delivery at the
  same boundary.

## MMIO Rules

All device registers are little-endian, 32-bit values at four-byte-aligned
offsets. Reads and writes must be aligned 32-bit transactions. An access with
another width, an unaligned access, a read from a write-only register, a write
to a read-only register, a reserved-bit write, or an undefined register offset
causes a `DEVICE_ACCESS` data abort.

All implemented MMIO pages are supervisor-only. A user PTE that targets a
device page causes a protection fault before the device is accessed.

### UART (`0x1000_0000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `RX_DATA` | R | Low byte is next queued input byte; read consumes it, or returns zero when empty |
| `0x04` | `TX_DATA` | W | Low byte is appended to console output |
| `0x08` | `STATUS` | R | Bit 0 RX ready, bit 1 TX ready |
| `0x0c` | `CONTROL` | RW | Bit 0 enables UART RX IRQ |

TX ready is always set. The UART source is level-pending while RX is nonempty
and RX IRQ is enabled. It clears when input is consumed, the queue becomes
empty, or RX IRQ is disabled. The platform exposes registers and raw MMIO
helpers only; students implement UART drivers.

### Timer (`0x1000_1000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `PERIOD` | RW | Positive virtual-tick interval |
| `0x04` | `CONTROL` | RW | Bit 0 enable, bit 1 periodic, bit 2 IRQ enable |
| `0x08` | `STATUS` | R | Bit 0 event pending |
| `0x0c` | `ACK` | W | Bit 0 clears pending event |

Writing zero to `PERIOD` is an invalid device access. A timer starts its first
interval when enabled. Periodic expirations advance by exact multiples of the
configured period, even when a batch crosses more than one deadline. ACK clears
the current pending state; it does not disable a periodic timer.

### Interrupt Controller (`0x1000_2000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `PENDING` | R | Pending source bitmap |
| `0x04` | `ENABLE` | RW | Enabled source bitmap |
| `0x08` | `CLAIM` | R | Current claim or highest-priority active source |
| `0x0c` | `EOI` | W | Completes the claimed source index |

Source 0 is UART RX, source 1 is timer, and source 2 is block completion.
Lower source indices have higher priority. A source is active when pending and
enabled. Reading CLAIM selects and retains the highest-priority active source;
it returns `0xffff_ffff` when none is active. A further CLAIM read returns the
retained claim until a matching EOI. Device ACK clears the underlying source;
EOI releases the controller claim. An EOI value that does not match the active
claim is an invalid device access.

### Block Device (`0x1000_3000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `COMMAND` | W | `1` read, `2` write |
| `0x04` | `LBA` | RW | First 512-byte sector |
| `0x08` | `SECTOR_COUNT` | RW | Nonzero requested sector count |
| `0x0c` | `DMA_PADDR` | RW | Physical RAM buffer address |
| `0x10` | `STATUS` | R | Bit 0 busy, bit 1 complete, bit 2 error |
| `0x14` | `ERROR` | R | Error code |
| `0x18` | `ACK` | W | Bit 0 clears complete and error state |
| `0x1c` | `CONTROL` | RW | Bit 0 enables completion IRQ |

Every command accepted while idle, including one that will complete with a
guest-visible validation error, completes exactly 32 virtual ticks after the
command write. A command issued while busy is synchronously rejected with
`Busy` and does not disturb the active request. DMA addresses are physical,
512-byte aligned, and wholly inside RAM. Commands operate on the
emulator-owned memory copy of an attached nonempty, sector-aligned raw disk.
Writes mark affected sectors dirty and do not synchronously modify the host
file.

| Error | Value |
|---|---:|
| None | `0` |
| No media | `1` |
| Busy | `2` |
| Invalid command | `3` |
| Invalid DMA | `4` |
| Invalid LBA or range | `5` |
| Deferred persistence failure | `6` |

The runtime flushes dirty sectors on pause, shutdown, and terminal
emulator/backend failure. A failed flush leaves dirty sectors intact for retry.
Guest command errors never force a host flush.

### RNG (`0x1000_5000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `SEED` | RW | Configured seed; writes restart the sequence |
| `0x04` | `DATA` | R | Advance once and return next `u32` |
| `0x08` | `STATE` | R | Current generator state |

The RNG is deterministic. Reset initializes SEED and STATE to
`0x4d45_4d55`. Writing zero to SEED stores that default value instead. DATA
uses `xorshift32`:

```text
x ^= x << 13
x ^= x >> 17
x ^= x << 5
state = x
return x
```

RNG reads have no IRQ, DMA, or extra virtual-time cost beyond the instruction
that performs the read.

### Trace Device (`0x1000_f000`)

The trace page is reserved for a future optional course-defined event device.
Until specified, every access is an invalid MMIO transaction.

## Observability And Testing

The runtime records bounded UART, timer, IRQ, block, exception, MMU, and
device events. It publishes lightweight immutable status at a cadence or after
a material state change. CPU-register, MMU-walk, memory, and detailed device
inspection are requested explicitly and are not continuously copied into every
status update.

`minemu test` runs a completed system-ROM image with scheduled input and
assertions over console output, events, faults, and selected machine state. It
uses the same machine and runtime contract as interactive execution.
