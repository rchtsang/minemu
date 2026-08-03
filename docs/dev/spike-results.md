# Feasibility Spike Results

## Purpose

The spike established that Unicorn can support the proposed A32 teaching
machine before the production implementation is built. It is a narrow Rust
prototype, not the foundation for the production codebase. The spike remains
on the `spike` branch while `main` is reset before Cargo initialization and
rewritten around the decisions below.

## Validated Capabilities

- A32 Cortex-A9 ELF fixtures load into guest RAM and execute through Unicorn.
- A boot ROM copies a kernel from system ROM into RAM, zeroes BSS, establishes
  initial state, and transfers control to the kernel.
- UART input/output, timer, interrupt controller, block device, ROM, RAM, and
  MMIO are available to guest fixtures.
- The MMU implements the documented two-level 4 KiB page-table format with
  valid, readable, writable, user, and executable permissions.
- The MMU maps RAM, boot ROM, system ROM, and implemented supervisor-only MMIO
  pages. User mappings cannot expose device pages.
- Translation, read, write, user, and execute failures enter the abort path
  with fault address and status metadata.
- CP15 virtualization supports TTBR0, SCTLR.M, VBAR, TLBIALL decoding, DFAR,
  and DFSR reads. CP15 transition fixtures prove MMU enable and mapped access.
- Guest fixtures configure the timer and interrupt controller through MMIO.
  Masked timer and block sources remain pending, timer has priority over block,
  and source acknowledgement plus EOI prevents redelivery.
- Timer and block deadlines use the virtual instruction clock. Timer expiry
  remains exact through MMU-abort recovery and subsequent IRQ delivery.
- Block DMA reads and writes complete on deterministic virtual-time deadlines.
  Malformed disk images, invalid DMA, missing media, and persistence failures
  complete without leaving the device busy.
- The emulator service confines Unicorn access to one thread, bounds lifecycle
  and input queues, gives lifecycle controls priority, and publishes immutable
  snapshots.
- The test suite establishes a representative strict instruction-counting
  throughput floor of 100,000 instructions per second.

## Verification

The spike passed:

```sh
just test
just check
just fmt
```

## Important Limits

- `MCR TLBIALL` is decoded but still lacks a dedicated reliability proof for
  process-to-process TTBR switching.
- The current MMU ABI is simplified and is not ARMv7 short-descriptor VMSA.
  CP15 is the control interface; it does not imply that Unicorn's native MMU
  can consume the specified page tables.
- Unicorn's native CP15/MMU implementation is available through the Rust
  binding, but adopting it would require changing the guest ABI to ARM VMSA
  page tables, permissions, domains, fault encodings, and related behavior.
- The spike supports only part of the documented exception model. The rewrite
  must implement the documented undefined-instruction and prefetch-abort paths
  as well as SVC, data abort, and IRQ.
- The current code has backend-specific virtual-TLB fault recovery and is not
  suitable as the production architecture.

## Findings For The Rewrite

- CP15 handling must explicitly enforce privilege and A32 condition semantics.
- UART receive-interrupt control and pending behavior must be implemented to
  match the documented register ABI.
- Host APIs, MMIO, and CP15 must share one MMU-control state machine with the
  same page-directory and page-table validation rules.
- MMIO must validate access width, alignment, and direction through a typed
  bus layer.
- The exception controller must be independent of Unicorn callbacks. Backend
  stop/restart and virtual-TLB workarounds belong in a narrow adapter layer.
- The block device must use write-back storage: clone the disk image into host
  RAM on attach, update that copy for guest writes, and track dirty sectors.
  Flush on pause, shutdown, and execution failure. A flush failure must retain
  dirty state for a later retry.
- Snapshot publication should provide small status updates at a cadence or on
  material state changes. Larger inspection data should be requested by the
  TUI only when needed.
- Service termination, including reset-factory failure, must follow one path
  that publishes a terminal status and error.
- The one-segment boot image format is spike-only. The production image format
  must be versioned and support the documented kernel segments and modules.

## Proposed Structure

1. Platform ABI definitions: memory map, MMIO registers, IRQ IDs, page-table
   structures, fault codes, image structures, and validation.
2. Machine core: physical memory lifecycle, CPU execution coordination,
   virtual clock, and snapshot projection.
3. Devices: UART, timer, interrupt controller, block device, and ROM behind a
   typed MMIO bus.
4. MMU and exception subsystem: page-table walker, CP15 interface, fault
   records, and exception entry.
5. Unicorn adapter: CPU setup, hooks, backend stop reasons, virtual-TLB
   integration, and no platform policy beyond adapting backend events.
6. Image subsystem: versioned parser and packer.
7. Runtime service: lifecycle state machine, bounded commands, write-back
   flushing, and cadence/material-change snapshot publication.

## Virtual-Time Contract

The production machine uses a deterministic logical instruction-progress
clock, not hardware-cycle simulation.

- A successfully completed instruction costs one tick.
- An SVC, undefined instruction, or other trap costs one tick, followed by a
  one-tick exception-entry cost.
- A data or prefetch fault costs zero ticks for the faulting instruction,
  followed by a one-tick exception-entry cost.
- A device command costs the normal instruction tick. Successful and failed
  completions occur at the same scheduled device deadline.
- Host or backend failures add no guest time after execution stops.

This gives faults and traps explicit, deterministic cost without counting a
retried faulting access as completed work.
