# Multi-Unit Block ABI And Course Foundation Plan

## Goal

Extend the existing guest-visible block controller from one attached medium to
two selectable media units so the `minimum` teaching OS can use independent
storage for persistent files and virtual-memory swap:

- Unit 0 is the general-purpose/filesystem disk.
- Unit 1 is the dedicated swap disk in the assignment environment.

The controller remains at its existing MMIO base and retains one interrupt
source, one register bank, and one active request. A new `UNIT` register selects
the medium for the next command. Requests to either unit serialize through the
single controller.

The change must preserve all existing register offsets, command behavior, IRQ
behavior, CLI usage, headless manifests, and guest names for unit 0.

Before the assignment documents become authoritative, this work also reconciles
the supplied template's documented bootstrap layout, corrects the module-flag
guide, and adds the instructor-owned virtual-memory and block-I/O foundations
required by the course progression. `minimum-template` is the canonical source
for those supplied guest components; synchronize them into `minimum-tests` and
the `minimum-rtsang` course repository after validation.

This plan assumes ABI v1 has not been released as an immutable external
contract. The implementation therefore amends the current v1 documents while
preserving existing guest behavior. If v1 has external frozen consumers before
implementation begins, stop and publish the device change as v2 instead. Do not
bump the system-image or boot-info wire-format version; those formats do not
change.

## ABI Decisions

### Controller And Units

The block controller remains at PA `0x1000_2000` with interrupt source ID `3`.
No MMIO region or interrupt-controller register moves, and no new interrupt
source is added.

| Unit | Course use |
|---:|---|
| `0` | General-purpose and filesystem media |
| `1` | Swap media |

The course convention does not change the machine ABI: both units have
identical raw-sector behavior and guest software may use them for other
purposes.

### Register Layout

Append one register to the existing block-controller layout:

| Offset | Register | Access | Value |
|---:|---|---|---|
| `0x00` | `COMMAND` | W | `1` media-to-RAM read, `2` RAM-to-media write |
| `0x04` | `LBA` | R/W | First 512-byte logical block |
| `0x08` | `SECTOR_COUNT` | R/W | Transfer length in sectors |
| `0x0c` | `PADDR` | R/W | First DMA PA |
| `0x10` | `STATUS` | R | Busy, complete, and error bits |
| `0x14` | `ERROR` | R | Completion error code |
| `0x18` | `ACK` | W | Exact value `1` clears complete and error |
| `0x1c` | `CONTROL` | R/W | Completion IRQ enable |
| `0x20` | `UNIT` | R/W | Medium selected for the next command |

Reset sets `UNIT` to zero.

A `COMMAND` write while idle snapshots `UNIT` together with `COMMAND`, `LBA`,
`SECTOR_COUNT`, and `PADDR`. Later writes to the staging registers, including
`UNIT`, do not change the active request.

Only one request may be active across both units. A command written while Busy
retains the existing Busy-error behavior regardless of the staged or active
unit.

Sector size, command values, status bits, 32-tick completion latency, DMA rules,
ACK behavior, control behavior, and write-back persistence semantics otherwise
remain unchanged.

### Unit Errors

Add block error code `7`, Invalid Unit. A command that snapshots a unit other
than `0` or `1` completes after the normal 32-tick latency with Complete, Error,
and Invalid Unit set. A valid unit without attached media completes with the
existing No Media error.

Keeping invalid-unit handling in the command state machine makes success and
failure timing identical and avoids turning ordinary device selection errors
into MMIO access faults.

### Interrupts And Serialization

- Both units share interrupt source ID `3` and the existing block priority.
- The interrupt controller remains unchanged with source mask `0x0f`.
- `STATUS`, `ERROR`, `ACK`, and `CONTROL` are controller-wide.
- Completion from either unit asserts the shared interrupt according to the
  existing `Complete AND irq_enable` rule.
- Software identifies the completed operation from its own serialized request
  state; no completion-unit register is needed because only one request can be
  active.
- A driver must serialize callers across both units until the active request is
  acknowledged.

Because there is only one active request, simultaneous or same-tick completion
ordering between units is not part of the ABI.

### Media And Persistence

- Each unit has an independent optional raw-media path and in-memory write-back
  copy.
- Dirty sectors are tracked per unit.
- Reject attaching the same canonical host path to both units.
- Flush both dirty media at every existing pause, reset, shutdown, and terminal
  failure boundary.
- Attempt both flushes even when one fails.
- Preserve dirty tracking independently after each failed flush.
- Report flush errors with the unit identity. If both fail, report unit 0 first
  after both attempts have completed.
- Reset preserves both attached media while resetting controller registers,
  active request, deadline, completion state, and IRQ state.

### Compatibility

Existing binaries never access offset `0x20`, so they continue selecting unit 0
after reset. Existing guest names remain unchanged:

- `MINEMU_BLOCK_BASE`
- `MINEMU_BLOCK`
- `MINEMU_IRQ_BLOCK`

Add these guest constants:

- `MINEMU_BLOCK_UNIT_FILESYSTEM = 0`
- `MINEMU_BLOCK_UNIT_SWAP = 1`
- `MINEMU_BLOCK_ERROR_INVALID_UNIT = 7`

The course-oriented names are aliases for numeric units, not separate hardware
types.

Preserve `--block-media`/`-m` as the CLI spelling for unit 0. Add explicit
`--block0-media` and `--block1-media` spellings. The legacy and explicit unit-0
spellings must not be accepted together in one invocation.

Headless manifests preserve `block_media` as a unit-0 alias and add
`block0_media` and `block1_media`. Existing block-media assertions default to
unit 0; add a unit selector for assertions against unit 1.

## Phase 0: Pre-Writing Course Corrections

Complete each correction before the first assignment that depends on it becomes
authoritative. Assignment 1 may release before the eager address-space and
multi-unit block foundations because it uses neither facility.

### Assignment 1 Release Gate

Before Assignment 1 goes live, the student-facing bootstrap layout must match
the supplied starter, and the Assignment 1 handout, kernel-runtime guide,
starter IRQ ownership, frozen platform/trap/IRQ headers, sparse public tests,
and development image must agree. Later user-mode, page-allocation, and storage
foundations do not block this release.

The bootstrap and module-format documentation corrections below are complete.
Replacing bootstrap literals with allocator-facing linker symbols and adding
the eager address-space implementation remain Assignment 2 prerequisites. The
multi-unit block foundation and all subsequent block-device phases remain
deferred for Assignments 6 and 7.

### Bootstrap Workspace

The three current guest repositories agree on this implemented bootstrap
workspace:

| Half-open PA range | Purpose |
|---|---|
| `[0x4001_0000, 0x4001_1000)` | Initial page directory |
| `[0x4001_1000, 0x4001_2000)` | Low-RAM identity-map table |
| `[0x4001_2000, 0x4002_2000)` | Sixteen RAM direct-map tables |
| `[0x4002_2000, 0x4002_3000)` | Supervisor MMIO table |

`docs/student/template-memory-layout.md` now records the implemented layout and
the full sixteen-table direct-map range.

Before Assignment 2, replace duplicated bootstrap workspace literals in
`minimum-template` with named linker symbols or shared assembly constants where
practical. Export the complete reserved range to the supplied memory-management
foundation so a student allocator cannot hand page-table frames to user
processes. Apply those bootstrap artifact changes to `minimum-tests` and
`minimum-rtsang`. This implementation work does not block Assignment 1.

Primary files:

- `docs/student/template-memory-layout.md`
- `minimum-template/kernel/src/startup/boot.S`
- `minimum-template/kernel/linker/kernel.ld`
- Mirrored startup and linker files in `minimum-tests` and `minimum-rtsang`

### Module Flags

`docs/student/module-format-and-loading.md` now states that serialized
module-segment flags are Readable, Writable, and Executable. There is no
serialized User flag. The kernel applies `MINEMU_PTE_USER` as address-space
policy when mapping a user module.

Before Assignment 2, ensure the supplied loader interfaces consume the shared
segment flag constants from `minemu/boot.h`, derive PTE permissions explicitly,
and reject unsupported flag combinations rather than treating module flags as
raw PTE bits.

Primary files:

- `docs/student/module-format-and-loading.md`
- `minimum-template/kernel/include/minemu/boot.h`
- New supplied loader/address-space implementation in `minimum-template`

### Supplied Eager Address-Space Foundation

Assignment 2 introduces user mode before students have studied page-table
implementation. Add a narrow instructor-owned foundation to `minimum-template`
that can:

- Reserve bootstrap, kernel, and page-table physical ranges.
- Allocate and release contiguous page-aligned user backing ranges.
- Create and destroy eager user address spaces while preserving supervisor
  kernel, vector, and MMIO mappings.
- Map packaged module segments with derived user permissions.
- Map and zero a fixed user stack.
- Validate user buffer ranges and required read/write access.
- Activate an address space using TTBR0 followed by TLBIALL.
- Eagerly clone a single-threaded process address space for Assignment 3
  `fork`.

Keep process tables, scheduling, syscall policy, `fork` return semantics, and
page-replacement policy student-owned. The supplied API should teach virtual
layout and protection without requiring students to construct PDEs and PTEs
before Assignment 6.

Add focused template tests or reference examples for segment loading, BSS
zeroing, user-stack mapping, permission derivation, user-range validation,
address-space switching, and eager cloning. Synchronize the validated
foundation into `minimum-rtsang`; conformance-specific copies or fixtures belong
in `minimum-tests`.

### Supplied Multi-Unit Block Foundation

`minimum-template` must become the canonical implementation of the
instructor-supplied synchronous block interface described in Phase 6. Add it
only after the parent ABI and runtime support two media units. The interface
must serialize requests across units and remain policy-neutral: course code
chooses unit 1 for swap and unit 0 for filesystems.

## Phase 1: Platform Definitions

- Add `UNIT` at block-controller offset `0x20`.
- Add unit constants and Invalid Unit error code `7`.
- Extend block register decoding and access validation.
- Extend block inspection with the staged unit and per-unit attachment/dirty
  state.
- Keep the physical map and interrupt definitions unchanged.
- Update stable peripheral constant and MMIO decode tests.

Primary files:

- `crates/minemu-platform/src/peripherals/block.rs`
- `crates/minemu-platform/src/mmio.rs`
- `crates/minemu-platform/src/observability.rs`
- `crates/minemu-platform/tests/abi.rs`

Required focused tests:

- Offset `0x20` decodes as the UNIT register.
- Offset `0x24` remains invalid.
- Reset inspection selects unit 0.
- Existing offsets and constants retain their values.

## Phase 2: Core Controller

- Store two independent optional media images and dirty-sector sets in the
  existing `BlockDevice`.
- Add a staged unit register to the controller.
- Snapshot the staged unit into each active request.
- Route completion against the snapshotted unit.
- Return Invalid Unit at the scheduled completion deadline.
- Return No Media for a valid unattached unit.
- Keep one active request and one completion/IRQ state for the controller.
- Expose per-unit attach, detach, clone, flush, and inspection operations.
- Preserve both media in machine reset clones.
- Qualify media and flush errors with unit identity.

Primary files:

- `crates/minemu-core/src/block.rs`
- `crates/minemu-core/src/bus.rs`
- `crates/minemu-core/src/machine.rs`
- `crates/minemu-core/src/error.rs`

Required focused tests:

- Reads and writes target the selected unit only.
- Changing UNIT while Busy does not redirect the active request.
- A second command while Busy is rejected across unit boundaries.
- Invalid units fail at the normal deadline with error code `7`.
- Valid unattached units report No Media.
- Dirty tracking and media bytes remain independent.
- Both in-memory media survive reset while UNIT returns to zero.

## Phase 3: Runtime Persistence

- Represent two optional media paths in runtime configuration.
- Attach and initialize each configured medium independently.
- Flush both units at every existing lifecycle boundary.
- Attempt unit 1 after a unit-0 flush failure.
- Preserve each unit's dirty state after its own flush failure.
- Reject duplicate canonical paths.
- Extend runtime status and inspection without duplicating the controller.

Primary files:

- `crates/minemu-runtime/src/types.rs`
- `crates/minemu-runtime/src/service.rs`
- `crates/minemu-runtime/src/tests.rs`

Required focused tests:

- Both media flush on pause, reset, shutdown, and terminal failure.
- A failure on either unit identifies that unit.
- One failed flush does not prevent the other from being attempted.
- Reset reconstructs both attachments and clears transient controller state.
- Duplicate host paths are rejected.

## Phase 4: CLI And Headless Tests

- Add explicit unit-0 and unit-1 run options while retaining the legacy unit-0
  spelling.
- Reject conflicting legacy and explicit unit-0 arguments.
- Add strict headless-manifest fields for both media.
- Keep old manifests valid through the unit-0 alias.
- Add a unit selector to persisted-media assertions, defaulting to unit 0.
- Resolve and validate both paths relative to the manifest.
- Ensure diagnostics identify the selected unit and path.

Primary files:

- `crates/minemu/src/main.rs`
- `crates/minemu/src/lib.rs`
- `crates/minemu/src/runner.rs`

Required focused tests:

- Legacy CLI and TOML spellings still select unit 0.
- Explicit unit-0 and unit-1 paths can be supplied together.
- Conflicting unit-0 spellings fail clearly.
- Assertions read the requested unit's persisted medium.
- Unknown fields and invalid assertion unit selectors remain rejected.

## Phase 5: TUI And Backend Integration

- Pass both media paths through TUI startup and runtime construction.
- Render the shared controller registers once.
- Render attachment and dirty-sector state for units 0 and 1 separately.
- Label the staged UNIT value and active request unit when Busy.
- Add formatting tests for both media states.
- Add Unicorn integration tests for UNIT MMIO read/write and command snapshot
  behavior.
- Verify inspection still treats the controller page as one device page.

Primary files:

- `crates/minemu/src/tui/mod.rs`
- `crates/minemu/src/tui/app.rs`
- `crates/minemu/src/tui/widgets/secondary.rs`
- `crates/minemu-unicorn/src/backend.rs`

## Phase 6: Guest Headers And Supplied Driver

Implement the canonical guest ABI definitions and driver in
`minimum-template`, validate them there, then synchronize the shared artifacts
into `minimum-tests` and the active `minimum-rtsang` course repository:

- Append `unit` to `struct minemu_block_regs` at offset `0x20`.
- Update its size assertion from 32 to 36 bytes while preserving every existing
  field offset.
- Add filesystem-unit, swap-unit, and Invalid Unit constants.
- Keep all existing block base, pointer, and IRQ names unchanged.
- Do not change the interrupt-controller struct or constants.

Add an instructor-supplied synchronous block layer that:

- Accepts a unit, LBA, sector count, and physical DMA address.
- Serializes all operations through the one controller.
- Waits for completion and validates controller status.
- Acknowledges completion before releasing the controller.
- Returns course-defined errors without hiding Invalid Unit, No Media, DMA, or
  LBA failures.

Assignment 6 uses unit 1 for swap. Assignment 7 uses unit 0 for the filesystem.
Students do not implement the low-level controller driver.

## Phase 7: Conformance

Extend the block fixture to attach two disposable media images and verify:

- Independent reads, writes, contents, and dirty tracking.
- UNIT readback and reset value.
- UNIT snapshot behavior while a command is active.
- Controller-wide Busy serialization across units.
- Invalid Unit and valid-unattached-unit errors at the scheduled deadline.
- Shared IRQ completion, ACK, and EOI behavior for both units.
- Independent persistence and reset behavior.
- Persisted-media assertions against both host files.

The interrupt-controller conformance fixture should remain unchanged except for
any terminology updates; no source, mask, priority, or register is added.

Primary areas:

- `minimum-tests/headless/block/`
- `minimum-tests/docs/conformance-authoring.md`
- `minimum-tests/README.md`

## Phase 8: Documentation

Update normative and informative block documentation where media cardinality or
the register layout matters:

- Keep `docs/platform/abi-v1.md` physical-map and interrupt sections unchanged.
- Amend `docs/platform/devices-v1.md` with UNIT offset `0x20`, two supported
  units, Invalid Unit error `7`, command snapshot behavior, and controller-wide
  serialization.
- Update `docs/user/cli.md` for two media paths and the compatibility spelling.
- Update `docs/dev/headless-testing.md` for dual media and assertion selection.
- Update architecture and persistence descriptions.
- Update the ABI conformance matrix with all new evidence.
- Update guest and assignment documentation to reserve unit 1 for swap and unit
  0 for files in the course environment.
- Verify the corrected bootstrap workspace and module-flag guidance remain
  aligned with the final `minimum-template` implementation.

## Validation

Run verification in each repository after synchronizing guest artifacts:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
make -C minimum-template clean all
make -C minimum-tests clean test
just ci
git diff --check
```

Do not run Docker automatically. Manually verify the TUI displays the shared
controller and both units, and that two distinct host media files can be
attached, modified, paused, reset, and shut down without cross-unit state or
persistence leakage.

## Completion Criteria

- Existing unit-0 guests, CLI invocations, and headless manifests retain their
  behavior.
- UNIT resets to zero and is snapshotted when a command begins.
- Units 0 and 1 have independent media contents and dirty tracking.
- The controller permits only one active request across both units.
- No MMIO address, existing register offset, or interrupt definition moves.
- Both media are flushed at every documented lifecycle boundary.
- `docs/student/template-memory-layout.md` matches the actual supplied
  bootstrap workspace and reserves all page-table frames.
- Module documentation and supplied loader code use Readable, Writable, and
  Executable segment flags; user access remains kernel mapping policy.
- `minimum-template` supplies the eager address-space helpers required by
  Assignments 2 and 3 without implementing student-owned process policy.
- `minimum-template` supplies one serialized synchronous block interface for
  both media units.
- Guest headers agree across all three teaching/conformance repositories.
- Normative constants agree with Rust implementation and conformance fixtures.
- Assignment 6 can use unit 1 exclusively for swap through supplied code.
- Assignment 7 can use unit 0 exclusively for its inode filesystem.
- Full workspace, template, and conformance CI passes.
