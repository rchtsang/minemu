# Device ABI Version 2

> **Status: Normative.** This document defines the version-2 device changes and
> incorporates the version-1 device contract where it is not replaced below.

Device ABI v2 retains the MMIO rules, interrupt controller, SysTick, RNG, UART,
and trace requirements from [Device ABI v1](devices-v1.md). It replaces the
version-1 block-device contract and its block-related reset and persistence
requirements with the requirements below. The physical map and interrupt source
assignments do not change.

## Reset State

The Device ABI v1 reset table applies with these replacement block rows:

| Device state | Reset value |
|---|---|
| Block `LBA`, `SECTOR_COUNT`, `PADDR`, `CONTROL`, `UNIT` | All `0` |
| Block `STATUS`, `ERROR`, active request, per-unit dirty sets | `0`, `0`, none, empty |

**MINEMU-DEV2-RESET-001:** Platform reset MUST establish the inherited Device
ABI v1 reset values and the replacement block values above, cancel asynchronous
deadlines, and set `UNIT` to zero.

**MINEMU-DEV2-RESET-002:** Attached block-media bytes for both units MUST remain
attached and unchanged across a successful reset. Before rebuilding device
state, the host MUST attempt to flush both dirty media, even if one flush fails.
If either flush fails, reset MUST fail rather than discard dirty data or begin
guest execution.

## Block Device

Base PA: `0x1000_2000`; interrupt source ID `3`; sector size 512 bytes.

| Offset | Register | Access | Value |
|---:|---|---|---|
| `0x00` | `COMMAND` | W | `1` media-to-RAM read, `2` RAM-to-media write |
| `0x04` | `LBA` | R/W | First 512-byte logical block |
| `0x08` | `SECTOR_COUNT` | R/W | Transfer length in sectors |
| `0x0c` | `PADDR` | R/W | First DMA PA |
| `0x10` | `STATUS` | R | Bit 0 busy, bit 1 complete, bit 2 error |
| `0x14` | `ERROR` | R | Error code below |
| `0x18` | `ACK` | W | Exact value `1` clears complete and error |
| `0x1c` | `CONTROL` | R/W | Bit 0 completion IRQ enable |
| `0x20` | `UNIT` | R/W | Medium selected for the next command |

Units `0` and `1` are supported and have identical raw-sector behavior. The
course environment uses unit `0` for filesystem/general storage and unit `1`
for swap. This convention is not a hardware restriction.

| Error | Name | Meaning |
|---:|---|---|
| `0` | None | Successful/no error |
| `1` | No media | No medium attached |
| `2` | Busy | Command written while a request was active |
| `3` | Invalid command | Command other than `1` or `2` |
| `4` | Invalid DMA | Misaligned, overflowing, or non-RAM DMA range |
| `5` | Invalid LBA | Media byte range outside attached media or size overflow |
| `6` | Deferred persistence | Host write-back flush failed |
| `7` | Invalid unit | Snapshotted unit is not `0` or `1` |

**MINEMU-DEV2-BLOCK-001:** A command write while idle MUST snapshot `COMMAND`,
`LBA`, `SECTOR_COUNT`, `PADDR`, and `UNIT`, clear prior Complete/Error state, set
Busy, and schedule completion at `T + 32` ticks. Later staging-register writes
MUST NOT change the active request. Command validity and transfer errors MUST be
reported at that deadline, not synchronously.

**MINEMU-DEV2-BLOCK-002:** At the deadline, the device MUST clear Busy, attempt
the complete transfer, set Complete, and set Error plus its code on failure.
Success and guest-visible failure have the same 32-tick latency.

**MINEMU-DEV2-BLOCK-003:** The DMA PA MUST be 512-byte aligned and the half-open
range `[PADDR, PADDR + 512 * SECTOR_COUNT)` MUST fit completely in RAM without
arithmetic overflow. `SECTOR_COUNT` MUST be nonzero; zero reports Invalid DMA.
The selected unit's media range beginning at LBA with the same byte length MUST
fit the attached medium.

**MINEMU-DEV2-BLOCK-004:** Command `1` MUST copy bytes from the selected unit
into RAM. Command `2` MUST copy RAM bytes into the selected unit's write-back
media and mark every written sector dirty for that unit. Media bytes and dirty
sets MUST remain independent between units. Later guest reads MUST observe the
updated write-back bytes without waiting for host persistence.

**MINEMU-DEV2-BLOCK-005:** The controller permits one active request across both
units. A command write while Busy MUST leave the active request and its deadline
unchanged, immediately set Complete and Error with code Busy, and recompute the
interrupt level. The active request may later replace that completion result at
its normal deadline.

**MINEMU-DEV2-BLOCK-006:** The block interrupt level is
`STATUS.complete AND CONTROL.irq_enable`. An exact `ACK = 1` clears Complete and
Error but does not cancel an active request. Changing IRQ enable immediately
recomputes the level.

**MINEMU-DEV2-BLOCK-007:** A host flush failure MUST retain dirty tracking for
the unit whose flush failed, set Complete and Error with code Deferred
persistence, and recompute the interrupt level. Host-file layout is a contiguous
sector image where LBA `n` corresponds to host-file byte offset `512 * n`; host
paths and the flush schedule are host configuration, not guest addresses.

**MINEMU-DEV2-BLOCK-008:** A request that snapshots a unit other than `0` or `1`
MUST complete at its normal deadline with Invalid Unit. A supported unit without
attached media MUST instead complete with No Media.

**MINEMU-DEV2-BLOCK-009:** Both units share `STATUS`, `ERROR`, `ACK`, `CONTROL`,
and interrupt source `3`. The host MUST track and flush each unit independently,
attempt both flushes when both are dirty, preserve dirty state for each failed
flush, and reject configuration of the same canonical host path for both units.
