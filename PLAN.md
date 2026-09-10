# Remaining Course Foundations And Release Plan

## Status

The multi-unit block controller, Platform ABI v2 documentation, host tooling,
guest headers, supplied synchronous block interface, and conformance fixtures
are implemented in `minemu` 0.2.0. This plan tracks only unfinished work.

`minimum-template` is the canonical source for supplied guest components.
Validate changes there, synchronize shared artifacts into `minimum-tests` and
`minimum-rtsang`, and then run each repository's verification.

## Bootstrap Workspace

Before Assignment 2 becomes authoritative:

- Replace duplicated bootstrap workspace literals with named linker symbols or
  shared assembly constants where practical.
- Export the complete reserved bootstrap range so allocators cannot return
  page-table or handoff frames.
- Synchronize startup and linker changes across all three guest repositories.

Primary files:

- `minimum-template/kernel/src/startup/boot.S`
- `minimum-template/kernel/linker/kernel.ld`
- Mirrored startup and linker files in `minimum-tests` and `minimum-rtsang`

## Loader And Address Spaces

Before Assignment 2 becomes authoritative, add a narrow instructor-owned eager
address-space foundation to `minimum-template` that can:

- Reserve bootstrap, kernel, and page-table physical ranges.
- Allocate and release contiguous page-aligned user backing ranges.
- Derive PTE permissions from the shared Readable, Writable, and Executable
  module flags and reject unsupported flag combinations.
- Create and destroy eager user address spaces while preserving supervisor
  kernel, vector, and MMIO mappings.
- Load packaged module segments, including BSS zeroing.
- Map and zero a fixed user stack.
- Validate user buffer ranges and required access.
- Activate an address space with TTBR0 followed by TLBIALL.
- Eagerly clone a single-threaded process address space for Assignment 3
  `fork`.

Keep process tables, scheduling, syscall policy, `fork` return semantics, and
page-replacement policy student-owned. Add focused tests or reference examples,
then synchronize the validated foundation into the course repositories.

## Block Persistence Follow-Up

- Add explicit runtime tests for unit-1 flush failure, two simultaneous flush
  failures, first-error ordering, and terminal-failure persistence.
- Record one manual TUI exercise attaching, modifying, pausing, resetting, and
  shutting down two distinct media files.

## Release Engineering

- Publish native `linux/amd64` and `linux/arm64` images containing `minemu`
  0.2.0 and Platform ABI v2.
- Assemble the multi-platform version tag and smoke-test its immutable digest.
- Maintain a representative throughput floor of 100,000 strict instructions
  per second.

The published Assignment 1 image remains an immutable historical `minemu` 0.1.0
and Platform ABI v1 artifact. Do not retag that digest as the ABI-v2 image.

## Validation

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
make -C minimum-template clean all
make -C minimum-tests clean test
make -C minimum-rtsang clean all
just ci
git diff --check
```
