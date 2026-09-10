# ABI Conformance Matrix

> **Status: Informative.** This matrix records evidence for the normative
> platform contract but does not define that contract.

Requirement IDs link to the current Platform ABI v2 components and inherited
version-1 specifications. Evidence prefixed with "planned" does not exist yet.
An implementation is not complete until its required evidence is green in CI.

## Base Platform

| Requirement IDs | Contract area | Evidence |
|---|---|---|
| [`MINEMU-ABI2-SCOPE-001`-`003`](../platform/abi-v2.md) | Version composition and unchanged wire formats | Documentation review; ABI-v1 guest conformance |
| [`MINEMU-ABI1-SCOPE-001`-`002`](../platform/abi-v1.md#scope) | Specification boundary | Documentation review |
| [`MINEMU-ABI1-TERM-001`-`002`](../platform/abi-v1.md#terminology) | Range, address, and encoding terminology | Documentation review; `minemu-platform` address tests |
| [`MINEMU-ABI1-CPU-001`-`005`](../platform/abi-v1.md#cpu) | A32 Cortex-A9 profile and unsupported facilities | `minemu-image` ELF tests; `minemu-unicorn` A32/undefined tests; planned `minimum-tests` `a32-entry` |
| [`MINEMU-ABI1-RESET-001`-`003`](../platform/abi-v1.md#reset-state) | CPU/MMU/time/RAM/ROM reset | `minemu-core` reset tests; `minemu-runtime` reset and prefill tests; [baseline guest test](../../minimum-tests/image/minimum-test.toml) |
| [`MINEMU-ABI1-MAP-001`-`004`](../platform/abi-v1.md#physical-memory-map) | Physical ranges, permissions, and direct map | `minemu-platform` memory-map and direct-map tests; [baseline guest test](../../minimum-tests/image/minimum-test.toml) |
| [`MINEMU-ABI1-TIME-001`-`005`](../platform/abi-v1.md#virtual-time) | Tick costs, processing boundary, IRQ order, and limit | `minemu-core` virtual-time tests; `minemu-unicorn` exact tick-deadline tests; `minemu-runtime` scheduled-boundary tests |

## Boot And Images

| Requirement IDs | Contract area | Evidence |
|---|---|---|
| [`MINEMU-BOOT1-ADDR-001`-`003`](../platform/boot-v1.md#fixed-locations) | Boot workspace, info, and bootstrap locations | `minemu-platform` constants/range tests; `minemu-image` overlap/bootstrap tests |
| [`MINEMU-BOOT1-HOST-001`-`003`](../platform/boot-v1.md#host-preconditions) | Host validation and ROM placement | `minemu` CLI argument tests; `minemu-image` parser tests; template firmware check |
| [`MINEMU-BOOT1-LOAD-001`-`002`](../platform/boot-v1.md#reset-firmware-sequence) | Segment copy, BSS clear, and handoff order | template firmware comparison; [baseline guest test](../../minimum-tests/image/minimum-test.toml) |
| [`MINEMU-BOOT1-HANDOFF-001`-`004`](../platform/boot-v1.md#bootstrap-handoff) | Future boot-info VA, MMU/vector setup, and entry | [baseline](../../minimum-tests/image/minimum-test.toml); [MMU case](../../minimum-tests/headless/mmu/test.toml); template linker/firmware checks |
| [`MINEMU-BOOT1-INFO-001`-`003`](../platform/boot-v1.md#boot-info-record) | Boot-info wire record and direct-map alias | `minemu-platform` boot-info encode/decode tests; [baseline](../../minimum-tests/image/minimum-test.toml) |
| [`MINEMU-IMG1-ENC-001`-`003`](../platform/system-image-v1.md#encoding) | Little-endian fields, exact magic, and zero fill | `minemu-platform` wire-format tests; `minemu-image` deterministic build tests |
| [`MINEMU-IMG1-HDR-001`-`003`](../platform/system-image-v1.md#image-header) | Header constants, bounds, tables, and kernel entry | `minemu-platform` image-header tests; `minemu-image` parser/negative tests |
| [`MINEMU-IMG1-KSEG-001`-`005`](../platform/system-image-v1.md#kernel-segment-record) | Kernel sizes, mappings, overlap, and bootstrap | `minemu-platform` segment tests; `minemu-image` validation tests; planned `minimum-tests` `image-multisegment` |
| [`MINEMU-IMG1-MOD-001`-`003`](../platform/system-image-v1.md#module-record) | Names, tables, and module entry | `minemu-image` module parsing and negative tests |
| [`MINEMU-IMG1-MSEG-001`-`002`](../platform/system-image-v1.md#module-segment-record) | Module segment sizes, flags, and VA exclusion | `minemu-platform` module-segment tests; `minemu-image` validation tests |
| [`MINEMU-IMG1-VALID-001`-`003`](../platform/system-image-v1.md#whole-image-validity) | Occupied ranges and offset-driven consumption | `minemu-image` overlap/bounds/parser tests |
| [`MINEMU-IMG1-PROD-001`](../platform/system-image-v1.md#canonical-production) | Consumer independence from canonical layout | `minemu-image` parser tests |

## Exceptions And MMU

| Requirement IDs | Contract area | Evidence |
|---|---|---|
| [`MINEMU-EXC1-ENTRY-001`-`004`](../platform/exceptions-and-mmu-v1.md#machine-exception-entry) | Vector offsets, modes, CPSR/SPSR, LR, IRQ, and ticks | `minemu-unicorn` exception-entry/delivery tests; [exceptions case](../../minimum-tests/headless/exceptions/test.toml) |
| [`MINEMU-MMU1-CP15-001`-`004`](../platform/exceptions-and-mmu-v1.md#supported-cp15-interface) | CP15 subset, privilege, alignment, and TLBIALL ordering | `minemu-platform` CP15 tests; `minemu-unicorn` CP15/condition/TLB tests; [TTBR-switch case](../../minimum-tests/headless/ttbr-switch/test.toml) |
| [`MINEMU-MMU1-WALK-001`-`003`](../platform/exceptions-and-mmu-v1.md#translation) | Walk arithmetic and PDE/PTE encoding | `minemu-platform` MMU entry tests; `minemu-core` walker tests |
| [`MINEMU-MMU1-WALK-004`-`007`](../platform/exceptions-and-mmu-v1.md#translation) | Targets, permissions, A/D bits, and cached behavior | `minemu-platform` permission tests; `minemu-core` MMU tests; [MMU case](../../minimum-tests/headless/mmu/test.toml) |
| [`MINEMU-MMU1-FAULT-001`-`005`](../platform/exceptions-and-mmu-v1.md#faults) | Fault causes, DFSR/DFAR, abort class, and retention | `minemu-platform` fault-status tests; `minemu-unicorn` abort/fault-register tests; [exceptions case](../../minimum-tests/headless/exceptions/test.toml) |

## Devices

| Requirement IDs | Contract area | Evidence |
|---|---|---|
| [`MINEMU-DEV1-MMIO-001`-`002`](../platform/devices-v1.md#common-mmio-rules) | MMIO width, alignment, direction, offsets, and values | `minemu-platform` MMIO decode tests; `minemu-core` bus tests; planned `minimum-tests` `mmio-invalid` |
| [`MINEMU-DEV2-RESET-001`-`002`](../platform/devices-v2.md#reset-state) | Device reset and dual-media flush boundary | `minemu-core` reset tests; `minemu-runtime` dual-media reset-flush test |
| [`MINEMU-DEV1-INTC-001`-`005`](../platform/devices-v1.md#interrupt-controller) | Levels, priority, claim retention, and EOI | `minemu-core` interrupt tests; `minemu-platform` priority tests; [interrupts case](../../minimum-tests/headless/interrupts/test.toml) |
| [`MINEMU-DEV1-TIMER-001`-`004`](../platform/devices-v1.md#systick) | Scheduling, rephase, periodic state, ACK, and IRQ | `minemu-core` SysTick tests; [interrupts case](../../minimum-tests/headless/interrupts/test.toml) |
| [`MINEMU-DEV2-BLOCK-001`-`009`](../platform/devices-v2.md#block-device) | Unit snapshot, deadline, DMA/LBA, shared busy/IRQ, independent media, unit errors, and persistence | `minemu-platform` ABI tests; `minemu-core` block/reset tests; `minemu-unicorn` UNIT MMIO tests; `minemu-runtime` dual-flush/reset/path tests; [block case](../../minimum-tests/headless/block/test.toml); [unattached-unit case](../../minimum-tests/headless/block-unattached/test.toml) |
| [`MINEMU-DEV1-RNG-001`-`002`](../platform/devices-v1.md#deterministic-rng) | Seed/state policy and xorshift32 sequence | `minemu-core` RNG tests; [RNG/trace case](../../minimum-tests/headless/rng-trace/test.toml) |
| [`MINEMU-DEV1-UART-001`-`004`](../platform/devices-v1.md#uart0-and-uart1) | Polling, queues, readback, output, and level IRQ | `minemu-core` UART tests; `minemu-unicorn` UART IRQ test; [UART case](../../minimum-tests/headless/uart/test.toml); [interrupts case](../../minimum-tests/headless/interrupts/test.toml) |
| [`MINEMU-DEV1-TRACE-001`-`002`](../platform/devices-v1.md#trace-device) | Retired timestamp and bounded events | `minemu-core` trace tests; [RNG/trace case](../../minimum-tests/headless/rng-trace/test.toml) |

## Product And Runtime Verification

These checks protect host-product behavior. They may support ABI evidence but do
not create guest requirements.

| Area | Evidence |
|---|---|
| Runtime lifecycle and bounded commands | `minemu-runtime` service and snapshot tests |
| Reset/flush service integration | `minemu-runtime` reset-flush and shutdown tests |
| CLI/help and strict manifest validation | `minemu` parser, manifest, and runner tests |
| TUI input, layout, and inspection routing | `minemu` TUI unit tests |

## Template And Reference-Guest Verification

These checks validate supplied guest software without making its implementation
choices normative.

| Area | Evidence |
|---|---|
| Canonical Boot ROM reproducibility | `minimum-template/bootloader` and `minimum-tests/bootloader` comparison targets |
| Startup, banked stacks, trap frame, and IRQ dispatcher | `just template`; [exceptions](../../minimum-tests/headless/exceptions/test.toml) and [interrupts](../../minimum-tests/headless/interrupts/test.toml) cases |
| Starter kernel/user build and packaged boot | `just template`; [baseline](../../minimum-tests/image/minimum-test.toml) |

One guest test can provide both ABI evidence and reference-runtime evidence.
Passing a supplied trampoline or stack test does not make that exact guest
implementation part of the platform ABI.

## Distribution And Release Gates

| Area | Evidence/status |
|---|---|
| Full source, template, and conformance gate | `just ci` |
| Assignment 1 development image | Published and verified for `linux/amd64` and `linux/arm64` at the digest recorded in [release readiness](release-readiness.md) |
| Current ABI-v2 development image | Manual native-platform Buildx workflow; publication and published-image smoke verification pending |
| Process/context-switch coursework gate | [TTBR-switch plus TLBIALL](../../minimum-tests/headless/ttbr-switch/test.toml) |

The TTBR0-switch plus TLBIALL test remains a hard release gate for process and
context-switch coursework.
