# Emulator Specification

## Purpose

`minemu` emulates a deliberately small, deterministic ARM teaching machine. It
is not intended to reproduce a production SoC. The machine provides only the
CPU, memory, exceptions, paging, and peripherals needed for the operating
systems curriculum.

The emulator is implemented in Rust and uses Unicorn for A32 instruction
execution. The SoC, devices, virtual time, exception delivery, and teaching
MMU are implemented by `minemu`.

## CPU

The guest CPU is a single little-endian ARMv7-A Cortex-A9 core.

- A32 (ARM) instructions only; Thumb is not supported.
- One core; no SMP or multicore behavior.
- Supported exception modes: USR, SVC, IRQ, ABT, and UND.
- Banked SP/LR registers and SPSR behavior are exposed for exception modes.
- VFP/NEON, FIQ, TrustZone, virtualization, caches, and DMA coherency are out
  of scope.

The toolchain targets freestanding A32 code. Student projects use C and ARM
assembly, normally with `arm-none-eabi-gcc`, `-mcpu=cortex-a9`, `-marm`, and
`-mfloat-abi=soft`.

## Memory Map

| Physical range | Size | Purpose |
|---|---:|---|
| `0x0000_0000` - `0x0000_ffff` | 64 KiB | Immutable platform boot ROM |
| `0x0800_0000` - `0x08ff_ffff` | 16 MiB | Immutable student system ROM |
| `0x1000_0000` - `0x1000_0fff` | 4 KiB | UART |
| `0x1000_1000` - `0x1000_1fff` | 4 KiB | Virtual timer |
| `0x1000_2000` - `0x1000_2fff` | 4 KiB | Interrupt controller |
| `0x1000_3000` - `0x1000_3fff` | 4 KiB | DMA block device |
| `0x1000_4000` - `0x1000_4fff` | 4 KiB | Teaching MMU control/status |
| `0x1000_f000` - `0x1000_ffff` | 4 KiB | Optional debug trace device |
| `0x4000_0000` - `0x43ff_ffff` | 64 MiB | Writable RAM |

MMIO is addressed physically. Once the MMU is enabled, the kernel must map
device pages before accessing them. User mappings must not expose device pages.

## Boot and Images

`minemu image` packages independently linked ELF files into one immutable
system-ROM image. It does not compile source code or perform general-purpose
linking.

The image contains:

- A versioned header and integrity check.
- Kernel loadable segments and entry address.
- A table of user-program modules.
- Each module's entry address, segment metadata, permissions, initialized
  bytes, and BSS size.

At reset, the supplied boot ROM:

1. Starts in SVC mode with IRQ and FIQ masked.
2. Validates the system-ROM header.
3. Copies kernel segments from system ROM to RAM.
4. Zeroes kernel BSS.
5. Writes boot information to RAM.
6. Establishes the initial vector base and stack.
7. Jumps to the kernel entry point with a boot-information pointer in `r0`.

The kernel, not the boot ROM, creates user processes. It selects a module,
allocates new RAM frames, copies the module's code and data from ROM, zeroes
its BSS, allocates a stack, maps the pages, and creates its initial user
context. This permits independent concurrent instances of the same program
without requiring students to parse ELF files.

Student Makefiles compile the kernel and user programs. A typical workflow is:

```sh
make
minemu image --config image.toml --out build/system.rom
minemu run build/system.rom --disk build/disk.img
```

## Exceptions and Interrupts

The platform follows ARMv7-A-style exception entry for the supported modes.
It uses the normal vector offsets for undefined instruction, SVC, prefetch
abort, data abort, and IRQ. The host performs exception entry for events that
Unicorn reports to the platform, preserving the appropriate banked state and
saved CPSR.

The supplied startup assembly provides the vector table and a C dispatch
boundary. Students implement exception handlers and decide their scheduling
and context-switch behavior.

Hardware IRQ delivery occurs at deterministic execution boundaries. Pending
interrupts remain queued while IRQs are masked.

## Virtual Time

Time is a virtual instruction clock, not host wall-clock time.

- The emulator runs bounded guest-instruction batches.
- Strict mode raises timer events after configured guest-instruction counts,
  independent of Unicorn translation-block shape.
- Device completions are scheduled on the same instruction clock.
- Interactive input is queued at the next execution boundary.
- Test input can be scheduled at an exact virtual instruction count.

This makes scheduling and device tests reproducible across supported hosts.

## Teaching MMU

The teaching MMU is a custom, software-defined two-level paging model. Unicorn
executes ARM instructions, while `minemu` uses its virtual-TLB hooks to walk
student-owned page tables and enforce mappings.

| Item | Definition |
|---|---|
| Virtual and physical addresses | 32-bit |
| Page size | 4 KiB |
| Directory | 1,024 entries indexed by VA bits `31:22` |
| Page table | 1,024 entries indexed by VA bits `21:12` |
| PTE permissions | valid, writable, user-accessible, executable |
| Root pointer | 4 KiB-aligned physical `PTBR` |
| Invalidation | Explicit `TLB_FLUSH` MMIO operation |
| Faults | translation, read, write, and execute |

The MMU is disabled after reset, where virtual addresses are identity mapped.
When enabled, a TLB miss causes the host to walk the directory and page table
in guest physical RAM. Missing or prohibited mappings become ABT-mode faults;
the fault virtual address and cause are exposed through MMU status registers.

The model intentionally avoids ARM short-descriptor, domain, and CP15 details
while retaining page allocation, multilevel translation, protection,
per-process address spaces, and replacement-policy work.

### Page Table Entries

Directory and page-table entries are little-endian 32-bit values.

| Entry | Bit | Meaning |
|---|---:|---|
| PDE | 0 | Valid; bits `31:12` are the physical page-table base |
| PTE | 0 | Valid |
| PTE | 1 | Writable |
| PTE | 2 | User accessible |
| PTE | 3 | Executable |
| PTE | `31:12` | Physical frame base |

All unspecified bits must be zero. Supervisor code may access valid user pages;
user code requires the user-accessible bit. Writes and instruction fetches also
require their respective PTE permissions.

## Peripherals

### UART

The UART is a byte-oriented console.

- `TX_DATA`: writing the low byte emits a console byte.
- `RX_DATA`: reading returns and consumes the next queued host byte.
- `STATUS`: receive-ready and transmit-ready state.
- `CONTROL`: receive-interrupt enable.

The console supports normal printable text, carriage return, line feed, and
backspace. It does not promise ANSI terminal emulation.

### Timer

The timer has a period, enable state, periodic mode, interrupt-enable state,
and acknowledge operation. It schedules events against the virtual instruction
clock and can raise a timer IRQ.

### Interrupt Controller

The controller provides UART receive, timer, and block-completion sources. It
has pending and enable bitmaps plus claim and EOI operations. Sources use a
fixed documented priority order.

### Block Device

The block device presents a persistent raw disk image with 512-byte sectors.
Students write a simple DMA driver using command, LBA, sector count, physical
RAM buffer address, status, and acknowledge registers.

Reads and writes complete after a deterministic virtual-time delay. The device
copies data between the raw disk file and guest physical RAM, then raises a
completion IRQ. Invalid LBAs, unaligned DMA buffers, and DMA outside RAM
produce documented errors.

### Trace Device

The optional trace device records course-defined diagnostic events such as
context switches, page evictions, or filesystem operations. It never affects
guest correctness or grading.

## MMIO Register Layout

All device registers are little-endian 32-bit values. Accesses of a different
width are permitted only where the device documentation explicitly allows them.

### UART (`0x1000_0000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `RX_DATA` | R | Low byte is the next queued input byte; read consumes it |
| `0x04` | `TX_DATA` | W | Low byte is appended to console output |
| `0x08` | `STATUS` | R | Bit 0: RX ready; bit 1: TX ready |
| `0x0c` | `CONTROL` | RW | Bit 0: enable RX interrupt |

TX ready is always set. A queued input byte with RX interrupts enabled raises
the UART source in the interrupt controller.

### Timer (`0x1000_1000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `PERIOD` | RW | Guest-instruction interval; zero is invalid |
| `0x04` | `CONTROL` | RW | Bit 0: enabled; bit 1: periodic; bit 2: IRQ enabled |
| `0x08` | `STATUS` | R | Bit 0: event pending |
| `0x0c` | `ACK` | W | Writing bit 0 clears the pending event |

### Interrupt Controller (`0x1000_2000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `PENDING` | R | Pending source bitmap |
| `0x04` | `ENABLE` | RW | Enabled source bitmap |
| `0x08` | `CLAIM` | R | Highest-priority active source, or `0xffff_ffff` if none |
| `0x0c` | `EOI` | W | Complete the claimed source index |

Source 0 is UART receive, source 1 is timer, and source 2 is block completion.
Lower source indices have higher priority. A source is eligible when it is both
pending and enabled.

### Block Device (`0x1000_3000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `COMMAND` | W | `1`: read; `2`: write |
| `0x04` | `LBA` | RW | First 512-byte sector |
| `0x08` | `SECTOR_COUNT` | RW | Requested sector count |
| `0x0c` | `DMA_PADDR` | RW | Physical RAM buffer address |
| `0x10` | `STATUS` | R | Bit 0: busy; bit 1: complete; bit 2: error |
| `0x14` | `ERROR` | R | Device-defined error code |
| `0x18` | `ACK` | W | Writing bit 0 clears complete and error state |
| `0x1c` | `CONTROL` | RW | Bit 0: enable completion IRQ |

Commands require an aligned RAM buffer and a nonzero sector count. A command
issued while busy fails with an error. The device performs no address
translation; DMA addresses are always physical.

### Teaching MMU (`0x1000_4000`)

| Offset | Name | Access | Definition |
|---:|---|---|---|
| `0x00` | `CTRL` | RW | Bit 0 enables translation and protection checks |
| `0x04` | `PTBR` | RW | 4 KiB-aligned physical page-directory address |
| `0x08` | `TLB_FLUSH` | W | Any write invalidates cached translations |
| `0x0c` | `FAULT_VA` | R | Virtual address from the most recent MMU fault |
| `0x10` | `FAULT_STATUS` | R | Fault cause and access metadata |

`FAULT_STATUS` values are: 1 for translation, 2 for read protection, 3 for
write protection, and 4 for execute protection. Bit 8 identifies a user-mode
fault, bit 9 a write access, and bit 10 an instruction fetch.

## Observability and Testing

The emulator records bounded hardware events, including UART I/O, timer
expiration, IRQ delivery, block requests, exceptions, and MMU faults. These
events feed the TUI and headless test harness.

`minemu test` runs a completed ROM image with scripted input and expected
console or machine-state assertions. Tests use the same emulator core as the
interactive TUI and do not require the TUI to run.
