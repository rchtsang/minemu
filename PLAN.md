# Documentation Reorganization Plan

## Status

Proposed. No documentation moves or behavior changes should begin until the
blocking decisions in this document are resolved.

This plan replaces the completed TUI redesign plan. It defines how to separate
student-facing material, public tool documentation, normative platform
specifications, emulator development documentation, and conformance-author
guidance.

## How To Record Decisions

For each decision below:

1. Change exactly one option from `[ ]` to `[x]`.
2. Change `Status: open` to `Status: decided`.
3. Add constraints or rationale under `Notes` when needed.

Recommendations are proposals, not recorded decisions.

## Goals

- Give students a complete path from a fresh `minimum-template` checkout to a
  running image and useful TUI session.
- Make the versioned guest-visible platform contract easy for students,
  emulator developers, and conformance authors to find.
- Clearly distinguish machine behavior from supplied guest runtime behavior.
- Keep emulator implementation details and conformance fixtures out of student
  instructions.
- Make CLI and headless-test behavior complete and precise rather than relying
  on one example manifest.
- Remove duplicated and historical prose from active documentation.
- Keep each independently hosted repository useful when checked out alone.

## Non-Goals

- Do not redesign the platform ABI merely to improve document organization.
- Do not duplicate normative register or wire-format tables across repositories.
- Do not expose conformance-only helpers such as `RamPrefill` in student guides.
- Do not preserve old file locations as competing sources of truth.
- Do not treat `TODO.md` or historical plans as normative documentation.

## Audience Model

The documentation has four audiences:

| Audience | Primary needs |
|---|---|
| Students | Build and run the template, use the TUI, understand the platform contract, implement kernel/runtime work |
| Emulator users | Package images, run interactively or headlessly, attach media, understand output and errors |
| Emulator developers | Understand architecture, source ownership, runtime behavior, local workflows, and release gates |
| Conformance authors | Write focused guest tests, use the headless schema, construct fixtures, and maintain ABI evidence |

The normative platform specification is shared by all four audiences. It must
not be categorized as emulator-internal documentation.

## Proposed Documentation Tree

```text
README.md
docs/
  README.md
  platform/
    abi-v1.md
    boot-v1.md
    system-image-v1.md
    exceptions-and-mmu-v1.md
    devices-v1.md
  student/
    kernel-runtime.md
    template-memory-layout.md
    module-loading.md
    tui.md
  user/
    cli.md
  dev/
    architecture.md
    workflows.md
    headless-testing.md
    abi-conformance.md
    tui-architecture.md
    archive/
      feasibility-spike.md
      tui-visual-spec.md
```

Independent repository entry points:

```text
minimum-template/README.md
minimum-template/bootloader/README.md
minimum-tests/README.md
minimum-tests/docs/conformance-authoring.md
```

## Blocking Decisions

### D1. Canonical Ownership Of Student Documentation

Status: decided

Choose one:

- [ ] Keep all student documentation in `minimum-template`.
- [x] Keep normative platform documents in the parent repository and maintain
  a standalone quickstart plus links in `minimum-template`.
- [ ] Duplicate versioned platform documents into `minimum-template` releases.

Recommendation: keep the canonical, versioned platform specification in the
parent repository. Keep `minimum-template/README.md` self-contained for setup
and normal use, then link to stable platform-document URLs. This avoids two
copies of normative tables while allowing the template to stand alone.

Notes:


### D2. Normative Specification Granularity

Status: decided

Choose one:

- [ ] Keep one large `emulator.md`, but reorganize it by contract layer.
- [x] Split it into the five proposed versioned platform documents.
- [ ] Use a smaller three-document split: machine ABI, boot/image ABI, and
  supplied runtime ABI.

Recommendation: use the five-document split. The current file combines CPU,
boot, image wire formats, exceptions, MMU, devices, host persistence,
observability, and testing in roughly 500 lines. The proposed split follows
stable technical boundaries without creating one file per device.

Notes:


### D3. Normative Status Of The Supplied Template Layout

Status: decided

Choose one:

- [ ] Make the current page-table workspace, high-kernel load address,
  temporary identity map, MMIO map, and banked-stack layout part of platform
  ABI v1.
- [x] Treat them as supported `minimum-template` implementation details that
  students may replace while preserving the machine and boot ABIs.
- [ ] Freeze only selected addresses; list them in `Notes`.

Recommendation: treat them as supplied-template contracts, not machine ABI.
The required physical bootstrap entry, boot-info location, and higher-half
mapping relationship remain normative. The exact page-table storage and
temporary mapping policy belong in `student/template-memory-layout.md`.

Notes:


### D4. Reset As A Block-Media Flush Boundary

Status: decided

Choose one:

- [x] Reset flushes dirty block media before rebuilding machine state.
- [ ] Reset discards unflushed guest writes; only pause, shutdown, and terminal
  failure flush.
- [ ] Reset fails while media is dirty and requires an explicit pause first.

Current state: the runtime flushes on reset and `cli.md` says so, while
`emulator.md` and `TODO.md` omit reset from the boundary list.

Recommendation: include reset as a flush boundary. It matches current behavior
and avoids silently losing completed guest writes during a user-requested
reset.

Notes:
yes, reset should be a flush boundary.

### D5. RNG `SEED` Read Semantics

Status: decided

Choose one:

- [x] `SEED` retains and returns the last configured seed; `STATE` returns the
  evolving generator state.
- [ ] `SEED` and `STATE` both return the evolving state; document `SEED` as an
  alias with restart-on-write behavior.
- [ ] Make `SEED` write-only and retain `STATE` as the only state read.

Current state: documentation describes a retained configured seed, but the
implementation returns the current state from both registers.

Recommendation: retain the configured seed separately. This gives the two
registers distinct meanings and matches their current names.

Notes:


### D6. RAM Contents After Reset

Status: decided

Choose one:

- [x] RAM is guaranteed to be zero after reset, before firmware writes.
- [ ] RAM contents are unspecified after reset.
- [ ] Normal reset clears RAM, but this is tool behavior rather than guest ABI.

Recommendation: guarantee zeroed RAM for this deterministic teaching platform,
while still requiring firmware to clear ELF BSS. Tests may prefill RAM to prove
that the firmware performs its required copy and clear operations.

Notes:


### D7. Headless Tick Budget And Input Scheduling

Status: decided

Choose one:

- [ ] Preserve the current asynchronous runner and document `max_ticks` and
  `at_tick` as soft, earliest-observed thresholds.
- [x] Move budget enforcement and scheduled input into the emulator thread so
  they occur at exact virtual-time boundaries.
- [ ] Keep soft scheduling for general tests and add a separate exact event
  schedule for conformance tests.

Current state: the host polls published status, so execution can pass
`max_ticks` and UART input can arrive after `at_tick`. `instruction_batch = 1`
does not make host polling exact.

Recommendation: enforce test budgets and scripted inputs on the emulator
thread. Deterministic scheduling is a core reason for the headless test runner.
Until that exists, documentation must call the current values thresholds.

Notes:
max_ticks and at_ticks don't make sense unless they are deterministic, i'd
prefer to modify the source code to make it so rather than treat these as soft
thresholds.

### D8. Test Success And Lifecycle Semantics

Status: decided

Choose one:

- [ ] Preserve the current optional assertions and post-shutdown lifecycle
  result; document the limitations.
- [ ] Reject empty assertions, fail automatically on runtime failure, and
  remove unusable lifecycle values.
- [x] Capture execution state before shutdown separately from final shutdown
  state and allow assertions over both.

Recommendation: capture pre-shutdown execution state separately, fail runtime
errors by default, and require at least one positive behavioral oracle.
`lifecycle = "running"` should not remain accepted if only post-shutdown state
is exposed.

Notes:


### D9. Unknown Manifest Fields

Status: decided

Choose one:

- [x] Reject unknown fields in image manifests, test manifests, inputs,
  prefills, and assertions.
- [ ] Reject them only in test assertion structures.
- [ ] Continue accepting unknown fields and document forward-compatible
  parsing.

Current state: top-level tests, prefills, and assertions reject unknown fields,
but UART inputs and image manifests do not.

Recommendation: reject unknown fields everywhere. These are checked project
inputs, and a misspelled field should not silently pass.

Notes:


### D10. Canonical Headless-Test Documentation

Status: decided

Choose one:

- [x] Keep the complete schema in the parent repository and a conformance
  author guide in `minimum-tests`.
- [ ] Keep all headless testing documentation in `minimum-tests`.
- [ ] Keep general schema documentation in the parent and duplicate it in the
  test repository.

Recommendation: keep the canonical CLI/schema reference in
`docs/dev/headless-testing.md`. Keep case organization, guest trace-oracle
conventions, and suite-maintenance instructions in
`minimum-tests/docs/conformance-authoring.md`.

Notes:


### D11. Historical Documentation Policy

Status: decided

Choose one:

- [x] Delete completed plans and spike documents after durable decisions are
  incorporated into current documentation.
- [ ] Retain them under `docs/dev/archive/` with a prominent historical and
  nonnormative notice.
- [ ] Leave historical documents in their current locations.

Recommendation: archive the feasibility spike and visual TUI specification,
but delete completed execution plans after extracting durable decisions.

Notes:
we'll rely on version control to keep the contents in history.

### D12. Unreleased Development Container Instructions

Status: decided

Choose one:

- [ ] Remove all container instructions until a student image is published.
- [x] Keep a status section that says the released image is not yet available.
- [ ] Continue using a placeholder image name in command examples.

Recommendation: keep a short status section without unusable commands. Add the
real image reference and workflow only when release work is complete.

Notes:


## Resolved Contract Statements

- The parent repository owns the canonical platform contract. The template
  keeps a standalone quickstart and links to the canonical specification.
- The platform contract is split into five versioned documents covering the
  overview, boot, image format, exceptions/MMU, and devices.
- Template page-table storage, temporary mappings, and stack placement are
  supported reference-policy choices rather than platform ABI.
- Reset flushes dirty block media before machine state is rebuilt.
- RNG `SEED` reads return the configured seed, `STATE` reads return the evolving
  generator state, and `DATA` advances that state.
- Physical RAM is zero after reset and before firmware writes.
- The emulator thread enforces `max_ticks` and scheduled UART `at_tick`
  boundaries exactly by shortening execution batches at those boundaries.
- Headless assertions inspect a pre-shutdown execution snapshot. Execution and
  shutdown lifecycle expectations are distinct, empty assertion sets are
  invalid, and any runtime failure fails the test.
- Unknown fields are rejected at every level of image and test manifests.
- The parent repository owns the complete headless schema; `minimum-tests`
  owns the conformance-suite authoring guide.
- Completed plans and superseded historical documents are deleted after their
  durable decisions are incorporated into current documentation.
- Container documentation states that no released student image is currently
  available and does not publish placeholder commands.

## Content Boundaries

### Normative Platform Documents

`docs/platform/abi-v1.md` should define document status, normative language,
address conventions, CPU/reset behavior, physical memory, virtual time, and
links to the component specifications.

`docs/platform/boot-v1.md` should define reserved physical boot locations,
reset firmware actions, the physical bootstrap handoff, the future virtual
boot-info pointer, the required higher-half mapping, and boot-info fields.

`docs/platform/system-image-v1.md` should define exact byte order, magic bytes,
headers, kernel and module records, offsets, flags, validity constraints, and
versioning. Host parser implementation details should not appear here.

`docs/platform/exceptions-and-mmu-v1.md` should define machine exception entry,
CP15 operations, translation caching, TTBR0/TLBIALL sequencing, PDE/PTE formats,
permissions, Accessed/Dirty behavior, and fault registers.

`docs/platform/devices-v1.md` should define general MMIO rules and complete
register contracts for interrupt control, SysTick, block, RNG, UART0, UART1,
and the guest-visible trace write port. Each table should include reset values,
writable masks, side effects, errors, and interrupt behavior.

### Student Documents

`docs/student/kernel-runtime.md` should clearly label guest software behavior:
banked stacks, exact trap-frame offsets and alignment, dispatch IDs, C hooks,
replacement-frame selection, SVC/IRQ trampoline ownership, and the
claim/ACK/EOI sequence.

`docs/student/template-memory-layout.md` should describe the reference linker
layout, bootstrap code, page-table workspace, transition identity map, initial
direct map, initial MMIO mapping, and which parts students may replace.

`docs/student/module-loading.md` should explain how to locate module records,
map system ROM, allocate frames, copy initialized data, clear BSS, apply segment
permissions, create a user stack, and switch TTBR0 with TLBIALL.

`docs/student/tui.md` should contain views, controls, navigation, commands, and
basic diagnostic logging. Runtime request routing and Unicorn ownership belong
in `docs/dev/tui-architecture.md`.

### User CLI Document

`docs/user/cli.md` should document:

- Global options and path resolution.
- `minemu image` inputs, output behavior, and links to the image format.
- `minemu run` as TUI by default.
- `minemu run --headless` output and exact tick-boundary behavior.
- Boot ROM and block-media requirements.
- Exit status and error behavior.
- Links to TUI and headless-test documentation.

The full test-manifest schema should not be embedded in this document.

### Developer Documents

`docs/dev/architecture.md` should replace the current future-tense `design.md`
with the implemented crate boundaries, ownership model, backend adapter,
runtime lifecycle, inspection path, and persistence design.

`docs/dev/workflows.md` should distinguish `just test`, `just template`,
`just conformance`, and `just ci`, including submodule prerequisites and clean
behavior.

`docs/dev/headless-testing.md` should be the complete schema and execution
reference for `minemu test`, including defaults, validation, path resolution,
timing limitations, retained-history limits, output, and failure semantics.

`docs/dev/abi-conformance.md` should map stable requirement IDs from the
versioned specification to exact Rust and guest tests. Product/runtime/release
checks should be distinguishable from guest ABI conformance.

## Current File Migration

| Current file or section | Destination |
|---|---|
| `docs/dev/emulator.md` status, CPU, memory, and virtual time | `docs/platform/abi-v1.md` |
| `docs/dev/emulator.md` boot and boot info | `docs/platform/boot-v1.md` |
| `docs/dev/emulator.md` image records | `docs/platform/system-image-v1.md` |
| `docs/dev/emulator.md` exception entry, CP15, MMU, faults | `docs/platform/exceptions-and-mmu-v1.md` |
| `docs/dev/emulator.md` MMIO and device registers | `docs/platform/devices-v1.md` |
| `docs/dev/emulator.md` guest trap frames and stacks | `docs/student/kernel-runtime.md` |
| `docs/dev/emulator.md` supplied page-table placement | `docs/student/template-memory-layout.md` |
| `docs/dev/emulator.md` persistence and observability | `docs/dev/architecture.md` and `docs/user/cli.md` |
| `docs/dev/cli.md` image and run commands | `docs/user/cli.md` |
| `docs/dev/cli.md` test manifest | `docs/dev/headless-testing.md` |
| `docs/dev/tui.md` controls and views | `docs/student/tui.md` |
| `docs/dev/tui.md` snapshot internals | `docs/dev/tui-architecture.md` |
| `docs/dev/design.md` durable implemented architecture | `docs/dev/architecture.md` |
| `docs/dev/spike-results.md` | `docs/dev/archive/feasibility-spike.md` or deletion, per D11 |
| `tui-design.md` | `docs/dev/archive/tui-visual-spec.md` or deletion, per D11 |

After migration, remove the old files or replace them with short relocation
notices for one release. They must not remain parallel sources of truth.

## Execution Plan

### Phase 1. Resolve Contracts

- [x] Record decisions D1 through D12.
- [x] Convert each selected behavior into a concise normative statement.
- [x] Identify decisions that require source or test changes before the new
  documentation can truthfully describe them.
- [x] Reconcile reset flushing, RNG `SEED`, RAM reset, timing, lifecycle, and
  unknown-field behavior across source, tests, `TODO.md`, and specifications.

Exit criterion: no known source/document contradiction is being moved into the
new documentation tree.

### Phase 2. Add Navigation

- [x] Add a parent `README.md` explaining `minemu`, `minimum-template`, and
  `minimum-tests` and linking documentation by audience.
- [x] Add `docs/README.md` as the documentation index.
- [x] Use real relative Markdown links rather than bare code-form filenames.
- [x] Mark the versioned platform documents as normative and all other guides
  as informative.

Exit criterion: a new reader can identify the correct starting document without
knowing the repository layout.

### Phase 3. Establish Student Entry Points

- [ ] Rewrite `minimum-template/README.md` as a standalone quickstart.
- [ ] Make the build sequence explicitly run both `make` and `make image`, or
  change the Make workflow and document the selected behavior.
- [ ] Add the essential paused-TUI controls and a link to the full TUI guide.
- [ ] Remove the obsolete claim that `minimum-template/image/` owns boot tests.
- [ ] Replace the development-container placeholder according to D12.
- [ ] Keep bootloader maintenance instructions separate from normal student
  use.

Exit criterion: a student can build, package, start, interact with, inspect, and
quit the starter image from a fresh template checkout.

### Phase 4. Split And Tighten The Platform Specification

- [ ] Add explicit PA, VA, ROM-offset, MMIO-offset, LBA, and host-file-offset
  terminology.
- [ ] Use half-open ranges consistently.
- [ ] Separate reserved physical boot locations from virtual mappings and
  arithmetic translation constants.
- [ ] Document the boot handoff as a numbered sequence and register-state table.
- [ ] State that `r0 = 0xc0007000` is a future VA that cannot be dereferenced
  before the direct map exists.
- [ ] Separate machine exception entry from guest trap-frame construction.
- [ ] Add complete reset values and transition semantics to device tables.
- [ ] Clarify TTBR0 writes, cached translations, and required TLBIALL ordering.
- [ ] Give numeric magic values and exact serialized byte sequences.
- [ ] Assign stable requirement IDs suitable for conformance mapping.

Exit criterion: every normative statement is guest-observable, versioned,
unambiguous about address space, and attributable to either machine hardware or
the supplied guest runtime.

### Phase 5. Split CLI Use From Test Authoring

- [ ] Rewrite the generated CLI descriptions so `run` is TUI-first and
  `--headless` is explicit.
- [ ] Document that `--ticks` has no effect in TUI mode, or change the CLI to
  reject that combination.
- [ ] Document manifest-relative paths separately from process-relative CLI
  paths.
- [ ] Document buffered UART output, limits, and exit behavior.
- [ ] Create the complete headless-test schema reference.
- [ ] Document all assertion fields, defaults, exact comparison rules, and
  block-media post-shutdown assertions.
- [ ] Apply decisions D7 through D10 to timing, lifecycle, and validation prose.

Exit criterion: `docs/user/cli.md`, generated `--help`, and actual CLI behavior
agree, while conformance-only setup is absent from student instructions.

### Phase 6. Document Development And Conformance Workflows

- [ ] Rewrite `docs/dev/design.md` as present-tense architecture or replace it
  with `docs/dev/architecture.md`.
- [ ] Document parent Just recipes and their exact scope.
- [ ] Rewrite `minimum-tests/README.md` for conformance maintainers.
- [ ] State that `make test` runs the baseline and all focused cases.
- [ ] Add `minimum-tests/docs/conformance-authoring.md`.
- [ ] Document case anatomy, registration in `headless/Makefile`,
  `MINEMU_REQUIRE`, trace success/failure oracles, tick budgets, scripted input,
  and disposable block-media fixtures.
- [ ] Split guest ABI evidence from product/runtime/release checks in the
  conformance matrix.

Exit criterion: an emulator contributor can run CI-equivalent checks and add a
focused conformance case without consulting source to discover the workflow.

### Phase 7. Archive And Verify

- [ ] Apply D11 to historical plans, spike notes, and visual specifications.
- [ ] Remove stale references to old paths and `docs/dev/emulator.md`.
- [ ] Check all internal and cross-repository links.
- [ ] Verify every documented command from a clean build state.
- [ ] Compare all normative constants and tables with `minemu-platform` and
  student headers.
- [ ] Run `just ci`.
- [ ] Add Markdown link checking and command smoke tests to CI where practical.

Exit criterion: no active document is historical, duplicated, unreachable, or
contradicted by source and tests.

## Required Student Quickstart Result

The final `minimum-template/README.md` must make this sequence complete and
correct from a fresh checkout:

```sh
make
make image
minemu run image/build/minimum.img \
  --boot-rom bootloader/bootloader.bin
```

It must also explain that the TUI starts paused and name the essential controls:

| Input | Effect |
|---|---|
| Space, then `s` | Start or pause execution |
| `i` | Send input to the selected UART |
| `Esc` | Leave insert mode |
| Space, then `i` | Enter inspection and pause |
| `?` | Open help |
| `:q` | Shut down and quit |

## Required Headless Schema Result

The final schema reference must document these current fields or their decided
replacements:

```text
image
boot_rom
block_media
instruction_batch
max_ticks
inputs[].at_tick
inputs[].uart
inputs[].data
ram_prefill[].address
ram_prefill[].length
ram_prefill[].value
assert.uart0_contains
assert.uart1_contains
assert.ticks_at_least
assert.lifecycle
assert.mmu_enabled
assert.fault_status
assert.trace_values
assert.block_media[].offset
assert.block_media[].bytes
```

For every field, document type, requirement/default, validation, timing, path
resolution, comparison behavior, and failure output.

## Completion Criteria

- [ ] Every decision in this plan is recorded.
- [ ] The parent and `docs/` directories have audience-based entry points.
- [ ] `minimum-template` is sufficient for a student's first successful run.
- [ ] Normative platform behavior is versioned and separated from
  implementation details.
- [ ] Supplied guest runtime obligations are separated from machine exception
  behavior.
- [ ] CLI usage and headless-test authoring are separate documents.
- [ ] The headless manifest schema is complete.
- [ ] `minimum-tests` has a conformance-author workflow rather than a copied
  student README.
- [ ] No active documentation contains placeholders for unreleased artifacts.
- [ ] No active documentation uses stale future-tense implementation plans.
- [ ] All links and documented commands are verified.
- [ ] Normative requirements map to conformance evidence.
- [ ] `just ci` passes after the migration.
