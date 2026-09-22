# Headless Testing

> **Status: Informative.** This is the canonical schema and execution reference
> for `minemu test`. Guest-visible behavior is defined by the versioned
> [platform specifications](../platform/abi-v2.md).

`minemu test MANIFEST` loads a prebuilt system image, executes it to an exact
virtual-time boundary, captures execution state, requests shutdown, and checks
the manifest's assertions. It does not package an image or compile guest code.

## Complete Example

```toml
image = "build/minimum.img"
boot_rom = "../bootloader/bootloader.bin"
block0_media = "build/filesystem.img"
block1_media = "build/swap.img"
instruction_batch = 1
max_ticks = 100

[[ram_prefill]]
address = 0x40030000
length = 128
value = 0xa5

[[inputs]]
at_tick = 20
uart = 0
data = "hello\n"

[assert]
uart0_contains = "hello"
ticks_at_least = 100
execution_lifecycle = "paused"
shutdown_lifecycle = "stopped"
mmu_enabled = true
fault_status = 0x00000101
trace_values = [1, 2, 3]

[[assert.block_media]]
unit = 1
offset = 512
bytes = [0xde, 0xad, 0xbe, 0xef]
```

Unknown fields are rejected in the top-level table, every input, every RAM
prefill, `[assert]`, and every block-media assertion. Missing required fields,
wrong types, out-of-range integers, unknown lifecycle names, and
`instruction_batch = 0` are also manifest errors.

## Top-Level Fields

| Field | Type | Required/default | Meaning |
|---|---|---|---|
| `image` | string path | Required | Valid prebuilt system image |
| `boot_rom` | string path | Required | Exactly 65,536 bytes of raw Boot ROM |
| `block_media` | string path | Optional | Existing nonempty raw media whose size is a multiple of 512 bytes |
| `block0_media` | string path | Optional | Explicit unit-0 media; conflicts with `block_media` |
| `block1_media` | string path | Optional | Unit-1 media |
| `instruction_batch` | positive integer | Default `1024` | Maximum instructions requested per backend batch |
| `max_ticks` | `u64` | Default `100000` | Absolute virtual-time deadline; zero is valid |
| `inputs` | array of tables | Default empty | Scheduled UART byte strings |
| `ram_prefill` | array of tables | Default empty | RAM initialization performed by the harness |
| `assert` | table | Required | Execution/shutdown expectations |

Image, Boot ROM, and all block-media paths are absolute or relative to the
test manifest's directory. They are joined lexically; `minemu` does not expand
`~` or environment variables. `block_media` remains a unit-0 compatibility
alias. The same canonical file cannot be attached to both units. Media files are
mutated in place, so tests should use ignored disposable copies.

The `[assert]` table must contain at least one recognized assertion field or a
nonempty `[[assert.block_media]]` array. Presence is the validation criterion;
for example, an empty UART substring or `ticks_at_least = 0` is accepted even
though it is usually a weak oracle.

## Scheduled UART Input

Each `[[inputs]]` table requires:

| Field | Type | Meaning |
|---|---|---|
| `at_tick` | `u64` | Exact virtual-time boundary at which input becomes due |
| `uart` | `u8` | Port `0` or `1`; every other value is rejected |
| `data` | string | UTF-8 bytes delivered in string order; empty is allowed |

The emulator thread sorts entries by `at_tick`; entries with equal ticks retain
manifest order. Before each execution batch it delivers every entry whose
`at_tick` is no later than current virtual time, then checks `max_ticks`.
Consequently, tick-0 input arrives before the first instruction, input exactly
at `max_ticks` is delivered before the runtime pauses, and input after the
deadline is not delivered.

Batches are shortened at the next input or execution deadline, so
`instruction_batch` affects throughput but not timing precision. Multi-tick
synchronous exception entry may be split at an exact boundary. Each UART RX
queue retains at most 4,096 bytes and drops its oldest byte on overflow; one
large scheduled string can therefore evict its own prefix before guest code
reads it.

## RAM Prefill

Each `[[ram_prefill]]` table requires:

| Field | Type | Meaning |
|---|---|---|
| `address` | `u32` | First physical RAM byte; no alignment requirement |
| `length` | `u32` | Nonzero byte count |
| `value` | `u8` | Byte repeated throughout the range |

The half-open range `[address, address + length)` must fit completely within PA
`[0x4000_0000, 0x4400_0000)` without overflow. Entries apply in manifest order,
so later overlapping prefills win. Prefills occur after platform reset clears
RAM and before reset firmware executes, and are reapplied after runtime reset.
They are test-harness behavior intended, for example, to prove that firmware
copies initialized data and clears BSS.

## Execution And Shutdown Snapshots

Normal test execution starts in `running` state and pauses at `max_ticks`. An
A32 `BKPT` may pause it earlier, allowing a manifest to inspect a
programmer-inserted checkpoint. Scheduled input and device deadlines are
processed by the emulator thread rather than by host polling.

The runner records two lifecycle snapshots:

| Snapshot | Capture point | Used by |
|---|---|---|
| Execution | At deadline pause, before requested shutdown | UART, ticks, execution lifecycle, MMU, fault, trace |
| Shutdown | After shutdown and final media flush | Shutdown lifecycle |

Peripheral, MMU, and event inspection requires a paused execution snapshot. A
runtime failure automatically fails the test before ordinary execution
assertions, even if the manifest does not include a lifecycle assertion.

Block-media assertions are checked from persisted host files after shutdown,
not from in-memory device state. Runtime and execution assertions are evaluated
first so a lifecycle failure is not hidden by a resulting media mismatch.

## Assertions

All scalar and sequence fields under `[assert]` are optional.

| Field | Accepted value | Exact comparison |
|---|---|---|
| `uart0_contains` | string | Case-sensitive contiguous substring of retained UART0 text |
| `uart1_contains` | string | Case-sensitive contiguous substring of retained UART1 text |
| `ticks_at_least` | `u64` | Execution tick must be greater than or equal to the value |
| `execution_lifecycle` | `"paused"` or `"stopped"` | Exact pre-shutdown lifecycle |
| `shutdown_lifecycle` | `"stopped"` | Exact final lifecycle |
| `mmu_enabled` | Boolean | Exact execution-snapshot MMU-enabled state |
| `fault_status` | `u32` | Exact raw DFSR value of the latest fault; a missing fault fails |
| `trace_values` | array of `u32` | Exact retained trace-value sequence, including order and length |

UART comparisons operate on each port's newest 8,192 transmitted bytes after
lossy UTF-8 conversion. Invalid byte sequences become the replacement
character. The fields are substring checks, not raw-byte or whole-output
comparisons; an empty expected string always matches.

`fault_status` does not compare DFAR and there is no assertion for “no fault.”
`trace_values` ignores trace timestamps and filters exception events, then
compares values exactly. Its source is the trace subset of the newest 4,096
global observable events; exceptions consume capacity and may evict older trace
events. `trace_values = []` asserts that no trace value remains in that history.

## Block-Media Assertions

Each `[[assert.block_media]]` table requires:

| Field | Type | Meaning |
|---|---|---|
| `unit` | `u8` | Unit `0` or `1`; defaults to `0` |
| `offset` | `u64` | Zero-based host-file byte offset |
| `bytes` | array of `u8` | Exact expected byte sequence |

The test must attach the selected unit with `block_media`, `block0_media`, or
`block1_media` as appropriate. After shutdown flushes dirty write-back media,
the runner rereads that unit's file and compares
`[offset, offset + bytes.len())` exactly. Assertions need not be sector-aligned,
may overlap, and run in manifest order. Out-of-range regions and byte mismatches
fail. An empty byte array is accepted and passes when its offset is no greater
than the media length.

## Result And Exit Behavior

Assertions stop at the first failure. Execution assertions are evaluated in
this order: runtime failure, UART0,
UART1, ticks, execution lifecycle, shutdown lifecycle, MMU state, fault status,
then trace values. Block-media assertions follow in manifest order.

Success prints nothing and returns status 0. Captured UART output is not printed.
Manifest, setup, runtime, persistence, and assertion errors are written to
standard error as `minemu: MESSAGE` and return nonzero. There is no structured
result output or distinct exit status for assertion failures.

A flush error is recorded in runtime status. A persistent error that makes the
final lifecycle `failed` fails the test automatically. If a deadline-pause
flush fails transiently but the shutdown retry succeeds, the final lifecycle
may be `stopped`; there is no separate assertion over the earlier `last_error`.
