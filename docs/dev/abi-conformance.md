# ABI Conformance Matrix

This matrix maps the normative rules in `emulator.md` to their required
evidence. A Rust unit test validates backend-independent behavior, a fixture is
an A32 guest integration program under `fixtures/arm`, a template smoke test
builds and boots the released student platform, and a headless test runs a
packed system-ROM image through `minemu test`.

All entries are planned until their named evidence exists.

| ID | ABI rule | Required evidence |
|---|---|---|
| CPU-01 | A32-only Cortex-A9 execution and unsupported Thumb entry rejection | `minemu-image` ELF tests; `fixtures/arm/a32-entry` |
| MAP-01 | Physical RAM, ROM, MMIO, and reserved ranges | `minemu-platform` range tests |
| MAP-02 | Higher-half direct map and bootstrap addresses | `fixtures/arm/high-half-boot`; kernel template smoke test |
| IMG-01 | Header, kernel-segment, module, and boot-info binary layouts | `minemu-platform` encode/decode tests |
| IMG-02 | Packer rejects malformed image inputs and unsupported ELF features | `minemu-image` negative tests |
| IMG-03 | Multi-segment kernel and fixed-address user modules boot | headless `image-multisegment` test |
| EXC-01 | Vector offsets, CPSR/SPSR, banked LR, and return rules | `fixtures/arm/exceptions` |
| EXC-02 | Undefined, SVC, prefetch abort, data abort, and IRQ delivery | `fixtures/arm/exceptions` subcases |
| CP15-01 | CP15 condition and privilege semantics | `fixtures/arm/cp15-privilege` |
| CP15-02 | TTBR0, SCTLR.M, VBAR, DFSR, and DFAR behavior | `fixtures/arm/cp15-control` |
| CP15-03 | TTBR switch plus TLBIALL has no stale translation | `fixtures/arm/cp15-switch` release gate |
| MMU-01 | Directory/PTE layout, reserved bits, and RAM-only page tables | `minemu-platform` MMU tests |
| MMU-02 | Read, write, execute, user, ROM, and device permissions | `fixtures/arm/mmu-permissions` |
| MMU-03 | Fault status causes and access metadata | `fixtures/arm/mmu-fault-status` |
| TIME-01 | Normal instruction, trap, fault, and exception-entry tick costs | `minemu-core` virtual-time tests |
| TIME-02 | Exact timer and block deadlines across traps and faults | `fixtures/arm/device-deadlines` |
| MMIO-01 | Width, alignment, direction, reserved-bit, and undefined-offset faults | `minemu-core` bus tests; `fixtures/arm/mmio-invalid` |
| UART-01 | UART polling, RX queue, TX output, and RX level IRQ | `fixtures/arm/uart` |
| TIMER-01 | Timer enable, periodic scheduling, ACK, and IRQ masking | `fixtures/arm/timer` |
| IRQ-01 | Source priority, CLAIM retention, source ACK, and EOI | `fixtures/arm/irq-controller` |
| BLOCK-01 | Physical DMA, deterministic read/write completion, and guest errors | `fixtures/arm/block` |
| BLOCK-02 | Write-back media, pause/shutdown flush, and flush retry | `minemu-runtime` disk tests; headless `block-writeback` test |
| RNG-01 | Default seed, zero-seed policy, xorshift32 sequence, and reseed | `minemu-core` RNG tests; `fixtures/arm/rng` |
| RNG-02 | RNG supervisor-only mapping | `fixtures/arm/rng-user-fault` |
| OBS-01 | Bounded events, material-change status, and requested inspection | `minemu-runtime` snapshot tests |
| RUNTIME-01 | Bounded commands, pause/reset/shutdown, and terminal error state | `minemu-runtime` service tests |
| TEMPLATE-01 | Kernel/user/system templates build in released container | container template smoke test |
| RELEASE-01 | Pinned toolchain, ABI assets, prebuilt CLI, and supported OCI architectures | release CI smoke test |

The implementation of a rule is not complete until its required evidence is
green in CI. CP15-03 is a hard gate for process and context-switch coursework.
