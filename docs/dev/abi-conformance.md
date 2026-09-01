# ABI Conformance Matrix

This matrix maps the normative rules in `emulator.md` to their required
evidence. Rust tests validate platform, core, backend, image, and runtime
behavior. A template smoke test builds the student platform, and a
`minimum-tests` headless test runs a focused packed system-ROM image through
`minemu test`.

Evidence prefixed with "planned" does not exist yet. Other entries name current
test suites or files. Planned `minimum-tests` headless scenario names are stable
work items; their final manifests belong under `minimum-tests/image/`.

| ID | ABI rule | Required evidence |
|---|---|---|
| CPU-01 | A32-only Cortex-A9 execution and unsupported Thumb entry rejection | `minemu-image` ELF tests; `minemu-unicorn` A32 execution tests; planned `minimum-tests` headless `a32-entry` |
| MAP-01 | Physical RAM, ROM, MMIO, and reserved ranges | `minemu-platform` range tests |
| MAP-02 | Higher-half direct map and bootstrap addresses | `minemu-platform` direct-map tests; `minimum-tests/image/minimum-test.toml` |
| BOOT-01 | Reset at Boot ROM, guest segment copy/BSS clear, boot info, and physical handoff | `minemu-core` reset-preservation tests; minimum-template firmware check; `minimum-tests/image/minimum-test.toml` |
| IMG-01 | Header, kernel-segment, module, and boot-info binary layouts | `minemu-platform` encode/decode tests |
| IMG-02 | Packer rejects malformed image inputs and unsupported ELF features | `minemu-image` negative tests |
| IMG-03 | Multi-segment kernel and fixed-address user modules boot | planned `minimum-tests` headless `image-multisegment` |
| EXC-01 | Vector offsets, CPSR/SPSR, banked LR, and return rules | `minemu-unicorn` exception-entry tests; planned `minimum-tests` headless `exceptions` |
| EXC-02 | Undefined, SVC, prefetch abort, data abort, and IRQ delivery | `minemu-unicorn` exception-delivery tests; planned `minimum-tests` headless `exceptions` subcases |
| EXC-03 | Banked-stack initialization, normalized frame, no nested IRQs, and claim/ACK/EOI sequencing | planned `minimum-tests` headless reference-example tests |
| CP15-01 | CP15 condition and privilege semantics | `minemu-platform` CP15 validation tests; `minemu-unicorn` condition/privilege tests; planned `minimum-tests` headless `cp15-privilege` |
| CP15-02 | TTBR0, SCTLR.M, VBAR, DFSR, and DFAR behavior | `minemu-unicorn` CP15 boundary/fault-register tests; planned `minimum-tests` headless `cp15-control` |
| CP15-03 | TTBR switch plus TLBIALL has no stale translation | `minemu-unicorn` TTBR/TLBIALL test; planned `minimum-tests` headless `cp15-switch` release gate |
| MMU-01 | Directory/PTE layout, reserved bits, and RAM-only page tables | `minemu-platform` MMU tests |
| MMU-02 | Read, write, execute, user, ROM, and device permissions | `minemu-platform` permission tests; `minemu-core` user-device test; planned `minimum-tests` headless `mmu-permissions` |
| MMU-03 | Fault status causes and access metadata | `minemu-platform` fault-status tests; planned `minimum-tests` headless `mmu-fault-status` |
| MMU-04 | Accessed/Dirty updates, software bits, and TLBIALL after clearing | `minemu-platform` page-entry tests; `minemu-core` MMU-walk tests; planned `minimum-tests` headless `mmu-replacement-bits` |
| MMU-05 | Fetch/data page faults enter prefetch/data abort and retry correctly | `minemu-unicorn` abort-delivery tests; planned `minimum-tests` headless `page-faults` |
| TIME-01 | Normal instruction, trap, fault, and exception-entry tick costs | `minemu-core` virtual-time tests |
| TIME-02 | Exact timer and block deadlines across traps and faults | `minemu-core` scheduler/device tests; planned `minimum-tests` headless `device-deadlines` |
| MMIO-01 | Width, alignment, direction, reserved-bit, and undefined-offset faults | `minemu-platform` MMIO decoding tests; `minemu-core` bus tests; planned `minimum-tests` headless `mmio-invalid` |
| UART-01 | UART0 and UART1 polling, RX queues, TX output, and RX level IRQs | `minemu-core` UART tests; `minemu-unicorn` UART IRQ test; planned `minimum-tests` headless `uart` |
| TIMER-01 | SysTick enable, periodic scheduling, ACK, and IRQ masking | `minemu-core` SysTick tests; planned `minimum-tests` headless `systick` |
| IRQ-01 | Source priority, CLAIM retention, source ACK, and EOI | `minemu-core` interrupt-controller tests; planned `minimum-tests` headless `irq-controller` |
| IRQ-02 | Configurable priority, SysTick-over-UART defaults, and source-ID tie breaking | `minemu-platform` peripheral-constant tests; `minemu-core` interrupt-controller tests; planned `minimum-tests` headless `irq-priority` |
| BLOCK-01 | Physical DMA, deterministic read/write completion, and guest errors | `minemu-core` block tests; planned `minimum-tests` headless `block` |
| BLOCK-02 | Write-back media, pause/shutdown flush, and flush retry | `minemu-core` block flush-retry tests; planned runtime boundary tests; planned `minimum-tests` headless `block-writeback` |
| RNG-01 | Default seed, zero-seed policy, xorshift32 sequence, and reseed | `minemu-core` RNG tests; planned `minimum-tests` headless `rng` |
| RNG-02 | RNG supervisor-only mapping | `minemu-core` user-device test; planned `minimum-tests` headless `rng-user-fault` |
| TRACE-01 | Guest trace value, retired-instruction timestamp, bounded history, and supervisor-only access | `minemu-core` trace tests; planned `minimum-tests` headless `trace` |
| OBS-01 | Bounded events, material-change status, and requested inspection | `minemu-runtime` snapshot tests |
| RUNTIME-01 | Bounded commands, pause/reset/shutdown, and terminal error state | `minemu-runtime` service tests |
| TEMPLATE-01 | Kernel/user/system templates build in released container | current host `minimum-template` build; planned development-container smoke test |
| RELEASE-01 | Pinned toolchain, ABI assets, prebuilt CLI, and supported OCI architectures | release CI smoke test |

The implementation of a rule is not complete until its required evidence is
green in CI. CP15-03 is a hard gate for process and context-switch coursework.
