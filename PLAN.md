# Dual Block-Device ABI Plan

## Goal

Extend `minemu` from one guest-visible block device to two symmetric devices so
the `minimum` teaching OS can use independent media for persistent files and
virtual-memory swap:

- BLOCK0 is the general-purpose/filesystem device.
- BLOCK1 is the dedicated swap device in the assignment environment.

Both devices expose the existing block-register interface and behavior. The
change must preserve all existing BLOCK0 addresses, register offsets, source
IDs, CLI behavior, and guest aliases while adding independent BLOCK1 state,
interrupts, media attachment, persistence, inspection, and tests.

This plan assumes ABI v1 has not been released as an immutable external
contract. The implementation therefore amends the current v1 documents while
preserving existing guest behavior. If v1 has external frozen consumers before
implementation begins, stop and publish the machine/device change as v2
instead. Do not bump the system-image or boot-info wire-format version: those
formats do not change.

## ABI Decisions

### Device Map

| Device | Base PA | Size | IRQ source | Reset priority |
|---|---:|---:|---:|---:|
| BLOCK0 | `0x1000_2000` | 4 KiB | `3` | `128` |
| BLOCK1 | `0x1000_6000` | 4 KiB | `4` | `128` |

BLOCK1 occupies the first currently unused MMIO page. Existing RNG, UART, and
trace addresses do not move. The Unicorn backend already maps the complete
`0x1000_0000..0x1001_0000` MMIO window, and the supplied bootstrap already
identity-maps that window, so no physical mapping expansion is required.

Both devices use the existing register layout:

| Offset | Register |
|---:|---|
| `0x00` | `COMMAND` |
| `0x04` | `LBA` |
| `0x08` | `SECTOR_COUNT` |
| `0x0c` | `PADDR` |
| `0x10` | `STATUS` |
| `0x14` | `ERROR` |
| `0x18` | `ACK` |
| `0x1c` | `CONTROL` |

Sector size, command values, status bits, error values, 32-tick completion
latency, DMA rules, ACK behavior, and write-back persistence semantics remain
identical.

### Interrupt Controller

- Append BLOCK1 priority at interrupt-controller offset `0x20`.
- Expand the valid source and enable mask from `0x0f` to `0x1f`.
- Preserve every existing register offset.
- Equal-priority arbitration continues to choose the lower source ID, so BLOCK0
  wins a tie with BLOCK1.
- BLOCK0 and BLOCK1 interrupt levels assert and deassert independently.
- `CLAIM` and `EOI` accept source IDs `3` and `4` under the existing rules.

### Determinism

If both block requests complete at the same virtual tick, process BLOCK0 before
BLOCK1. This order is guest-visible when transfers use overlapping RAM and must
be normative and tested.

### Media And Persistence

- Each device has an independent optional raw-media path and in-memory
  write-back copy.
- Reject attaching the same canonical host path to both devices.
- Flush both dirty media at every existing pause, reset, shutdown, and terminal
  failure boundary.
- Attempt both flushes even when one fails.
- Preserve dirty tracking independently after each failed flush.
- Report flush errors with the block-device identity. If both fail, return the
  BLOCK0 error first after both attempts have completed.
- Reset preserves both attached media while resetting both devices' command,
  status, error, deadline, and interrupt state.

### Compatibility Names

Guest C compatibility is concrete because the current template and tests use
the singular names. Preserve these aliases:

- `MINEMU_BLOCK_BASE` aliases `MINEMU_BLOCK0_BASE`.
- `MINEMU_BLOCK` aliases `MINEMU_BLOCK0`.
- `MINEMU_IRQ_BLOCK` aliases `MINEMU_IRQ_BLOCK0`.
- The existing BLOCK priority field remains at offset `0x1c` and represents
  BLOCK0.

New guest names are `MINEMU_BLOCK0`, `MINEMU_BLOCK1`,
`MINEMU_IRQ_BLOCK0`, and `MINEMU_IRQ_BLOCK1`.

Preserve `--block-media`/`-m` as a CLI alias for BLOCK0. Add explicit
`--block0-media` and `--block1-media` spellings. The legacy and explicit BLOCK0
spellings must not be accepted together in one invocation.

Headless manifests preserve `block_media` as a BLOCK0 alias and add
`block0_media` and `block1_media`. Existing block-media assertions default to
BLOCK0; add an explicit device selector for BLOCK1 assertions.

Internal Rust names should be corrected from the misleading singular `Dma` and
`Block` names to explicit block identities. Compatibility wrappers are not
required for workspace-internal APIs unless implementation discovers an actual
external consumer.

## Phase 1: Platform Definitions

- Add BLOCK0 and BLOCK1 identities to the physical memory map.
- Decode each MMIO page to a distinct target while reusing the shared block
  register enum.
- Add interrupt source `Block1 = 4` and explicit BLOCK0 naming for source `3`.
- Add the BLOCK1 priority register at `0x20`.
- Replace hard-coded source counts and masks with shared constants.
- Expand interrupt priority inspection from four to five entries.
- Replace singular block inspection with two independently identified
  snapshots.
- Update stable peripheral constant and MMIO decode tests.

Primary files:

- `crates/minemu-platform/src/mmap.rs`
- `crates/minemu-platform/src/mmio.rs`
- `crates/minemu-platform/src/peripherals/interrupt.rs`
- `crates/minemu-platform/src/peripherals/block.rs`
- `crates/minemu-platform/src/observability.rs`
- `crates/minemu-platform/tests/abi.rs`

## Phase 2: Core Machine

- Make each `BlockDevice` instance carry its interrupt source identity.
- Instantiate BLOCK0 and BLOCK1 with independent media, registers, active
  commands, deadlines, dirty sectors, and IRQ state.
- Route MMIO transactions by block identity.
- Advance both devices and impose BLOCK0-before-BLOCK1 ordering at equal
  deadlines.
- Expand interrupt-controller storage and claim arbitration to five sources.
- Preserve both media in machine reset clones.
- Include both block statuses and inspections in machine snapshots.
- Qualify block media and flush errors with device identity.

Primary files:

- `crates/minemu-core/src/block.rs`
- `crates/minemu-core/src/bus.rs`
- `crates/minemu-core/src/interrupt.rs`
- `crates/minemu-core/src/machine.rs`
- `crates/minemu-core/src/error.rs`

Required focused tests:

- Register state is independent between BLOCK0 and BLOCK1.
- Both devices can have active requests concurrently.
- Same-tick completion follows device-index order.
- Source IDs 3 and 4 assert, claim, ACK, and EOI independently.
- Equal priorities choose source 3 first.
- Both in-memory media survive reset.

## Phase 3: Runtime Persistence

- Represent two optional media paths in runtime configuration.
- Attach and initialize each configured medium independently.
- Flush both devices at every existing lifecycle boundary.
- Attempt the second flush after the first fails.
- Preserve each device's dirty state after its own flush failure.
- Reject duplicate canonical paths.
- Extend runtime status and inspection responses without duplicating control
  flow for each device.

Primary files:

- `crates/minemu-runtime/src/types.rs`
- `crates/minemu-runtime/src/service.rs`
- `crates/minemu-runtime/src/tests.rs`

Required focused tests:

- Both media flush on pause, reset, shutdown, and terminal failure.
- A failure on either device identifies that device.
- One failed flush does not prevent the other from being attempted.
- Reset reconstructs both attachments and clears transient device state.
- Duplicate host paths are rejected.

## Phase 4: CLI And Headless Tests

- Add explicit BLOCK0 and BLOCK1 run options while retaining the legacy BLOCK0
  spelling.
- Reject conflicting legacy and explicit BLOCK0 arguments.
- Add strict headless-manifest fields for both media.
- Keep old manifests valid through the BLOCK0 alias.
- Add a device selector to persisted-media assertions, defaulting to BLOCK0.
- Resolve and validate both paths relative to the manifest.
- Ensure diagnostics identify the selected device and path.

Primary files:

- `crates/minemu/src/main.rs`
- `crates/minemu/src/lib.rs`
- `crates/minemu/src/runner.rs`

Required focused tests:

- Legacy CLI and TOML spellings still select BLOCK0.
- Explicit BLOCK0 and BLOCK1 paths can be supplied together.
- Conflicting BLOCK0 spellings fail clearly.
- Assertions read the requested device's persisted medium.
- Unknown fields and invalid device selectors remain rejected.

## Phase 5: TUI And Backend Integration

- Pass both media paths through TUI startup and runtime construction.
- Render BLOCK0 and BLOCK1 state separately in peripheral inspection.
- Show all five interrupt priorities.
- Label claimed sources 3 and 4 distinctly.
- Add BLOCK1 formatting and source-name tests.
- Add Unicorn integration tests for MMIO access at `0x1000_6000`, BLOCK1 IRQ
  delivery, and invalid access to a still-reserved neighboring MMIO page.
- Verify virtual-memory inspection rejects both block pages as devices.

Primary files:

- `crates/minemu/src/tui/mod.rs`
- `crates/minemu/src/tui/app.rs`
- `crates/minemu/src/tui/widgets/secondary.rs`
- `crates/minemu-unicorn/src/backend.rs`

## Phase 6: Guest Headers And Examples

Apply synchronized ABI definitions to `minimum-template`, `minimum-tests`, and
the active `minimum-rtsang` course repository:

- Add `MINEMU_BLOCK0_BASE`, `MINEMU_BLOCK1_BASE`, and compatibility aliases.
- Add BLOCK0/BLOCK1 MMIO pointers and IRQ source constants.
- Change `MINEMU_IRQ_ENABLE_MASK` to `0x1f`.
- Add BLOCK1 reset-priority constants.
- Append `priority_block1` to `struct minemu_interrupt_regs`.
- Update its size assertion from 32 to 36 bytes while preserving old field
  offsets.
- Keep one shared `struct minemu_block_regs`.
- Update IRQ and MMIO examples to identify both devices where relevant.

The supplied course block layer should provide one common synchronous API over
both devices. Assignment 6 uses BLOCK1 for swap; Assignment 7 uses BLOCK0 for
the filesystem. Students do not implement the low-level device driver.

## Phase 7: Conformance

Extend the block fixture to attach two disposable media images and verify:

- Independent read, write, busy, error, ACK, and control state.
- Independent persistence and reset behavior.
- BLOCK0 and BLOCK1 DMA success and failure paths.
- Source IDs 3 and 4 and the appended priority register.
- Equal-priority arbitration.
- Concurrent requests and same-tick completion ordering.
- Persisted-media assertions against both host files.

Extend interrupt conformance to prove enable bit 4, priority offset `0x20`,
claim/EOI source 4, and unchanged ordering for existing sources.

Primary areas:

- `minimum-tests/headless/block/`
- `minimum-tests/headless/interrupts/`
- `minimum-tests/docs/conformance-authoring.md`
- `minimum-tests/README.md`

## Phase 8: Documentation

Update all normative and informative references from a singular block device to
BLOCK0/BLOCK1 where cardinality matters:

- Amend `docs/platform/abi-v1.md` with the BLOCK1 page.
- Amend `docs/platform/devices-v1.md` with both instances, source 4, priority
  offset `0x20`, mask `0x1f`, and deterministic equal-tick order.
- Update `docs/user/cli.md` for both media flags and compatibility spelling.
- Update `docs/dev/headless-testing.md` for dual media and assertion selection.
- Update architecture and persistence descriptions.
- Update the ABI conformance matrix with all new evidence.
- Update guest and assignment documentation to reserve BLOCK1 for swap and
  BLOCK0 for files in the course environment.

## Validation

Run verification in each repository after synchronizing the guest artifacts:

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

Do not run Docker automatically. Manually verify the TUI shows both devices and
that two distinct host media files can be attached, modified, paused, reset, and
shut down without cross-device state or persistence leakage.

## Completion Criteria

- Existing BLOCK0 guests, CLI invocations, and headless manifests retain their
  behavior.
- BLOCK1 has independent MMIO state, media, timing, IRQ, persistence, and
  inspection.
- No old MMIO address or interrupt-controller offset moves.
- Both simultaneous and same-tick block operations are deterministic.
- Both media are flushed at every documented lifecycle boundary.
- Guest headers agree across all three teaching/conformance repositories.
- Normative constants agree with Rust implementation and conformance fixtures.
- Assignment 6 can use BLOCK1 exclusively for swap through supplied code.
- Assignment 7 can use BLOCK0 exclusively for its inode filesystem.
- Full workspace, template, and conformance CI passes.
