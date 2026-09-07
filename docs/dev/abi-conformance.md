# ABI Conformance Matrix

> **Status: Informative.** This matrix records evidence for the normative
> platform contract but does not define that contract.

Requirement IDs link to the five version-1 specifications. Evidence prefixed
with "planned" does not exist yet. An implementation is not complete until its
required evidence is green in CI.

## Base Platform

| Requirement IDs | Contract area | Evidence |
|---|---|---|
| [`MINEMU-ABI1-SCOPE-001`-`002`](../platform/abi-v1.md#scope) | Specification boundary | Documentation review |
| [`MINEMU-ABI1-TERM-001`-`002`](../platform/abi-v1.md#terminology) | Range, address, and encoding terminology | Documentation review; `minemu-platform` address tests |
| [`MINEMU-ABI1-CPU-001`-`005`](../platform/abi-v1.md#cpu) | A32 Cortex-A9 profile and unsupported facilities | `minemu-image` ELF tests; `minemu-unicorn` A32/undefined tests; planned `minimum-tests` `a32-entry` |
| [`MINEMU-ABI1-RESET-001`-`003`](../platform/abi-v1.md#reset-state) | CPU/MMU/time/RAM/ROM reset | `minemu-core` reset tests; `minemu-runtime` reset and prefill tests; baseline guest test |
| [`MINEMU-ABI1-MAP-001`-`004`](../platform/abi-v1.md#physical-memory-map) | Physical ranges, permissions, and direct map | `minemu-platform` memory-map and direct-map tests; baseline guest test |
| [`MINEMU-ABI1-TIME-001`-`005`](../platform/abi-v1.md#virtual-time) | Tick costs, processing boundary, IRQ order, and limit | `minemu-core` virtual-time tests; `minemu-unicorn` exact tick-deadline tests; `minemu-runtime` scheduled-boundary tests |

## Boot And Images

| Requirement IDs | Contract area | Evidence |
|---|---|---|
| [`MINEMU-BOOT1-ADDR-001`-`003`](../platform/boot-v1.md#fixed-locations) | Boot workspace, info, and bootstrap locations | `minemu-platform` constants/range tests; `minemu-image` overlap/bootstrap tests |
| [`MINEMU-BOOT1-HOST-001`-`003`](../platform/boot-v1.md#host-preconditions) | Host validation and ROM placement | `minemu-cli` argument tests; `minemu-image` parser tests; template firmware check |
| [`MINEMU-BOOT1-LOAD-001`-`002`](../platform/boot-v1.md#reset-firmware-sequence) | Segment copy, BSS clear, and handoff order | template firmware comparison; baseline guest test |
| [`MINEMU-BOOT1-HANDOFF-001`-`004`](../platform/boot-v1.md#bootstrap-handoff) | Future boot-info VA, MMU/vector setup, and entry | baseline guest test; `minimum-tests/headless/mmu/test.toml`; template linker/firmware checks |
| [`MINEMU-BOOT1-INFO-001`-`003`](../platform/boot-v1.md#boot-info-record) | Boot-info wire record and direct-map alias | `minemu-platform` boot-info encode/decode tests; baseline guest test |
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
| [`MINEMU-EXC1-ENTRY-001`-`004`](../platform/exceptions-and-mmu-v1.md#machine-exception-entry) | Vector offsets, modes, CPSR/SPSR, LR, IRQ, and ticks | `minemu-unicorn` exception-entry/delivery tests; `minimum-tests/headless/exceptions/test.toml` |
| [`MINEMU-MMU1-CP15-001`-`004`](../platform/exceptions-and-mmu-v1.md#supported-cp15-interface) | CP15 subset, privilege, alignment, and TLBIALL ordering | `minemu-platform` CP15 tests; `minemu-unicorn` CP15/condition/TLB tests; `minimum-tests/headless/ttbr-switch/test.toml` release gate |
| [`MINEMU-MMU1-WALK-001`-`003`](../platform/exceptions-and-mmu-v1.md#translation) | Walk arithmetic and PDE/PTE encoding | `minemu-platform` MMU entry tests; `minemu-core` walker tests |
| [`MINEMU-MMU1-WALK-004`-`007`](../platform/exceptions-and-mmu-v1.md#translation) | Targets, permissions, A/D bits, and cached behavior | `minemu-platform` permission tests; `minemu-core` MMU tests; `minimum-tests/headless/mmu/test.toml` |
| [`MINEMU-MMU1-FAULT-001`-`005`](../platform/exceptions-and-mmu-v1.md#faults) | Fault causes, DFSR/DFAR, abort class, and retention | `minemu-platform` fault-status tests; `minemu-unicorn` abort/fault-register tests; `minimum-tests/headless/exceptions/test.toml` |

## Devices

| Requirement IDs | Contract area | Evidence |
|---|---|---|
| [`MINEMU-DEV1-MMIO-001`-`002`](../platform/devices-v1.md#common-mmio-rules) | MMIO width, alignment, direction, offsets, and values | `minemu-platform` MMIO decode tests; `minemu-core` bus tests; planned `minimum-tests` `mmio-invalid` |
| [`MINEMU-DEV1-RESET-001`-`002`](../platform/devices-v1.md#reset-state) | Device reset and media flush boundary | `minemu-core` reset tests; `minemu-runtime` reset-flush test |
| [`MINEMU-DEV1-INTC-001`-`005`](../platform/devices-v1.md#interrupt-controller) | Levels, priority, claim retention, and EOI | `minemu-core` interrupt tests; `minemu-platform` priority tests; `minimum-tests/headless/interrupts/test.toml` |
| [`MINEMU-DEV1-TIMER-001`-`004`](../platform/devices-v1.md#systick) | Scheduling, rephase, periodic state, ACK, and IRQ | `minemu-core` SysTick tests; `minimum-tests/headless/interrupts/test.toml` |
| [`MINEMU-DEV1-BLOCK-001`-`007`](../platform/devices-v1.md#block-device) | Snapshot, deadline, DMA/LBA, busy, IRQ, and persistence | `minemu-core` block tests; `minemu-runtime` flush/reset tests; `minimum-tests/headless/block/test.toml` |
| [`MINEMU-DEV1-RNG-001`-`002`](../platform/devices-v1.md#deterministic-rng) | Seed/state policy and xorshift32 sequence | `minemu-core` RNG tests; `minimum-tests/headless/rng-trace/test.toml` |
| [`MINEMU-DEV1-UART-001`-`004`](../platform/devices-v1.md#uart0-and-uart1) | Polling, queues, readback, output, and level IRQ | `minemu-core` UART tests; `minemu-unicorn` UART IRQ test; `minimum-tests/headless/uart/test.toml`; `minimum-tests/headless/interrupts/test.toml` |
| [`MINEMU-DEV1-TRACE-001`-`002`](../platform/devices-v1.md#trace-device) | Retired timestamp and bounded events | `minemu-core` trace tests; `minimum-tests/headless/rng-trace/test.toml` |

## Non-ABI Checks

These checks protect product behavior but do not define guest hardware:

| Area | Evidence |
|---|---|
| Runtime lifecycle and bounded commands | `minemu-runtime` service and snapshot tests |
| CLI and manifest validation | `minemu-cli` parser and integration tests |
| Supplied trap frame, stacks, and IRQ dispatcher | template build; exceptions/interrupts guest tests |
| Student template build | `just template`; planned development-container smoke test |
| Release assets and OCI architectures | Release CI smoke tests |

The TTBR0-switch plus TLBIALL test remains a hard release gate for process and
context-switch coursework.
