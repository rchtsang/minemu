# Rewrite Implementation Plan

## Goal

Replace the feasibility spike with a maintainable A32 teaching platform. The
production implementation must preserve the documented deterministic machine
contract while separating platform policy from Unicorn-specific adaptation.

The `spike` branch remains the evidence source for backend behavior and test
scenarios. Do not copy its monolithic implementation into this workspace.

## Agreed ABI Decisions

- [x] The CPU is a little-endian, single-core ARMv7-A Cortex-A9 running A32
  instructions only.
- [x] CP15 is the sole privileged MMU control interface. Remove the competing
  MMIO MMU-control ABI from the production specification.
- [x] The MMU remains the specified custom two-level, 4 KiB paging model, not
  ARM VMSA short descriptors.
- [x] PTE permissions are valid, readable, writable, user, and executable.
  All reserved PTE/PDE bits are rejected.
- [x] PTEs include MMU-managed Accessed and Dirty bits plus five kernel-owned
  software metadata bits for replacement policy hints.
- [x] Physical RAM is `0x4000_0000..0x43ff_ffff`. The kernel has a fixed
  higher-half direct mapping at `0xc000_0000..0xc3ff_ffff`.
- [x] Boot uses a physical trampoline, enables the MMU, and branches to the
  higher-half kernel entry.
- [x] User modules are fixed-address, non-PIE A32 images with no dynamic
  relocation support in v1.
- [x] Supported exception paths are undefined instruction, SVC, prefetch
  abort, data abort, and IRQ.
- [x] Virtual time is logical instruction progress:
  - [x] A completed instruction costs one tick.
  - [x] SVC and undefined instructions cost one tick plus one exception-entry
    tick.
  - [x] Data and prefetch faults cost zero ticks for the faulting instruction
    plus one exception-entry tick.
  - [x] Device success and failure complete at the same scheduled deadline.
  - [x] Host/backend failure adds no time after execution stops.
- [x] The block device is write-back. It clones its disk into host memory on
  attach, tracks dirty sectors, and flushes only on pause, shutdown, or a
  terminal emulator/backend failure. Failed flushes retain dirty state for
  retry. Guest block command errors do not trigger a host flush.
- [x] `libgcc` is an allowed static toolchain dependency. Newlib and
  newlib-nano are not part of the supported platform.
- [x] Device MMIO is supervisor-only, including the RNG. User code accesses
  devices through kernel services.
- [x] The interrupt controller assigns nonnegative source IDs and configurable
  priorities. SysTick defaults above UART0, UART1, and block completion.
- [x] UART0 and UART1 are independent MMIO peripherals with separate RX IRQ
  sources and priorities.
- [x] The trace device records a supervisor-written `u32` event at the
  issuing instruction's completed virtual tick.
- [x] The boot ROM trusts image contents initially. The image packer validates
  inputs, but boot-time integrity validation is deferred.

## 0. Freeze ABI v1

- [x] Rewrite `docs/dev/emulator.md` as the normative ABI document and remove
  stale spike terminology.
- [x] Specify CP15 operations, privilege checks, A32 condition behavior, and
  behavior for unsupported CP15 operations.
- [x] Specify all exception vector offsets, saved LR/SPSR values, banked-stack
  ownership, and return rules.
- [x] Specify the physical trampoline, high-half mapping, kernel link/load
  addresses, VBAR transition, and boot-info handoff.
- [x] Specify MMIO access width, alignment, read/write permissions, and error
  behavior for every device register.
- [x] Specify IRQ priorities, pending/claim/ACK/EOI behavior, device error
  codes, and virtual device latencies.
- [x] Specify MMU Accessed/Dirty behavior, kernel software metadata bits, and
  prefetch/data-abort page-fault recovery.
- [x] Specify dual UART source IDs, configurable source-priority registers,
  SysTick defaults, and trace-event semantics.
- [x] Specify the versioned image and boot-info wire formats with explicit
  little-endian fields and bounds rules.
- [x] Build a conformance matrix mapping each ABI rule to a Rust test, guest
  fixture, template smoke test, or headless image test.

## 1. Implement `minemu-platform`

- [ ] Define checked physical/virtual address and range types.
- [ ] Define the memory map, MMIO register offsets, access permissions, IRQ
  IDs, fault codes, and device status/error values.
- [ ] Define typed MMIO transactions with width, alignment, direction, address,
  and value.
- [ ] Define PTE/PDE parsing and validation, including RAM-only page-table
  backing, Accessed/Dirty updates, software metadata preservation, and
  reserved-bit rejection.
- [ ] Define backend-neutral CP15 operation and exception request types.
- [ ] Define stable, explicit little-endian image-header, segment, module, and
  boot-info records. Never serialize Rust structs directly.
- [ ] Add ABI unit tests for encoding, decoding, overflow, malformed ranges,
  page-table validation, and all fault-status combinations.

## 2. Implement `minemu-core`

- [ ] Implement physical memory/ROM lifecycle and a typed MMIO bus with
  centralized width, alignment, direction, and access validation.
- [ ] Implement UART0/UART1 state, bounded RX/TX histories, RX-ready status,
  and independently configurable RX interrupt behavior.
- [ ] Implement SysTick state and exact virtual deadline scheduling.
- [ ] Implement interrupt-controller state with deterministic priority,
  pending, enable, claim, EOI, source acknowledgement, and configurable source
  priorities.
- [ ] Implement block-device command state, physical-RAM-only DMA validation,
  deterministic completion, guest-visible errors, write-back media, dirty
  sector tracking, and flush/retry state.
- [ ] Implement deterministic MMIO RNG at `0x1000_3000`:
  - [ ] `SEED` is read/write, `DATA` advances and returns the next `u32`, and
    `STATE` exposes current state for inspection.
  - [ ] Use a specified `xorshift32` transition and a documented nonzero
    default/zero-seed policy.
  - [ ] Do not emit an event for every RNG read.
- [ ] Implement trace EVENT writes with retired-instruction timestamps and
  bounded event-history behavior without affecting guest correctness.
- [ ] Implement the MMU walker through a narrow physical-memory interface.
- [ ] Implement exception planning and fault records without Unicorn callbacks.
- [ ] Implement the virtual scheduler, bounded event history, small status
  projection, and on-demand inspection response types.
- [ ] Unit-test every device state machine independently of Unicorn.

## 3. Implement `minemu-unicorn`

- [ ] Configure Unicorn as A32 Cortex-A9 with physical RAM, ROM, and typed MMIO
  mappings.
- [ ] Adapt Unicorn MMIO callbacks into core MMIO transactions only.
- [ ] Implement bounded instruction execution with explicit backend stop
  reasons.
- [ ] Integrate Unicorn virtual-TLB callbacks with the core MMU walker.
- [ ] Implement CP15 interception with explicit A32 condition evaluation and
  privileged-access checks.
- [ ] Apply CP15 state changes only at safe execution boundaries.
- [ ] Implement CPU register, CPSR/SPSR, banked SP/LR, VBAR, and vector entry
  mechanics required by the core exception plan.
- [ ] Deliver undefined, SVC, prefetch abort, data abort, and IRQ paths with
  correct state and virtual-time accounting.
- [ ] Keep TLB fallback/restart workarounds isolated to this crate.
- [ ] Prove CP15 TTBR switch plus TLBIALL reliability across two address spaces
  before process/context-switch coursework depends on it.

## 4. Implement `minemu-image`

- [ ] Define a versioned system-ROM image layout with kernel segment and user
  module tables, explicit offsets/lengths, permissions, BSS metadata, entry
  points, and reserved fields.
- [ ] Parse and validate independently linked A32 ELF inputs.
- [ ] Reject Thumb entries, unsupported relocations, overlapping segments,
  invalid ranges, duplicate module names, and ROM overflow.
- [ ] Produce byte-identical images for identical inputs.
- [ ] Implement boot-ROM handling for multi-segment kernel copy, BSS zeroing,
  boot-info publication, physical trampoline entry, and high-half handoff.
- [ ] Add corrupt/truncated/version-mismatched image parser tests. Boot-time
  image integrity validation is intentionally deferred.

## 5. Build the Student ARM Platform

- [ ] Add `platform/arm/include/minemu/platform.h` with the stable memory map,
  constants, dual-UART definitions, and a limited set of raw MMIO helpers.
- [ ] Add `mmu.h`, `boot.h`, `trap.h`, and `syscall.h` with fixed-width,
  student-facing ABI definitions.
- [ ] Use packed, four-byte-aligned MMIO register structs with `_Static_assert`
  checks for size, alignment, and field offsets.
- [ ] Use explicit masks for register bits; do not use C bit-fields.
- [ ] Add kernel and user linker scripts for physical bootstrap and fixed
  higher-half/user virtual addresses.
- [ ] Add startup/vector assembly and normalized trap-frame mechanics for all
  five supported exception paths.
- [ ] Initialize SVC, IRQ, ABT, and UND stacks according to the ABI.
- [ ] Build `libminemu_rt.a` with freestanding memory primitives and
  panic/halt support, but no UART driver convenience API.
- [ ] Document and link `libgcc` explicitly through supported Makefile rules.
- [ ] Provide kernel, user, and system templates as separate student projects.
- [ ] Require students to implement low-level device drivers, especially UART,
  using exposed definitions and raw MMIO helpers.
- [ ] Verify template builds have no hosted libc/newlib dependency.

## 6. Implement `minemu-runtime`

- [ ] Run the concrete machine exclusively on the emulator thread.
- [ ] Define starting, running, paused, stopping, stopped, and failed lifecycle
  states.
- [ ] Implement prioritized, bounded lifecycle commands and bounded/coalesced
  UART input.
- [ ] Implement pause, reset, shutdown, and terminal-error paths with one final
  status publication.
- [ ] Flush dirty disk state on pause, shutdown, and terminal backend failure.
- [ ] Publish lightweight immutable status at a cadence or material state
  change; serve larger CPU/MMU/memory/device inspection on request.
- [ ] Prove slow snapshot consumers cannot block guest execution or accumulate
  unbounded state.

## 7. Implement CLI and Test Runner

- [ ] Add `minemu image` to package a system image from a declarative manifest.
- [ ] Add `minemu run` to load an image, attach optional write-back media, and
  run headlessly or through the runtime service.
- [ ] Add `minemu test` to run headless images with scheduled input and
  assertions over console output, events, faults, and machine state.
- [ ] Ensure diagnostics identify image, ELF, device, CP15, and runtime errors
  without exposing backend internals as ABI.

## 8. Build the TUI

- [ ] Implement a Ratatui/Crossterm TUI that owns terminal mode and renders
  only immutable status/inspection responses.
- [ ] Implement console input, pause/resume, reset, shutdown, and inspector
  selection commands.
- [ ] Render console, machine status, hardware status, bounded event history,
  and on-demand MMU/memory inspection.
- [ ] Preserve console visibility in small terminals.
- [ ] Restore the terminal after normal exit, emulator failure, and panic.

## 9. Port Fixtures and Release the Platform

- [ ] Port spike scenarios into `fixtures/arm` as backend/ABI conformance
  tests, separate from student templates.
- [ ] Cover ROM boot, dual-UART polling/IRQ, SysTick deadlines, configurable
  IRQ priority, block success/error/write-back paths, RNG sequences, trace
  events, MMU replacement bits and permissions, CP15, all exception paths, and
  process TTBR/TLBIALL switches.
- [ ] Build the fixture suite through `just fixtures/test`.
- [ ] Add template smoke tests: build kernel/user projects, package an image,
  boot it, run scripted input, and assert output/state.
- [ ] Maintain the 100,000 strict-instructions-per-second representative
  throughput floor.
- [ ] Build the student `dev` container with Zsh, ARM GCC, pinned Just, and
  required native Unicorn build tools.
- [ ] Add a multi-stage release image that includes a prebuilt `minemu` binary
  plus version-matched platform artifacts.
- [ ] Publish versioned `linux/amd64` and `linux/arm64` images only after
  workspace, fixture, template, and container smoke tests pass.

## Completion Criteria

- [ ] All ABI rules are documented and covered by conformance tests.
- [ ] No production core type exposes Unicorn callbacks or types.
- [ ] The booted higher-half kernel handles every documented exception path.
- [ ] A kernel creates and runs a fixed-address user module from system ROM.
- [ ] MMU-backed supervisor device mappings and user isolation are proven.
- [ ] CP15 process address-space switching is reliable.
- [ ] Virtual time and device deadlines remain deterministic across traps and
  faults.
- [ ] Dirty disks flush only at the documented boundaries and recover from
  flush errors.
- [ ] The student template workflow succeeds inside the released container
  without host Rust or ARM toolchain installation.
