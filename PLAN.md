# TUI Redesign Plan

`tui-design.md` is the implementation reference for this redesign.

Status: implemented and verified.

## Agreed Architecture

1. `App` owns only global UI structure: the runtime controller, widget registry,
   active view, focused widget, input state, split geometry, pending requests,
   action queue, and quit state. Emulator snapshots move into their owning
   widgets.
2. `TuiWidget` implementations are rendered in registry order and receive an
   immutable render context, focused key events, and app events. Widgets emit
   semantic actions but never access `RuntimeHandle` or one another directly.
3. `Action::RequestInspection { target, request }` carries
   `RuntimeInspectionRequest` directly. No intermediate `InspectionIntent`
   type is needed. The runtime controller owns response receivers and routes
   completed snapshots back to the target widget.
4. `InputMode` names only input grammars: normal, insert, command, leader,
   ASCII search, byte search, and goto. Insert input is interpreted by the
   focused widget, initially the console; it is not a console-specific mode.
5. A centralized input router handles global bindings, leader sequences,
   command/search/goto text, and count-aware motions before forwarding
   unhandled normal-mode keys to the focused widget.
6. Layout computation is centralized and maps widget IDs to rectangles. Widgets
   do not determine sibling geometry. Ctrl+drag resizing updates split state
   through actions and is tested without a terminal.

## Interaction And Commands

1. Replace the current focus model with persistent runtime panes
   `console/events/dialog` and inspect panes `primary/secondary/dialog`.
2. Add explicit normal, insert, command, leader, ASCII-search, byte-search,
   and goto input states. Render their input in the persistent input bar and
   clear it after an action.
3. Start paused in console normal mode. `i` enters insert mode and `Esc`
   returns to normal mode. Normal-mode `?` opens the help popup.
4. Bind Ctrl+C, Ctrl+E, Ctrl+D, Ctrl+P, and Ctrl+S to console, events, dialog,
   primary, and secondary focus. Keep Tab for primary/secondary subview
   selection.
5. Add leader commands: Space then `r`, `i`, or `s` selects runtime, inspect,
   or toggles emulation. Show partial leader input and contextual hints.
6. Replace command parsing with:
   - `:q`/`:quit`
   - `:?`/`:help`
   - `:start`, `:stop`, and `:s` lifecycle toggle
   - `:reset`
   - `:v`/`:view` with optional `r`/`runtime` or `i`/`inspect`; no argument
     toggles views
   - `:set uart|primary|secondary`
   - `:g`/`:goto`, delegated to the active pane
7. Map `:stop` to pause and `:start` to resume. Shutdown remains exclusive to
   quit.

## Views And Data

1. Add primary `memory` and `disassembly` subviews, rendering only the selected
   subview. Add secondary `registers`, `peripherals`, and `pending` subviews.
2. Render pending state from the existing MMU last-fault fields plus interrupt
   pending/enabled/claim state; no platform ABI expansion is needed.
3. Add independent scroll/cursor state for every pane, with count-prefixed
   motions, `G`, and pane-specific goto behavior. Register rows show
   hexadecimal and decimal values.
4. Implement `/ASCII` and `\\HEX` searches across all 64 MiB RAM. Add an
   emulator-thread `RuntimeInspectionRequest` that scans Unicorn RAM in
   bounded, overlapping chunks and returns only the matching physical address;
   move the 256-byte display window to that match.
5. Refresh paused inspection snapshots on inspect entry, navigation,
   search/goto, configuration changes, reset, and explicit actions rather than
   every render loop.

## Rendering And Errors

1. Convert operational failures from terminal exits into bounded red dialog
   entries, retaining tracing context and original backend errors. Terminal and
   runtime-fatal failures remain terminal errors.
2. Rebuild the renderer around the documented layouts: tabs, focus markers,
   dim yellow inactive panes, brighter bold focused panes, red dialog errors,
   a distinct input background, contextual hints, and hexadecimal tick footer.
3. Add mouse capture and Ctrl+left-drag split resizing. Persist runtime
   console width and inspect primary/secondary width in app state.
4. Synchronize `docs/dev/tui.md` with `tui-design.md` after implementation.

## Verification

1. Add tests for aliases, leader state, modes, dialog recovery, Ctrl focus,
   search parsing and chunk-boundary matches, goto behavior, split geometry,
   and runtime/Unicorn inspection paths.
2. Run formatting, strict Clippy, workspace tests, template image/headless
   tests, and a manual TUI pass using `--log-file`.
