# CLI And Headless Tests

`minemu` packages versioned system-ROM images and runs them through the
deterministic emulator runtime. It does not build student C or assembly; run
`make` in `minimum-template` first.

## Image Manifests

`minemu image` accepts a TOML manifest. Paths are relative to the manifest.

```toml
kernel = "../build/kernel/example.elf"

[[modules]]
name = "minimum-user"
elf = "../build/user/minimum-user.elf"
```

Package the supplied template system after building it:

```sh
make -C minimum-template
minemu image minimum-template/system/minimum.toml \
  --output minimum-template/system/build/minimum.img
```

The packer validates that every input is a little-endian ARM executable ELF,
rejects Thumb entries and relocations, then writes the stable versioned image
format documented in `emulator.md`.

## Interactive And Headless Runs

`minemu run` validates and maps the system image and the explicitly supplied
64-KiB Boot ROM, then opens the interactive TUI in the paused reset state with
`PC = 0`. Starting emulation executes the Boot ROM; the host does not
prepopulate kernel RAM. Use the command prompt to start, pause, resume, reset,
inspect the paused machine, or quit. See `tui.md` for controls.

```sh
minemu run minimum-template/system/build/minimum.img \
  --boot-rom minimum-template/bootrom/minemu-bootrom.bin
minemu run minimum-template/system/build/minimum.img \
  --boot-rom minimum-template/bootrom/minemu-bootrom.bin \
  --block-media build/disk.img
```

For CI or scripts, `--headless` stops after the requested virtual-tick budget.
UART0 output is sent to standard output and UART1 output to standard error.

```sh
minemu run minimum-template/system/build/minimum.img \
  --boot-rom minimum-template/bootrom/minemu-bootrom.bin \
  --headless --ticks 100000
```

The optional media file is copied into write-back device state. The runtime
flushes dirty media when pausing, resetting, shutting down, or handling a
terminal backend failure.

## Test Manifests

`minemu test` runs a TOML manifest with scheduled UART input and assertions.
The image and Boot ROM paths are relative to the test manifest.

```toml
image = "build/minimum.img"
boot_rom = "../bootrom/minemu-bootrom.bin"
max_ticks = 100

[[inputs]]
at_tick = 20
uart = 0
data = "hello\n"

[assert]
uart0_contains = "hello"
ticks_at_least = 100
lifecycle = "stopped"
mmu_enabled = true
fault_status = 0x00000101
trace_values = [1, 2, 3]
```

`uart` accepts `0` or `1`. Inputs are injected once the virtual instruction
clock reaches `at_tick`. Assertions may independently omit console, MMU/fault,
trace, lifecycle, and tick checks. Trace values are compared to the complete
bounded trace-event sequence in order.

Run the supplied smoke test after packaging its image:

```sh
minemu test minimum-template/system/minimum-test.toml
```
