# TUI Architecture

> **Status: Informative.** This document describes implementation architecture,
> not guest-visible platform requirements.

This is the architecture of the implemented TUI redesign. The [TUI guide](tui.md)
documents its user-facing controls.

## Ownership

`App` owns global UI structure only:

- Runtime command and asynchronous inspection controller.
- Ordered `TuiWidget` registry.
- Active view, focused widget, input mode, split geometry, pending requests,
  action queue, and quit state.

Widgets own the snapshots and local navigation state they render. The
registry contains header, console, events, dialog, primary inspector, secondary
inspector, input bar, and hints widgets. The console owns selected UART,
bounded per-UART transcripts, local input echo, and scroll state; the primary
inspector owns independent physical-memory,
virtual-memory, and disassembly state; the secondary inspector owns independent
registers/system/peripherals/pending scroll state; the dialog owns bounded UI
messages.

Widgets must not own `RuntimeHandle`, request Unicorn data directly, or mutate
other widgets.

## Widget Contract

The project-level `TuiWidget` trait is distinct from Ratatui's `Widget` trait.
Each implementation supplies a stable `WidgetId`, visibility predicate,
rendering method, focused-key handler, and app-event updater. Widget methods
receive immutable context and return semantic actions. The app renders visible
widgets in registry order after centralized layout maps widget IDs to terminal
rectangles.

## Actions And Events

`Action` represents user or widget intent. It includes view/focus changes,
pause/resume/reset/quit, UART ingress, settings changes, navigation, search,
goto, split resize, dialog messages, and:

```rust
Action::RequestInspection {
    target: WidgetId,
    request: RuntimeInspectionRequest,
}
```

There is no `InspectionIntent` layer. The runtime controller sends the existing
runtime request directly, retains its receiver without blocking the UI, and
returns a targeted `AppEvent` when the response arrives. Full-RAM search adds a
new owned runtime inspection request containing the byte pattern; it scans
Unicorn RAM in overlapping bounded chunks and returns only a physical address.

`AppEvent` transports immutable runtime status, peripheral/event/MMU snapshots,
physical and translated virtual memory, address translations, execution
snapshots, search results, and recoverable errors. The controller turns ordinary
action failures into red dialog events. Only terminal I/O failures, explicit
quit, and irrecoverable runtime termination close the TUI.

## Input Routing

`InputMode` describes an input grammar, not a target pane:

```rust
enum InputMode {
    Normal,
    Insert,
    Command,
    Leader,
    SearchAscii,
    SearchBytes,
    Goto,
}
```

In insert mode, input is routed to the focused widget. The initial console
widget forwards it to its selected UART, while future editable widgets may use
the same mode. A centralized router handles Ctrl focus bindings, Space leader
sequences, command/search/goto text input, and count-aware motion decoding.
Unhandled normal-mode keys are forwarded to the focused widget.

## Layout

`SplitLayout` owns runtime console width and inspect primary/secondary width.
It computes all rectangles centrally. A left click selects a header view tab or
focuses the visible pane under the pointer. Ctrl+left-drag creates a resize
action; widgets only render into their assigned rectangle. This keeps mouse
geometry pure and unit-testable.

The runtime layout uses console, events, and dialog panes. The inspect layout
uses one selected primary subview (physical memory, virtual memory, or
disassembly), one selected secondary subview (registers, system, peripherals, or
pending), and dialog. Pending combines MMU last-fault data with interrupt
pending/enabled/claim state. System renders live guest MMU state, including
TTBR0 and VBAR.
