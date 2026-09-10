# Architecture

> **Status: Informative.** This document describes the implemented host
> architecture. The versioned [platform specifications](../platform/abi-v1.md)
> define guest-visible behavior.

## Design Boundaries

`minemu` is a deterministic A32 teaching platform rather than a model of a
production SoC. Unicorn executes instructions; repository code owns boot/image
validation, the custom MMU and devices, virtual time, exception adaptation,
runtime lifecycle, persistence, inspection, CLI behavior, and the TUI.

The custom two-level MMU intentionally exposes paging concepts without using
ARM short-descriptor formats. The simple interrupt and device models likewise
retain operating-system concepts while avoiding production GIC, PL011, and
virtio complexity.

## Crate Structure

The workspace has six crates with an acyclic dependency direction:

```text
minemu-platform <- minemu-core <- minemu-unicorn <- minemu-runtime <- minemu
        ^                ^                                      ^
        |                +--------------------------------------+
        +----------- minemu-image ------------------------------+
```

| Crate | Responsibility |
|---|---|
| `minemu-platform` | Backend-independent ABI types, constants, wire records, MMIO decoding, CP15 operations, and validation |
| `minemu-core` | Physical-memory backing, custom MMU policy, peripherals, interrupts, virtual time, and bounded observable events |
| `minemu-image` | ELF ingestion, canonical image packing, image parsing/validation, and reference boot plans |
| `minemu-unicorn` | Concrete A32 CPU, physical mappings, hooks, virtual TLB, CP15 mediation, exception entry, and live CPU inspection |
| `minemu-runtime` | Dedicated emulator thread, lifecycle, bounded command/input queues, scheduling, inspection RPC, reset, and media persistence |
| `minemu` | CLI, strict manifests, bounded test runner, assertions, and Ratatui/Crossterm TUI |

`minemu-platform` has no Unicorn, filesystem, or host-runtime dependency.
`minemu-core` remains directly testable without an emulator thread. The top-level
`minemu` crate does not call Unicorn directly.

## State Ownership

`minemu_core::Machine` owns backend-independent guest state: `PhysicalMemory`,
the typed MMIO bus, custom MMU state, virtual ticks, and the bounded event queue.
It does not own CPU registers or the Unicorn engine.

`minemu_unicorn::UnicornBackend` owns Unicorn. Its callback data contains the
`Machine`, callback stop information, pending CP15 work, and deferred exception
state. Boot ROM and System ROM are copied into immutable Unicorn mappings. RAM
uses one stable allocation owned by `PhysicalMemory` and mapped into Unicorn, so
the CPU and core devices observe the same bytes.

`minemu_runtime::RuntimeHandle` starts a named emulator thread. Construction and
all subsequent access to Unicorn occur on that thread. Host callers hold only:

| Host-facing state | Synchronization |
|---|---|
| Control and inspection commands | Bounded synchronous channel |
| Lightweight `RuntimeStatus` | `Arc<Mutex<_>>` latest value |
| Interactive UART ingress | Per-port bounded queues under `Arc<Mutex<_>>` |
| Emulator thread completion | Mutex-protected join handle |

The interactive main thread owns terminal setup, input polling, rendering, and
the TUI application. No Unicorn callback renders UI, waits for a TUI receiver,
or transfers mutable guest memory to another thread.

## CPU And Memory Path

At startup, the backend configures a little-endian Cortex-A9-compatible A32
engine, maps ROM/RAM/MMIO physical ranges, and installs instruction, trap,
invalid-instruction, MMIO, and virtual-TLB hooks.

A translated fetch/read/write follows this path:

1. Unicorn requests a virtual-TLB entry.
2. The backend derives access type and privilege from the CPU state.
3. `minemu-core` performs identity translation or the custom two-level walk.
4. The core validates permissions/targets, records faults, and updates PTE
   Accessed/Dirty bits.
5. The backend returns an access-specific physical mapping or arranges the
   corresponding abort.

An MMIO read/write follows this path:

1. Unicorn's physical MMIO callback creates a typed transaction.
2. `minemu-platform` validates 32-bit alignment, direction, offset, and value.
3. `minemu-core::MmioBus` dispatches to the selected device.
4. Peripheral interrupt-level changes are drained into the interrupt controller.
5. Invalid transactions become a recorded device-access fault and Data Abort.

Supported CP15 instructions are decoded and privilege-checked by the adapter,
then applied to core MMU state. Unsupported or unprivileged operations enter the
Undefined vector. TLBIALL also flushes Unicorn's cached virtual translations.

## Time And Exceptions

The backend counts attempted/completed instructions and reports boundaries to
`Machine`. Core accounting advances deterministic ticks, services SysTick and
block deadlines, drains interrupt levels, and commits staged trace events.
Synchronous exceptions and failed accesses use explicit outcomes so their
different tick costs remain visible.

Exception entry is split between layers. Core produces a backend-neutral
exception plan and records the observable event. The Unicorn adapter updates
CPSR/SPSR, selects banked state, writes LR, and transfers PC to the VBAR-relative
vector. IRQ delivery occurs between instruction batches after priority claim.

## Runtime Service

Published lifecycle values are `starting`, `running`, `paused`, `stopping`,
`stopped`, and `failed`. The command interface supports pause, unbounded or
instruction-bounded resume, reset, shutdown, and inspection.

While running, the service repeatedly:

1. Drains control and inspection commands.
2. Delivers due scheduled UART input.
3. Stops at an execution deadline when reached.
4. Drains interactive UART ingress.
5. Clips the next instruction batch to instruction, input, and tick boundaries.
6. Runs Unicorn and handles the resulting stop reason.
7. Publishes lightweight status periodically or on lifecycle transitions.

Paused mode waits for commands without running guest instructions. TUI runtimes
are configured to start paused before tick 0; headless runtimes start running
with an exact deadline.

Commands use nonblocking sends. Queue-full and stopped-service conditions are
explicit errors rather than unbounded host memory growth.

## Inspection

The runtime does not continuously clone the whole machine. `RuntimeStatus`
contains lifecycle, compact machine status, the latest stop detail, and latest
error. Larger data uses request/response inspection:

1. A caller enqueues an inspection request with a one-item response channel.
2. The emulator thread performs it between execution batches.
3. Borrowed memory views are converted to owned bytes before crossing threads.
4. The result is sent nonblockingly; a dropped receiver cannot stall execution.

Core requests cover memory, MMU, peripheral, and event projections. Backend
requests cover authoritative CPU registers, physical/translated memory,
translation, execution bytes, and RAM search. TUI widgets poll outstanding
receivers and route completed responses to the requesting pane.

## Input And Persistence

Interactive UART ingress and core UART RX queues each retain at most 4,096 bytes
per port and discard oldest bytes on overflow. Scripted input bypasses the host
inbox and is delivered by the emulator thread at exact virtual-time boundaries.
Each UART retains the newest 8,192 transmitted bytes for inspection and
headless output.

Each of the two optional block media is copied from its host file into an
independent write-back unit. The shared controller serializes requests and
snapshots its staged unit selector when a command starts. Guest writes mark
sectors dirty only on the selected unit. A flush writes that unit's whole media
image, then clears its dirty tracking. Both units are attempted independently at flush boundaries including pause,
bounded-resume completion, execution deadline, reset, terminal backend failure,
and shutdown. Failed writes retain dirty state for retry and update the
guest-visible block error/interrupt state.

Reset first flushes both media, reconstructs core state from retained ROM and media,
reapplies configured test RAM writes, rebuilds Unicorn, and restores the reset
entry PC.

## Frontends And Tests

`minemu image` parses strict image manifests and produces validated versioned
System ROM images. Production kernel copying remains guest Boot ROM behavior;
the image crate's boot plan is a host-side reference used by tests.

`minemu run` starts the TUI by default. The TUI requests state through the same
runtime interface used by tests and supports console/event observation,
physical and virtual memory, A32 disassembly, translation, search, registers,
MMU, peripheral, interrupt, and fault views.

`minemu test` parses the strict schema documented in
[Headless testing](headless-testing.md), runs to an exact deadline, captures a
pre-shutdown execution snapshot, shuts down and flushes media, then evaluates
host assertions. Guest conformance programs are maintained separately in the
`minimum-tests` submodule.
