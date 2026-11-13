# Guest Conformance Suite

These guest programs and manifests check `minemu` against the
[platform specifications](../docs/platform/abi-v2.md). The
[headless-testing reference](../docs/dev/headless-testing.md) defines every
manifest field and comparison rule, and the
[ABI conformance matrix](../docs/dev/abi-conformance.md) maps requirements to
cases.

## Ownership

The suite is self-contained. It builds only from this directory:

| Path | Contents |
|---|---|
| `bootrom/` | Reference Boot ROM (Boot ABI v1) and its canonical `bootloader.bin` |
| `runtime/` | Kernel startup, vectors, linker script, headers, and helpers shared by cases |
| `case.mk` | Per-case build, image packaging, and `minemu test` rules |
| `baseline/`, `uart/`, … | One directory per case |

`runtime/` and `bootrom/` began as copies of the `minimum-template` starter but
are owned here and must **not** track it. They implement only what the platform
specifications require, so the student OS can be restructured freely without
breaking conformance. Change them only when the platform contract changes, and
never make a case depend on `minimum-template`.

## Suite Layers

The `baseline` case packages a kernel and one minimal user module. Its
RAM prefill proves that reset firmware replaces initialized data and clears BSS
instead of relying on reset-zero RAM. The kernel validates boot info and the
higher-half handoff before emitting success value `0x20000001`.

Focused cases isolate one contract area:

| Case | Primary evidence |
|---|---|
| `uart` | Polling, RX queues, TX output, and both ports |
| `interrupts` | Timer deadlines, source levels, priority, claim/EOI, and IRQ entry |
| `block` | Two-unit DMA, snapshotting, shared IRQ/Busy state, write-back media, and persistence |
| `block-unattached` | Scheduled No Media completion for a supported unattached unit |
| `rng-trace` | Deterministic RNG sequence and ordered trace events |
| `mmu` | PTE permissions, Accessed/Dirty bits, and translation faults |
| `exceptions` | SVC, undefined, aborts, IRQ state, CP15, and user/device protection |
| `ttbr-switch` | TTBR0 replacement and explicit TLBIALL behavior |

## Case Anatomy

Each `NAME/` directory contains:

| File | Purpose |
|---|---|
| `Makefile` | Sets `PROGRAM := NAME` and includes `../case.mk` |
| `main.c` | Guest checks, trace diagnostics, final success event, and fail-stop |
| Optional `*.S` | Exact instruction sequences or custom exception trampolines |
| `image.toml` | Packages `build/NAME.elf` as the case kernel |
| `test.toml` | Sets the deadline/setup and exact host assertions |

`case.mk` discovers every directory-local `.c` and `.S` file, links it with
`runtime/` and its linker script, and writes all generated files under the
case's `build/` directory. Shared startup establishes banked stacks,
bootstrap page tables, TTBR0, the MMU, VBAR, and the high-half transition before
calling `minemu_kernel_main`.

Default weak exception handlers enter `minemu_fail_stop()`. A case may override
the C dispatch hooks or provide trampolines when the exception transition itself
is under test.

## Create A Case

1. Create `NAME/`.
2. Add this `Makefile`:

   ```make
   PROGRAM := NAME
   include ../case.mk
   ```

3. Add `main.c` and any assembly sources.
4. Add this `image.toml`:

   ```toml
   kernel = "build/NAME.elf"
   ```

5. Add `test.toml` with `image`, `boot_rom`, a finite `max_ticks`, lifecycle and
   MMU expectations, and exact `trace_values`.
6. Emit a unique final success value, then call `minemu_fail_stop()`.
7. Add `NAME` to `CASES` in `Makefile`.
8. Implement a phony `prepare` target if the case needs disposable host state.
9. Run the case directly, then the whole suite.

Registration is mandatory. An unregistered directory can run directly but is
absent from aggregate image, test, and clean targets.

## Guest Assertions

Use `MINEMU_REQUIRE(condition, code)` from
`runtime/include/minemu/conformance.h`. It evaluates the condition once. On
failure it emits:

```text
0xf0000000 | (code & 0x0fffffff)
```

and enters `minemu_fail_stop()`. Only the low 28 code bits are meaningful. Keep
codes unique within a case so an unexpected trace identifies the failed guest
condition.

The fail-stop routine does not exit the emulator. It spins until the manifest's
virtual-time deadline pauses execution. Unexpected exceptions normally enter a
weak fail-stop handler and are detected because the expected success trace is
missing.

## Trace Oracle Convention

Current cases reserve these value classes:

| Range/prefix | Use |
|---|---|
| `0x2.......` | Success/progress values expected by the manifest |
| `0xe.......` | Case-specific contextual failure diagnostics |
| `0xf.......` | `MINEMU_REQUIRE` failures |

The baseline retains older `0xb007bad0`/`0xb007bad1` diagnostics. New focused
cases should use the shared convention.

`trace_values` compares the complete retained trace-value sequence exactly,
including order, duplicates, and length. It is not a contains check. Therefore:

- A `MINEMU_REQUIRE` failure adds an unexpected `0xf.......` value.
- A timeout or unhandled exception omits the final success value.
- Extra diagnostics or duplicate success values fail the host assertion.
- The success marker must be the final guest trace event.

Do not emit trace events after the expected success sequence. The host retains
the trace subset of a bounded 4,096-event global history, so cases should remain
small and purpose-specific.

## Tick Budgets

`max_ticks` is a hard observation window, not a guest exit time. Choose a value
large enough for firmware, bootstrap, and the case to finish on all supported
hosts. The value does not make a slow host fail because it is virtual time.

Most current focused cases use 500,000 ticks. `exceptions` uses 600,000 and
`interrupts` uses 900,000. The baseline uses 500,000. Keep a larger budget only
when the guest behavior requires more virtual progress.

Leave `instruction_batch` at its default. Since `minemu` 0.2.4, device
deadlines, trace stamps, IRQ delivery, scheduled input, and `max_ticks` are
exact at every instruction boundary regardless of batch size, so the setting
only affects throughput.

Focused manifests normally assert:

```toml
[assert]
execution_lifecycle = "paused"
shutdown_lifecycle = "stopped"
mmu_enabled = true
trace_values = [0x20080001]
```

Use a case-specific success value that does not collide with existing cases.

## Scripted UART Input

Use `[[inputs]]` for deterministic input:

```toml
[[inputs]]
at_tick = 0
uart = 0
data = "hello"
```

Port must be 0 or 1. Data is delivered as UTF-8 bytes. Tick-0 input is available
before the first execution batch. Entries at equal ticks retain manifest order;
later entries are delivered at their exact emulator-thread boundary. Keep each
input within the 4,096-byte UART RX capacity unless overflow is the behavior
under test.

## Disposable Block Media

Never rely on media left by a previous run. The block case overrides the common
empty `prepare` target:

```make
.PHONY: prepare
prepare:
	mkdir -p build
	rm -f build/filesystem.img build/swap.img
	truncate -s 1024 build/filesystem.img
	truncate -s 1024 build/swap.img
```

`case.mk` makes `prepare` and the packaged image prerequisites of test execution.
The manifest attaches the resulting two-sector images with `block0_media` and
`block1_media`, then selects each assertion's unit and checks persisted bytes at
host-file offset 512 after shutdown. The legacy `block_media` field remains a
unit-0 alias. `make image` alone does not prepare media; `make test` does. Place
every disposable fixture under `build/` so aggregate clean removes it.

The `block-unattached` case prepares and attaches only
`build/filesystem.img` as unit 0. It deliberately omits `block1_media`, leaving
unit 1 unattached so the guest can verify scheduled No Media completion. Its
registration in `Makefile` includes it in aggregate test and clean
targets.

## Run And Diagnose

```sh
# The whole suite: Boot ROM check, runtime build, then every registered case.
make -C conformance test        # or `just conformance` from the repository root

# One registered case through the aggregate Makefile.
make -C conformance test-uart

# One case directly.
make -C conformance/uart test

# Build one image without execution.
make -C conformance image-uart
```

Pass `MINEMU=/path/to/minemu` to any command when the desired executable is not
on `PATH`. Running a case directly skips the Boot ROM check.

When a case fails, inspect the reported expected/actual trace vector first. A
`0xf.......` value identifies a `MINEMU_REQUIRE` code, a contextual `0xe.......`
value identifies a case-defined failure path, and a missing success value usually
means an exception, hang, or insufficient budget.
