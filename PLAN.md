# Tracing Plan

## Goal

Make TUI and runtime inspection failures diagnosable without a second terminal
or corrupting the Ratatui alternate screen. Logging must preserve the original
runtime/backend error before user-facing code reduces it to `CliError::RuntimeSetup`.

## Steps

1. Add `tracing` to `minemu-runtime` and `minemu-unicorn`; add `tracing` and
   `tracing-subscriber` to the `minemu` CLI crate.
2. Add a global `--log-file PATH` argument. Initialize one non-ANSI,
   file-only subscriber before command execution, honoring `RUST_LOG` and
   defaulting to `warn`. Do not write tracing output to the TUI terminal.
3. Document the diagnostic invocation:

   ```sh
   RUST_LOG=minemu=debug,minemu_runtime=debug,minemu_unicorn=trace \
   cargo run -p minemu -- --log-file /tmp/minemu.log run \
     minimum-template/system/build/minimum.img
   ```

4. Instrument the TUI boundary with structured events for command
   start/cancel/execute, command-induced pause/resume behavior, introspection
   entry, and every memory/execution inspection request. Log exact request or
   response errors before mapping them to `CliError::RuntimeSetup`.
5. Instrument `minemu-runtime` with structured events for command enqueue and
   receipt, lifecycle transitions, inspection request type, inspection result,
   lifecycle, and virtual tick. Do not log periodic status publication or guest
   instruction batches.
6. Instrument `minemu-unicorn` inspection operations with PC, CPSR, virtual
   address/range, requested access mode, and exact Unicorn errors. Keep this
   out of per-instruction execution paths unless explicitly enabled at trace
   level.
7. Add regression coverage for enabled-MMU inspection failures and document
   retrieving the log file in `docs/dev/tui.md`.

## Scope For This Change

All seven steps are implemented. Unicorn inspection tracing remains outside
guest instruction execution and is enabled only by the `minemu_unicorn` filter.
