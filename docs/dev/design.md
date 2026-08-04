# minemu Design

## Goals

`minemu` is a Rust platform for a 14-week operating-systems course. It lets
students build a small ARM kernel incrementally, beginning with console I/O and
interrupts and ending with paging and a filesystem.

The platform favors a small, specified machine over realistic but complex
hardware. It is cross-platform for students, deterministic for grading, and
observable through a TUI that supports print-style debugging.

## Key Decisions

### Unicorn-Based Platform

Unicorn executes ARM instructions. `minemu` supplies the system-emulation
layer: physical memory, ROM, MMIO devices, timer, interrupt controller,
exception adapter, MMU, image loading, and observability.

QEMU is intentionally not the primary backend. Its real GIC, PL011, virtio,
and ARM platform conventions would consume course time without supporting the
goal of simplified peripherals.

### Curated ARMv7-A

The CPU retains ARMv7-A concepts that matter to OS work: user versus privileged
modes, exception modes, banked registers, traps, IRQs, and protected virtual
memory. Features unrelated to the course are omitted.

The MMU is deliberately custom rather than ARM short-descriptor paging. CP15
is its privileged control interface, while the page-table format and
permissions remain platform-defined. This keeps multilevel paging and
replacement policy central without requiring ARM VMSA descriptor details.

### ROM, RAM, and Process Creation

The image packer produces an immutable system-ROM image from separately linked
kernel and user ELF files. A provided boot ROM copies the kernel into RAM.
User programs remain immutable ROM modules until the student kernel creates a
process by allocating RAM and copying a module into it.

This provides a realistic boot model and enables multiple independent instances
of a user program. It also avoids requiring a student ELF parser in the process
assignment.

### Deterministic Virtual Time

Timer and device behavior uses guest instruction counts rather than host time.
This makes scheduling behavior and automated tests repeatable. Interactive
input is naturally nondeterministic in arrival time, but it is accepted only at
bounded execution boundaries; scripted tests can inject it at exact virtual
times.

### TUI Is Observability, Not a Debugger

Students primarily use console output and explicit trace events to debug their
kernels. The TUI shows useful machine state, device events, faults, and page
translations, but does not try to become an instruction-step or source-level
debugger.

### GPL-Compatible Licensing

Unicorn and its Rust binding are GPL-2.0. The `minemu` repository is therefore
GPL-2.0-or-later compatible. Student kernel source remains separate student
work; it is not linked into the host emulator.

## Software Structure

The project should begin as a small Cargo workspace.

| Component | Responsibility |
|---|---|
| `minemu-platform` | Stable ABI definitions: memory map, MMIO, CP15, MMU, faults, and image records |
| `minemu-core` | Backend-neutral device state, MMU policy, exception plans, virtual time, and observability projections |
| `minemu-unicorn` | Unicorn CPU, memory, hook, and virtual-TLB adapter |
| `minemu-image` | Versioned system-ROM image parsing and packing |
| `minemu-runtime` | Emulator-thread lifecycle, bounded commands, disk flush, and status publication |
| `minemu` | CLI, headless test runner, and Ratatui/Crossterm TUI |
| `platform/` | Student headers, linker scripts, startup/vector assembly, runtime, and templates |

The primary Rust dependencies are `unicorn-engine`, `object`, `ratatui`,
`crossterm`, `clap`, `serde`, and an error library such as `thiserror`.


The `object` crate reads ELF files for `minemu image`. The image format emitted
by the tool is a stable course ABI and must be versioned independently from
Rust implementation details.

## Ownership and Concurrency

`Machine` is single-owner state. It owns Unicorn and all mutable guest state:

- CPU state and physical memory mappings.
- ROM and disk state.
- MMIO device state.
- Interrupt, timer, and MMU state.
- Event log and virtual instruction clock.

All Unicorn calls and callbacks execute on the emulator thread. This avoids
sharing the non-Rust FFI engine across threads and prevents callback lock or
borrow reentrancy problems.

The production runtime has two threads:

| Thread | Responsibility |
|---|---|
| Emulator | Owns `Machine`, drains commands, runs bounded instruction batches, handles callbacks, publishes snapshots |
| TUI | Owns the terminal, renders snapshots, collects input, sends commands |

The TUI sends commands over a channel. The emulator publishes immutable,
reference-counted `MachineSnapshot` values through a latest-value mechanism,
such as a bounded channel or `ArcSwap`. The snapshot contains copies of
renderable state only and never exposes Unicorn or mutable guest memory.

The core remains usable without threads. Unit tests can instantiate `Machine`
directly; integration tests may use the same command interface as the TUI.

## Emulator Loop

The emulator service follows this high-level loop:

1. Drain queued input and control commands.
2. If running, execute a bounded instruction batch.
3. Advance virtual time and complete due device work.
4. Select and deliver a pending enabled IRQ when permitted.
5. Record observable events.
6. Publish a new immutable snapshot when state changed or a refresh interval
   elapsed.

No callback renders the TUI, waits on a channel, or takes ownership of mutable
machine state from another thread.

## Tooling and Distribution

Student projects own their own Makefiles. `minemu` does runtime and packaging
work only:

```sh
minemu image --config image.toml --out build/system.rom
minemu run build/system.rom --disk build/disk.img
minemu test build/system.rom --script tests/smoke.toml
```

An OCI image provides the pinned Rust binary, cross-compiler, and supporting
tools through Docker or Podman. This gives Linux, macOS, and Windows students
the same compiler and emulator environment without depending on hardware
virtualization.

## Implementation Gate

Before building the full platform, validate these capabilities in Rust:

The feasibility spike completed these backend checks and is retained on the
`spike` branch. `TODO.md` defines the staged production rewrite and its
remaining backend, ABI, and student-platform gates.
