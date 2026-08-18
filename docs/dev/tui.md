# TUI

`minemu run IMAGE` starts the terminal UI. Use `--headless --ticks N` for
bounded automation. The TUI owns raw mode, mouse capture, and the alternate
screen and restores them on normal exit, errors, and panic unwinding.

The internal controller/widget design is documented in
`tui-architecture.md`; `tui-design.md` is the visual interaction reference.

## Views

The **runtime** view contains console, events, and dialog panes. The TUI opens
with emulation paused; use `:start`, `:s`, or Space+s to begin execution. The
console is focused initially in normal mode. Press `i` to enter insert mode and forward
ordinary keys, Enter, Backspace, and pasted text to the selected UART. `Esc`
returns to normal mode.

The **inspect** view contains one selected primary subview, one selected
secondary subview, and the dialog. Entering inspect pauses emulation before
requesting live Unicorn-backed snapshots.

- Primary: memory or A32 disassembly.
- Secondary: registers, peripherals, or pending fault/interrupt state.
- Pending combines MMU last-fault state with interrupt pending, enabled, and
  claimed state.

Instruction-read failures do not close the TUI. Registers and other snapshots
remain visible while the disassembly pane reports the original error.

Memory inspection covers the readable physical byte map: Boot ROM, system ROM,
and RAM. Physical `00000000` contains the platform reset vector and boot
firmware, while the packaged system image begins at `08000000`. A highlighted
cursor selects one byte; motions move the cursor and automatically shift the
256-byte window across region boundaries while skipping unmapped and MMIO gaps.
Wide panes display eight bytes per row and narrow panes display four. Addresses
omit the `0x` prefix to preserve byte columns. Scrollable panes include an inset
vertical position indicator that does not replace border corners. Narrow
register panes omit decimal values and retain hexadecimal values.

## Input Modes

The persistent input bar displays the current input grammar:

- Normal: pane navigation and global bindings.
- Insert: focused-widget text input; currently supported by console.
- Command: `:` command text.
- Leader: Space followed by one command key.
- ASCII search: `/pattern`.
- Byte search: `\de ad be ef` using whitespace-separated hexadecimal pairs.
- Goto: `>location` using pane-specific syntax.

The hints bar changes with mode and focused pane. It also displays virtual ticks
in hexadecimal.

## Controls

| Input | Effect |
|---|---|
| Ctrl+C / Ctrl+E / Ctrl+D | Focus console, events, or dialog. |
| Ctrl+P / Ctrl+S | Focus inspect primary or secondary. |
| Space+r / Space+i | Select runtime or inspect. |
| Space+s | Toggle emulation start/stop. |
| `[#]h/j/k/l`, `w`, `e`, `gg`, `G` | Count-aware pane-local navigation. |
| `Tab` | Switch the focused inspect pane's subview. |
| `?` | Open the green-bordered help table. |
| Ctrl+left-drag | Resize the main horizontal split. |

Memory and disassembly goto accept hexadecimal addresses, optionally prefixed
with `0x`. Register goto accepts `pc`, `lr`, `sp`, `r0` through `r12`, `cpsr`,
or `spsr`. ASCII and byte searches scan all physical RAM on the emulator thread
in bounded overlapping chunks and move the memory window to a match.

## Commands

| Command | Effect |
|---|---|
| `:q`, `:quit` | Shut down and exit. |
| `:?`, `:help` | Open the help popup. |
| `:start`, `:stop`, `:s` | Start, stop, or toggle emulation. |
| `:reset` | Request an emulated power cycle. |
| `:v`, `:view` | Toggle views. |
| `:view r`, `:view runtime` | Select runtime. |
| `:view i`, `:view inspect` | Select inspect and pause. |
| `:set uart 0|1` | Select console UART. |
| `:set primary mem|disasm` | Select primary inspect subview. |
| `:set secondary reg|peri|pend` | Select secondary inspect subview. |
| `:g LOCATION`, `:goto LOCATION` | Apply pane-specific goto. |

Opening the command bar temporarily pauses a running guest. Cancelling or
executing a non-lifecycle command resumes it. Stop, reset, and selecting inspect
leave it paused. Parse and runtime-operation failures are retained as bounded
red messages in dialog rather than closing the terminal UI.

Dialog messages wrap to the pane width and begin with `>` so message boundaries
remain visible while scrolling.

## Snapshots

Widgets never access `RuntimeHandle` or Unicorn. They emit actions containing
targeted `RuntimeInspectionRequest` values. The TUI runtime controller owns
nonblocking response receivers and routes immutable results to the requesting
widget.

Runtime console and event snapshots update periodically. Paused inspection
snapshots update on inspect entry, navigation, search/goto, subview changes,
reset, and explicit refresh actions. The TUI never continuously copies all
64 MiB of RAM.

## Diagnostic Log

Use `--log-file` to keep structured diagnostics out of the alternate screen.
The file is appended to; `RUST_LOG` selects verbosity and defaults to `warn`.

```sh
RUST_LOG=minemu=debug,minemu_runtime=debug,minemu_unicorn=trace \
cargo run -p minemu -- --log-file /tmp/minemu.log run \
  minimum-template/system/build/minimum.img
```
