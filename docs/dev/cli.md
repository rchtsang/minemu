# CLI And Headless Tests

> **Status: Informative.** The platform ABI takes precedence over this usage
> guide.

`minemu` packages versioned system-ROM images and runs them through the
deterministic emulator runtime. It does not build student C or assembly; run
`make` in `minimum-template` first.

## Image Manifests

`minemu image` accepts a TOML manifest. Paths are relative to the manifest.

```toml
kernel = "../kernel/build/minimum-kernel.elf"

[[modules]]
name = "minimum-user"
elf = "../user/prog/minimum-user/build/minimum-user.elf"
```

Package the supplied template system after building it:

```sh
make -C minimum-template
minemu image minimum-template/image/minimum.toml \
  --output minimum-template/image/build/minimum.img
```

The packer validates that every input is a little-endian ARM executable ELF,
rejects Thumb entries and relocations, then writes the stable versioned image
format documented in the [Emulator ABI](emulator.md).

## Interactive And Headless Runs

`minemu run` validates and maps the system image and the explicitly supplied
64-KiB Boot ROM, then opens the interactive TUI in the paused reset state with
`PC = 0`. Starting emulation executes the Boot ROM; the host does not
prepopulate kernel RAM. Use the command prompt to start, pause, resume, reset,
inspect the paused machine, or quit. See the [TUI guide](tui.md) for controls.

```sh
minemu run minimum-template/image/build/minimum.img \
  --boot-rom minimum-template/bootloader/bootloader.bin
minemu run minimum-template/image/build/minimum.img \
  --boot-rom minimum-template/bootloader/bootloader.bin \
  --block-media minimum-template/image/build/disk.img
```

For CI or scripts, `--headless` stops exactly at the requested virtual-tick
budget. The emulator thread enforces the boundary rather than relying on host
polling. UART0 output is sent to standard output and UART1 output to standard
error.

```sh
minemu run minimum-template/image/build/minimum.img \
  --boot-rom minimum-template/bootloader/bootloader.bin \
  --headless --ticks 100000
```

The optional media file is copied into write-back device state. The runtime
flushes dirty media when pausing, resetting, shutting down, or handling a
terminal backend failure.

## Test Manifests

`minemu test` runs a TOML manifest with scheduled UART input and assertions.
The image, Boot ROM, and optional block-media paths are relative to the test
manifest.

```toml
image = "build/minimum.img"
boot_rom = "../bootloader/bootloader.bin"
block_media = "build/disk.img"
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
offset = 512
bytes = [0xde, 0xad, 0xbe, 0xef]
```

`uart` accepts `0` or `1`. The emulator thread injects inputs at the exact
`at_tick` boundary and stops execution at exactly `max_ticks`;
`instruction_batch` is a positive maximum batch size, and the runtime shortens
batches at scheduled boundaries. Every test must contain at least one
assertion, and a runtime failure always fails the test. Ordinary machine,
output, fault, event, and tick assertions use the execution snapshot captured
before shutdown.
`execution_lifecycle` and `shutdown_lifecycle` explicitly select the snapshot
whose lifecycle is checked. Trace values are compared to the complete bounded
trace-event sequence in order. Block media is attached write-back, so tests
should use an ignored working copy; byte-region assertions run after shutdown
flushes it. `ram_prefill` regions must fall within physical RAM and are reapplied
on reset before the Boot ROM runs; they are intended for tests that prove reset
firmware overwrites or clears RAM. Unknown fields are rejected throughout image
and test manifests so misspelled settings or checks cannot silently pass.

Run the supplied smoke test after packaging its image:

```sh
minemu test minimum-tests/image/minimum-test.toml
```
