# TUI Specification

## Purpose

The `minemu` terminal user interface is an observability console for a running
guest kernel. It supports print-style debugging and machine inspection; it is
not intended to be a source-level or instruction-stepping debugger.

The TUI is implemented in Rust with Ratatui and Crossterm. It runs on Linux,
macOS, and Windows terminals supported by Crossterm.

## Responsibilities

The TUI thread owns all terminal behavior:

- Entering and restoring raw mode and the alternate screen.
- Receiving keyboard, resize, and quit events.
- Rendering all panes from immutable machine snapshots.
- Sending user input and runtime-control commands to the emulator thread.

The TUI never accesses Unicorn, guest RAM, live device state, or mutable
emulator state directly.

## Default Layout

The default layout prioritizes the guest console.

- **Console pane:** scrollable UART output and active keyboard input target.
- **Machine pane:** virtual instruction clock, run state, CPU mode, PC, SP,
  and CPSR.
- **Hardware pane:** enabled/pending IRQs and current UART, timer, and block
  device state.
- **Event pane:** recent UART, timer, IRQ, block, exception, and MMU events.
- **Inspector pane:** selected virtual-address translation, page-table walk,
  or memory region.
- **Status line:** active focus, shortcut reminder, image name, and errors.

At small terminal sizes, the console remains visible and inspector panes are
hidden before console space is reduced.

## Interaction Model

Guest-directed input is forwarded as UART receive bytes. TUI-specific commands
use reserved host shortcuts and never become guest input.

The initial command set should provide:

| Action | Behavior |
|---|---|
| Focus console | Send printable keys and paste bytes to the UART queue |
| Toggle inspector | Move focus among machine, event, and memory views |
| Pause/resume | Request an execution-state change from the emulator |
| Reset | Reset the machine and boot the loaded ROM image again |
| Select memory/VA | Change the inspected virtual or physical address |
| Quit | Request orderly emulator shutdown and restore the terminal |

Exact key bindings are a UX detail and may evolve. They must be listed in the
status line and help view, and must avoid consuming ordinary console text while
the console has focus.

## Console Semantics

The console represents UART output, not the host shell.

- Output is retained in bounded scrollback.
- Input bytes are queued in order for the UART receive register.
- Carriage return, line feed, and backspace are rendered for readability.
- Escape sequences are not interpreted as a full terminal protocol.
- Paste sends the pasted byte sequence as guest input.

The TUI should display output promptly, but screen refresh timing must not
change guest virtual time or scheduling behavior.

## Snapshots and Events

The emulator publishes immutable `MachineSnapshot` values. A snapshot includes
only data needed for rendering:

- Run state and virtual instruction count.
- Selected CPU register values and current mode.
- Device and interrupt-controller state.
- Bounded console scrollback.
- Bounded event history.
- Current MMU fault state and selected translation result.

Snapshots do not retain references to Unicorn or live emulator memory. The TUI
may discard stale snapshots and render only the newest one.

The event pane is intentionally hardware-oriented. It should show events such
as timer expiration, IRQ claim/EOI, page fault, block completion, and UART
input. It does not attempt to infer arbitrary student kernel structures. The
optional guest trace device can provide higher-level course events when a
kernel chooses to emit them.

## Runtime Behavior

The TUI communicates with the emulator through channels.

- TUI-to-emulator messages contain input bytes or explicit runtime commands.
- Emulator-to-TUI data consists of immutable snapshots and terminal errors.
- The emulator processes commands between bounded guest execution batches.
- A slow TUI must not block guest execution or accumulate an unbounded backlog.

The TUI must restore the terminal on normal exit, emulator error, and panic.
