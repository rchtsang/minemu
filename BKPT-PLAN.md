# BKPT Support Plan

## Goal

Treat an A32 `BKPT #imm16` instruction as a programmer-inserted pause point in
every minemu runtime. A user can inspect the stopped machine in the TUI and then
resume execution manually.

This is a minemu debugging extension, not a guest exception. It does not enter
an ARM vector or add a platform `ExceptionKind`.

## Backend

- Add a strict A32 `BKPT` decoder beside the existing SVC and CP15 decoders.
- Recognize it in `a32_synchronous_trap_callback` before Unicorn raises its
  internal breakpoint exception. Do not depend on QEMU's private
  `EXCP_BKPT == 7` value.
- Extract the 16-bit immediate and return
  `BackendStop::Breakpoint { address, immediate }` through the existing callback
  stop path.
- Count the breakpoint instruction as one completed instruction and one virtual
  tick.
- Leave PC at the breakpoint address while paused.

## Runtime

- Give a breakpoint stop precedence over instruction-limit completion at the
  same boundary.
- Transition from running to paused, clear any bounded-run remainder, and
  publish the address and immediate in `RuntimeStatus.last_stop`.
- Retain a pending resume address. On the next explicit resume, advance PC by
  four only if it still points at the breakpoint, then clear the pending state.
- Clear pending breakpoint state on reset.
- Apply the same behavior to TUI and headless runtimes.

## TUI

- Keep the current view when a breakpoint pauses execution.
- Refresh paused inspection snapshots and report the breakpoint stop reason.
- Let the user enter Inspect and resume with the existing controls.

## Verification

Test strict instruction decoding, immediate extraction, PC and tick state at the
pause, resume at the following instruction, reset behavior, consecutive
breakpoints, and precedence during bounded execution. Add a headless lifecycle
test that observes a paused runtime after `BKPT`.
