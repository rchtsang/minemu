# TUI

`minemu run IMAGE --boot-rom BOOT_ROM` starts the terminal UI. Use `--headless
--ticks N` for bounded automation. The TUI owns raw mode, mouse capture, and
the alternate screen and restores them on normal exit, errors, and panic
unwinding.

The internal controller/widget design is documented in
`tui-architecture.md`; `tui-design.md` is the visual interaction reference.

## Views

The **runtime** view contains console, events, and dialog panes. The TUI opens
with emulation paused; use `:start`, `:s`, or Space+s to begin execution. Use
`:start COUNT` or `:s COUNT` to execute exactly that many instructions and
automatically pause again. The
console is focused initially in normal mode. Press `i` to enter insert mode and forward
ordinary keys, Enter, Backspace, and pasted text to the selected UART. `Esc`
returns to normal mode.

The **inspect** view contains one selected primary subview, one selected
secondary subview, and the dialog. Entering inspect pauses emulation before
requesting live Unicorn-backed snapshots.

- Primary: physical memory, virtual memory, or A32 disassembly.
- Secondary: registers, MMU system state, peripherals, or pending
  fault/interrupt state.
- Pending combines MMU last-fault state with interrupt pending, enabled, and
  claimed state.

Instruction-read failures do not close the TUI. Registers and other snapshots
remain visible while the disassembly pane reports the original error.

Physical memory inspection covers the readable byte map: Boot ROM, system ROM,
and RAM. Physical `00000000` contains the platform reset vector and boot
firmware, while the packaged system image begins at `08000000`. Virtual memory
inspection translates each page through the guest's live MMU state. It rejects
device mappings so inspection cannot trigger MMIO side effects. The first
virtual-memory selection opens around the stopped PC. Both memory views then
retain independent cursors and visible windows.

Virtual mappings are guest-defined and may alias physical addresses. The
minimum template intentionally identity-maps `0x40000000..0x403fffff` during
bootstrap, so virtual and physical inspection at `0x40030264` show the same
bytes. Its `0xc0030264` higher-half address is another alias of physical
`0x40030264`. Unmapped or read-protected virtual addresses report an inspection
error instead of falling back to physical memory.

A highlighted cursor selects one byte; motions move the cursor and automatically
shift the visible window. Physical navigation skips unmapped and MMIO gaps. The
byte request expands or contracts to fill every visible data row. Wide panes
display eight bytes per row and narrow panes display four. Addresses omit the
`0x` prefix to preserve byte columns. Disassembly labels its address column as
`paddr` while the MMU is disabled and `vaddr` while it is enabled, labels
instruction bytes as raw, and highlights the current PC when it is visible.
Scrollable panes include an inset vertical position indicator that does not
replace border corners. Pane content reserves one blank column before the right
border so clipped text remains apparent. Narrow register panes omit decimal
values and retain hexadecimal values.

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

Physical memory, virtual memory, and disassembly goto accept hexadecimal addresses, optionally prefixed
with `0x`. Register goto accepts `pc`, `lr`, `sp`, `r0` through `r12`, `cpsr`,
or `spsr`. ASCII and byte searches scan all physical RAM on the emulator thread
in bounded overlapping chunks and move the memory window to a match.

## Commands

| Command | Effect |
|---|---|
| `:q`, `:quit` | Shut down and exit. |
| `:?`, `:help` | Open the help popup. |
| `:start [COUNT]` | Start continuously, or execute COUNT instructions and pause. |
| `:s [COUNT]` | Toggle without COUNT, or execute COUNT instructions and pause. |
| `:stop` | Stop emulation. |
| `:reset` | Request an emulated power cycle. |
| `:v`, `:view` | Toggle views. |
| `:view r`, `:view runtime` | Select runtime. |
| `:view i`, `:view inspect` | Select inspect and pause. |
| `:set uart 0|1` | Select console UART. |
| `:set primary pmem|vmem|disasm` | Select primary inspect subview. |
| `:set secondary reg|sys|peri|pend` | Select secondary inspect subview. |
| `:translate ADDRESS`, `:xlate ADDRESS` | Translate a virtual address through the live MMU. |
| `:g LOCATION`, `:goto LOCATION` | Apply pane-specific goto. |

Opening the command bar temporarily pauses a running guest. Cancelling or
executing a non-lifecycle command resumes it. Stop, reset, and selecting inspect
leave it paused. Parse and runtime-operation failures are retained as bounded
red messages in dialog rather than closing the terminal UI.

Dialog messages wrap to the pane width and begin with `>` so message boundaries
remain visible while scrolling. Help descriptions wrap within their table cells,
and the close hint is rendered separately on the popup's bottom border.

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
  minimum-template/image/build/minimum.img \
  --boot-rom minimum-template/bootloader/bootloader.bin
```
