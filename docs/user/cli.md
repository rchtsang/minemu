# Command-Line Interface

> **Status: Informative.** The versioned
> [platform specifications](../platform/abi-v1.md) define guest-visible
> behavior. This guide defines command-line use.

`minemu` packages A32 ELF files, boots system images in an interactive terminal
UI, and runs deterministic headless jobs. It does not compile student C or
assembly.

## Paths And Diagnostics

Paths supplied directly as command-line arguments are absolute or relative to
the process working directory. Paths stored inside image and test manifests are
absolute or relative to the directory containing that manifest. Paths are not
subject to shell-style tilde or environment-variable expansion by `minemu`.

The global `--log-file PATH` option appends structured diagnostics to a file.
`RUST_LOG` controls its tracing filter; the default level is `warn`. Ordinary
command errors are still written to standard error as `minemu: MESSAGE`.

Successful commands return status 0. Setup, I/O, validation, runtime, and test
assertion failures return nonzero. Clap handles help and invalid command-line
arguments before execution.

## Package An Image

An image manifest names one kernel ELF and zero or more independently linked
module ELFs:

```toml
kernel = "../kernel/build/minimum-kernel.elf"

[[modules]]
name = "minimum-user"
elf = "../user/prog/minimum-user/build/minimum-user.elf"
```

Unknown top-level and module fields are rejected. Relative `kernel` and `elf`
paths use the manifest directory, while the command-line manifest and output
paths use the process working directory.

```sh
minemu image minimum-template/image/minimum.toml \
  --output minimum-template/image/build/minimum.img
```

The output's parent directory must already exist. An existing output file is
replaced. Success produces no terminal output. Inputs must be little-endian ARM
executable ELF files with supported fixed load segments, no relocations, and an
A32-aligned entry. The output is the normative
[system-image format v1](../platform/system-image-v1.md).

## Interactive Run

`minemu run` opens the TUI by default:

```sh
minemu run minimum-template/image/build/minimum.img \
  --boot-rom minimum-template/bootloader/bootloader.bin
```

The system image is validated before startup. `--boot-rom` is required and must
name exactly 64 KiB of raw reset firmware. The TUI begins paused at reset with
tick 0 and `PC = 0`; starting execution runs the Boot ROM. See the
[TUI guide](../dev/tui.md) for controls and inspection.

Attach existing raw media for unit 0 and unit 1 with `--block0-media` and
`--block1-media`:

```sh
minemu run minimum-template/image/build/minimum.img \
  --boot-rom minimum-template/bootloader/bootloader.bin \
  --block0-media filesystem.img \
  --block1-media swap.img
```

`--block-media` and `-m` remain aliases for unit 0 and cannot be combined with
`--block0-media`. Unit 0 is conventionally filesystem/general storage and unit
1 is swap. Media must be nonempty and a whole number of 512-byte sectors. Each
file is copied into independent write-back state and may be modified in place.
The same canonical file cannot back both units. Dirty bytes are flushed when
execution pauses, resets, shuts down, or terminates with a backend failure.

`--ticks` is headless-only and is rejected in interactive mode.

## Headless Run

Use explicit `--headless` for a bounded noninteractive run:

```sh
minemu run minimum-template/image/build/minimum.img \
  --boot-rom minimum-template/bootloader/bootloader.bin \
  --headless --ticks 100000
```

`--ticks` or `-t` is an absolute virtual-time deadline and defaults to 100,000.
Zero is valid. The emulator thread stops exactly at the deadline, including at
a boundary between instruction retirement and synchronous exception entry.

After execution pauses and shutdown completes, the retained UART0 transmit tail
is written to standard output and UART1 to standard error. Output is not
streamed, no newline is added, and each port retains only its newest 8,192
bytes. Older bytes are discarded. Bytes are converted with lossy UTF-8; relative
ordering between the two ports is not preserved. If execution fails before a
paused inspection can be captured, retained UART diagnostics are not emitted
and the command returns nonzero.

## Declarative Tests

`minemu test MANIFEST` runs scheduled UART input and assertions:

```sh
minemu test minimum-tests/image/minimum-test.toml
```

Success is silent and returns status 0. Captured UART is evaluated by assertions
but is not printed. Runtime, validation, and assertion failures are printed to
standard error and return nonzero. See the complete
[headless-testing reference](../dev/headless-testing.md) for the schema, exact
timing, comparisons, and persistence behavior.
