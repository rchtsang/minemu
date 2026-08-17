# TUI

`minemu run IMAGE` starts the terminal UI. Use `--headless --ticks N` for a
bounded, noninteractive run suitable for scripts and CI.

The TUI is an observability console, not a source-level debugger. The TUI
thread owns Crossterm raw mode and the alternate screen; it communicates with
the emulator only through `minemu-runtime` commands and immutable snapshots.
Terminal state is restored on normal exit, errors, and panic unwinding.

## Views

The initial **runtime** view prioritizes guest UART output and event history.
The console is the initial focus and ordinary key presses and pasted text are
queued to the selected UART. `Enter` and `Backspace` send their corresponding
guest input bytes. Press `Esc` to leave console focus.

Use `:view inspect` to enter **introspection**. Entering it pauses a running
guest before requesting its snapshot. It presents live Unicorn-backed physical
RAM as hex/ASCII, A32 Capstone disassembly from the current PC, CPU registers,
and interrupt/peripheral state. It is intentionally a paused inspection view:
the runtime does not maintain a continuously copied 64 MiB RAM image. Use
`:view runtime` to return; use `:resume` when execution should continue.

At small terminal sizes, the active inspection pane is shown alone instead of
compressing all panes into unusable columns.

The header presents `runtime` and `inspect` tabs, with the active view
highlighted. Pane borders and titles are yellow; the focused pane uses a bold
yellow outline and title. Entering `:` temporarily pauses a running guest while
the command prompt is active. Cancelling the prompt or running a non-lifecycle
command resumes that guest; `:pause`, `:reset`, and `:view inspect` leave it
paused.

## Controls

While console focus is active, guest input takes precedence. After `Esc`, the
following controls work in both views:

| Input | Effect |
|---|---|
| `Tab` | Cycle panes in the current view. |
| `h` `j` `k` `l` | Move by pane-local character, row, or byte/line units. |
| `w` `e` | Advance by pane-local natural items: memory words, instructions, or rows. |
| `gg` / `G` | Move to the first / last position in the focused pane. |
| Count prefix | Applies a decimal count, for example `12j` or `4w`. |
| `:` | Open a command prompt. |
| `Esc` | Cancel a partial motion or command. |

The supported commands are:

| Command | Effect |
|---|---|
| `:pause`, `:resume`, `:reset` | Control emulator lifecycle. |
| `:quit` or `:q` | Shut down the runtime and exit. |
| `:view runtime` / `:view inspect` | Change view. |
| `:focus console|events|memory|disasm|cpu|hardware` | Select a pane. |
| `:mem ADDRESS` | Inspect a 256-byte physical RAM window, with a hexadecimal address. |
| `:uart 0` or `:uart 1` | Select the console input UART. |
| `:help` | Display the command overlay. |

## Snapshot Boundaries

Status, events, and platform peripherals are copied into immutable runtime
responses. Live memory and current instruction bytes are read on the emulator
thread only after the guest is paused, directly from Unicorn. This keeps
inspection accurate after guest stores without making every rendered frame copy
the complete RAM mapping.

## Diagnostic Log

Use `--log-file` to keep structured diagnostics out of the TUI terminal. The
file is appended to and `RUST_LOG` selects verbosity; the default filter is
`warn`.

```sh
RUST_LOG=minemu=debug,minemu_runtime=debug,minemu_unicorn=trace \
cargo run -p minemu -- --log-file /tmp/minemu.log run \
  minimum-template/system/build/minimum.img
```

For a failed `:view inspect`, inspect `/tmp/minemu.log` for the TUI request,
runtime inspection result, lifecycle, tick, PC, CPSR, requested virtual range,
and the exact Unicorn error.
