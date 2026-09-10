# Device ABI Version 1

> **Status: Normative.** This document defines version-1 MMIO registers,
> peripheral state transitions, interrupt levels, and deterministic deadlines.

## Common MMIO Rules

All register positions below are MMIO offsets from the device page's PA base,
not absolute addresses. All registers are 32-bit little-endian words.

**MINEMU-DEV1-MMIO-001:** Every MMIO access MUST be one aligned 32-bit read or
write to a listed register in the permitted direction. Byte, halfword, 64-bit,
unaligned, instruction-fetch, unlisted-offset, and wrong-direction accesses are
invalid device accesses.

**MINEMU-DEV1-MMIO-002:** A write containing bits or values excluded by a
register definition is an invalid device access and MUST have no device state
effect. Invalid accesses produce the fault specified by
[Exceptions and MMU ABI v1](exceptions-and-mmu-v1.md#faults).

## Reset State

| Device state | Reset value |
|---|---|
| Interrupt `PENDING`, `ENABLE`, active claim | `0`, `0`, none |
| Interrupt priorities | SysTick `0`, UART0 `64`, UART1 `64`, block `128` |
| SysTick `PERIOD`, `CONTROL`, `STATUS`, deadline | `0`, `0`, `0`, none |
| Block `LBA`, `SECTOR_COUNT`, `PADDR`, `CONTROL`, `UNIT` | All `0` |
| Block `STATUS`, `ERROR`, active request, per-unit dirty sets | `0`, `0`, none, empty |
| RNG configured seed and state | `0x4d45_4d55` |
| UART0/1 RX and TX queues, `CONTROL` | Empty, empty, `0` |
| Trace event history | Empty |

**MINEMU-DEV1-RESET-001:** Platform reset MUST establish every value in the
reset-state table and cancel asynchronous deadlines.

**MINEMU-DEV1-RESET-002:** Attached block-media bytes for both units MUST remain
attached and unchanged across a successful reset. Before rebuilding device
state, the host MUST attempt to flush both dirty media, even if one flush fails.
If either flush fails, reset MUST fail rather than discard dirty data or begin
guest execution.

## Interrupt Controller

Base PA: `0x1000_0000`. Source IDs and corresponding bits are SysTick `0`,
UART0 `1`, UART1 `2`, and block `3`.

| Offset | Register | Access | Value |
|---:|---|---|---|
| `0x00` | `PENDING` | R | Raw peripheral level bits `[3:0]` |
| `0x04` | `ENABLE` | R/W | Delivery-enable bits `[3:0]`; other write bits invalid |
| `0x08` | `CLAIM` | R | Active/selected source ID, or `0xffff_ffff` |
| `0x0c` | `EOI` | W | Complete active claim by exact source ID |
| `0x10` | `PRIORITY_SYSTICK` | R/W | Priority byte |
| `0x14` | `PRIORITY_UART0` | R/W | Priority byte |
| `0x18` | `PRIORITY_UART1` | R/W | Priority byte |
| `0x1c` | `PRIORITY_BLOCK` | R/W | Priority byte |

**MINEMU-DEV1-INTC-001:** `PENDING` MUST reflect peripheral levels independent
of `ENABLE`. A source is claimable exactly when its pending and enable bits are
both set.

**MINEMU-DEV1-INTC-002:** Reading `CLAIM` with no active claim MUST select the
claimable source with the numerically lowest priority value, breaking equal
priority by lowest source ID. It MUST retain and return that source on later
reads until successful EOI. If no source is claimable it returns
`0xffff_ffff`.

**MINEMU-DEV1-INTC-003:** IRQ delivery performs the same selection and retention
as a `CLAIM` read before vector entry. No second source may become active while
a claim is retained.

**MINEMU-DEV1-INTC-004:** An `EOI` write MUST equal the retained source ID and
there MUST be an active claim. Otherwise the write is invalid. Successful EOI
clears only the claim; it does not acknowledge the peripheral level.

**MINEMU-DEV1-INTC-005:** Priority writes MUST be in `[0, 255]` and `ENABLE`
writes MUST be in `[0, 15]`. Changes affect the next selection, not an already
retained claim.

## SysTick

Base PA: `0x1000_1000`; interrupt source ID `0`.

| Offset | Register | Access | Value |
|---:|---|---|---|
| `0x00` | `PERIOD` | R/W | Nonzero tick interval required on write |
| `0x04` | `CONTROL` | R/W | Bit 0 enable, bit 1 periodic, bit 2 IRQ enable |
| `0x08` | `STATUS` | R | Bit 0 pending |
| `0x0c` | `ACK` | W | Exact value `1` clears pending |

**MINEMU-DEV1-TIMER-001:** Writing `CONTROL` with Enable transitioning from zero
to one MUST schedule the next deadline at `T + PERIOD`, where `T` is the issue
tick, if `PERIOD` is nonzero. Clearing Enable cancels the deadline. IRQ-enable
changes immediately recompute the interrupt level without clearing pending.

**MINEMU-DEV1-TIMER-002:** Writing `PERIOD` while enabled MUST rephase the next
deadline to `T + new_PERIOD`. A `PERIOD` write of zero or a `CONTROL` write with
bits outside `[2:0]` is invalid.

**MINEMU-DEV1-TIMER-003:** At an expired deadline the timer MUST set pending. In
one-shot mode it MUST also clear Enable and cancel the deadline. In periodic
mode it MUST preserve phase by advancing the deadline by whole `PERIOD`
intervals until it is strictly later than current virtual time.

**MINEMU-DEV1-TIMER-004:** The SysTick interrupt level is
`STATUS.pending AND CONTROL.irq_enable`. Pending remains set across later
deadlines until an exact `ACK = 1` write clears it.

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

**MINEMU-DEV1-BLOCK-001:** A command write while idle MUST snapshot `COMMAND`,
`LBA`, `SECTOR_COUNT`, `PADDR`, and `UNIT`, clear prior Complete/Error state, set
Busy, and schedule completion at `T + 32` ticks. Later staging-register writes
MUST NOT change the active request. Command validity and transfer errors MUST be
reported at that deadline, not synchronously.

**MINEMU-DEV1-BLOCK-002:** At the deadline, the device MUST clear Busy, attempt
the complete transfer, set Complete, and set Error plus its code on failure.
Success and guest-visible failure have the same 32-tick latency.

**MINEMU-DEV1-BLOCK-003:** The DMA PA MUST be 512-byte aligned and the half-open
range `[PADDR, PADDR + 512 * SECTOR_COUNT)` MUST fit completely in RAM without
arithmetic overflow. `SECTOR_COUNT` MUST be nonzero; zero reports Invalid DMA.
The media range beginning at LBA with the same byte length MUST fit the attached
medium.

**MINEMU-DEV1-BLOCK-004:** Command `1` MUST copy bytes from the selected unit
into RAM. Command `2` MUST copy RAM bytes into the selected unit's write-back
media and mark every written sector dirty for that unit. Media bytes and dirty
sets MUST remain independent between units. Later guest reads MUST observe the
updated write-back bytes without waiting for host persistence.

**MINEMU-DEV1-BLOCK-005:** The controller permits one active request across both
units. A command write while Busy MUST leave the active request and its deadline
unchanged, immediately set Complete and Error with code Busy, and recompute the
interrupt level. The active request may later replace that completion result at
its normal deadline.

**MINEMU-DEV1-BLOCK-006:** The block interrupt level is
`STATUS.complete AND CONTROL.irq_enable`. An exact `ACK = 1` clears Complete and
Error but does not cancel an active request. Changing IRQ enable immediately
recomputes the level.

**MINEMU-DEV1-BLOCK-007:** A host flush failure MUST leave dirty tracking intact,
set Complete and Error with code Deferred persistence, and recompute the
interrupt level. Host-file layout is a contiguous sector image where LBA `n`
corresponds to host-file byte offset `512 * n`; a host path and flush schedule
are host configuration, not guest addresses.

**MINEMU-DEV1-BLOCK-008:** A request that snapshots a unit other than `0` or `1`
MUST complete at its normal deadline with Invalid Unit. A supported unit without
attached media MUST instead complete with No Media.

**MINEMU-DEV1-BLOCK-009:** Both units share `STATUS`, `ERROR`, `ACK`, `CONTROL`,
and interrupt source `3`. The host MUST track and flush each unit independently,
attempt both flushes when both are dirty, preserve dirty state for each failed
flush, and reject configuration of the same canonical host path for both units.

## Deterministic RNG

Base PA: `0x1000_3000`.

| Offset | Register | Access | Value |
|---:|---|---|---|
| `0x00` | `SEED` | R/W | Configured effective seed |
| `0x04` | `DATA` | R | Advance xorshift32 and return new state |
| `0x08` | `STATE` | R | Current state without advancing |

**MINEMU-DEV1-RNG-001:** Reset MUST set both configured seed and state to
`0x4d45_4d55`. Writing nonzero `x` to `SEED` MUST set both to `x`; writing zero
MUST set both to `0x4d45_4d55`.

**MINEMU-DEV1-RNG-002:** A `DATA` read MUST perform, with 32-bit wrapping and in
this order, `x ^= x << 13`, `x ^= x >> 17`, `x ^= x << 5`, then store and
return `x`. `SEED` and `STATE` reads MUST NOT advance state.

## UART0 and UART1

UART0 base PA is `0x1000_4000` with interrupt source ID `1`. UART1 base PA is
`0x1000_5000` with source ID `2`. Their behavior is otherwise identical.

| Offset | Register | Access | Value |
|---:|---|---|---|
| `0x00` | `RX_DATA` | R | Oldest received byte, or zero if empty |
| `0x04` | `TX_DATA` | W | Low byte to transmit; high bits must be zero |
| `0x08` | `STATUS` | R | Bit 0 RX ready, bit 1 TX ready |
| `0x0c` | `CONTROL` | R/W | Bit 0 RX IRQ enable |

**MINEMU-DEV1-UART-001:** `STATUS.TX_READY` MUST always be one.
`STATUS.RX_READY` MUST be one exactly when the receive queue is non-empty.
Reading `RX_DATA` removes and returns the oldest byte, or returns zero without
queue effect when empty.

**MINEMU-DEV1-UART-002:** Each receive queue MUST retain at most 4096 bytes. A
host-injected byte appends to the queue; when full, it first discards the oldest
byte. Host input timing is test/runtime input, not autonomous device behavior.

**MINEMU-DEV1-UART-003:** `TX_DATA` writes append the low byte to the port's
transmit history, which retains at most 8192 bytes and discards the oldest when
full. `CONTROL` writes may contain only bit 0 and MUST read back that bit.

**MINEMU-DEV1-UART-004:** Each UART interrupt level is
`CONTROL.rx_irq_enable AND STATUS.rx_ready` and MUST be recomputed after receive,
RX_DATA read, or CONTROL write.

## Trace Device

Base PA: `0x1000_f000`.

| Offset | Register | Access | Value |
|---:|---|---|---|
| `0x00` | `EVENT` | W | Arbitrary 32-bit event value |

**MINEMU-DEV1-TRACE-001:** A successful EVENT write MUST commit one observable
`(tick, value)` event when the issuing instruction retires. The event tick is
the virtual time after that instruction's one-tick cost is applied.

**MINEMU-DEV1-TRACE-002:** Trace history MUST retain at most 4096 committed
events, discarding the oldest on overflow. The device has no readable register
and generates no interrupt.
