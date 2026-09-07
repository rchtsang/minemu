# ABI Conformance Matrix

> **Status: Informative.** This matrix records evidence for the normative
> platform contract but does not define that contract.

This matrix maps the normative rules in the [Emulator ABI](emulator.md) to
their required evidence. Rust tests validate platform, core, backend, image,
and runtime behavior. A template smoke test builds the student platform, and
`minimum-tests` runs focused packed system-ROM images through `minemu test`.

Evidence prefixed with "planned" does not exist yet. Other entries name current
test suites or files.

| ID | ABI rule | Required evidence |
|---|---|---|
| CPU-01 | A32-only Cortex-A9 execution and unsupported Thumb entry rejection | `minemu-image` ELF tests; `minemu-unicorn` A32 execution tests; planned `minimum-tests` headless `a32-entry` |
| MAP-01 | Physical RAM, ROM, MMIO, and reserved ranges | `minemu-platform` range tests |
| MAP-02 | Higher-half direct map and bootstrap addresses | `minemu-platform` direct-map tests; `minimum-tests/image/minimum-test.toml` |
| BOOT-01 | Reset at Boot ROM, guest segment copy/BSS clear, boot info, and physical handoff | `minemu-core` reset-preservation tests; minimum-template firmware check; `minimum-tests/image/minimum-test.toml` |
| IMG-01 | Header, kernel-segment, module, and boot-info binary layouts | `minemu-platform` encode/decode tests |
| IMG-02 | Packer rejects malformed image inputs and unsupported ELF features | `minemu-image` negative tests |
| IMG-03 | Multi-segment kernel and fixed-address user modules boot | planned `minimum-tests` headless `image-multisegment` |
| EXC-01 | Vector offsets, CPSR/SPSR, banked LR, and return rules | `minemu-unicorn` exception-entry tests; `minimum-tests/headless/exceptions/test.toml` |
| EXC-02 | Undefined, SVC, prefetch abort, data abort, and IRQ delivery | `minemu-unicorn` exception-delivery tests; `minimum-tests/headless/exceptions/test.toml` |
| EXC-03 | Banked-stack initialization, normalized frame, no nested IRQs, and claim/ACK/EOI sequencing | `minimum-tests/headless/exceptions/test.toml`; `minimum-tests/headless/interrupts/test.toml` |
| CP15-01 | CP15 condition and privilege semantics | `minemu-platform` CP15 validation tests; `minemu-unicorn` condition/privilege tests; `minimum-tests/headless/exceptions/test.toml` |
| CP15-02 | TTBR0, SCTLR.M, VBAR, DFSR, and DFAR behavior | `minemu-unicorn` CP15 boundary/fault-register tests; baseline boot test; `minimum-tests/headless/exceptions/test.toml` |
| CP15-03 | TTBR switch plus TLBIALL has no stale translation | `minemu-unicorn` TTBR/TLBIALL test; `minimum-tests/headless/ttbr-switch/test.toml` release gate |
| MMU-01 | Directory/PTE layout, reserved bits, and RAM-only page tables | `minemu-platform` MMU tests |
| MMU-02 | Read, write, execute, user, ROM, and device permissions | `minemu-platform` permission tests; `minemu-core` user-device test; `minimum-tests/headless/mmu/test.toml`; `minimum-tests/headless/exceptions/test.toml` |
| MMU-03 | Fault status causes and access metadata | `minemu-platform` fault-status tests; `minimum-tests/headless/mmu/test.toml`; `minimum-tests/headless/exceptions/test.toml` |
| MMU-04 | Accessed/Dirty updates, software bits, and TLBIALL after clearing | `minemu-platform` page-entry tests; `minemu-core` MMU-walk tests; `minimum-tests/headless/mmu/test.toml` |
| MMU-05 | Fetch/data page faults enter prefetch/data abort and retry correctly | `minemu-unicorn` abort-delivery tests; `minimum-tests/headless/exceptions/test.toml` |
| TIME-01 | Normal instruction, trap, fault, and exception-entry tick costs | `minemu-core` virtual-time tests |
| TIME-02 | Exact timer and block deadlines across traps and faults | `minemu-core` scheduler/device tests; `minimum-tests/headless/interrupts/test.toml`; `minimum-tests/headless/block/test.toml` |
| TIME-03 | Exact headless budget and scheduled-UART boundaries, including multi-tick exception entry | `minemu-unicorn` tick-deadline test; `minemu-runtime` scheduled-boundary tests |
| MMIO-01 | Width, alignment, direction, reserved-bit, and undefined-offset faults | `minemu-platform` MMIO decoding tests; `minemu-core` bus tests; planned `minimum-tests` headless `mmio-invalid` |
| UART-01 | UART0 and UART1 polling, RX queues, TX output, and RX level IRQs | `minemu-core` UART tests; `minemu-unicorn` UART IRQ test; `minimum-tests/headless/uart/test.toml`; `minimum-tests/headless/interrupts/test.toml` |
| TIMER-01 | SysTick enable, periodic scheduling, ACK, and IRQ masking | `minemu-core` SysTick tests; `minimum-tests/headless/interrupts/test.toml` |
| IRQ-01 | Source priority, CLAIM retention, source ACK, and EOI | `minemu-core` interrupt-controller tests; `minimum-tests/headless/interrupts/test.toml` |
| IRQ-02 | Configurable priority, SysTick-over-UART defaults, and source-ID tie breaking | `minemu-platform` peripheral-constant tests; `minemu-core` interrupt-controller tests; `minimum-tests/headless/interrupts/test.toml` |
| BLOCK-01 | Physical DMA, deterministic read/write completion, and guest errors | `minemu-core` block tests; `minimum-tests/headless/block/test.toml` |
| BLOCK-02 | Write-back media, pause/reset/shutdown flush, and flush retry | `minemu-core` block flush-retry tests; `minemu-runtime` reset-flush test; `minimum-tests/headless/block/test.toml` |
| RNG-01 | Default seed, zero-seed policy, xorshift32 sequence, and reseed | `minemu-core` RNG tests; `minimum-tests/headless/rng-trace/test.toml` |
| RNG-02 | RNG supervisor-only mapping | `minemu-core` user-device test; `minimum-tests/headless/exceptions/test.toml` |
| TRACE-01 | Guest trace value, retired-instruction timestamp, bounded history, and supervisor-only access | `minemu-core` trace tests; `minimum-tests/headless/rng-trace/test.toml`; `minimum-tests/headless/exceptions/test.toml` |
| OBS-01 | Bounded events, material-change status, and requested inspection | `minemu-runtime` snapshot tests |
| RUNTIME-01 | Bounded commands, pause/reset/shutdown, and terminal error state | `minemu-runtime` service tests |
| TEMPLATE-01 | Kernel/user/system templates build in released container | current host `minimum-template` build; planned development-container smoke test |
| RELEASE-01 | Pinned toolchain, ABI assets, prebuilt CLI, and supported OCI architectures | release CI smoke test |

The implementation of a rule is not complete until its required evidence is
green in CI. CP15-03 is a hard gate for process and context-switch coursework.
