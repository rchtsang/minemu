# Documentation

This index routes readers by audience and records document status. Start with
the row matching the work you are doing rather than the repository layout. See
the [project overview](../README.md) for the relationship among the repositories.

## By Audience

| Audience | Start here | Continue with |
|---|---|---|
| Students | [`minimum-template` overview](../minimum-template/README.md) | [TUI guide](dev/tui.md), current [platform ABI](dev/emulator.md) |
| Emulator users | [CLI and headless usage](dev/cli.md) | [TUI guide](dev/tui.md) |
| Conformance authors | [`minimum-tests` overview](../minimum-tests/README.md) | [ABI conformance matrix](dev/abi-conformance.md), current [platform ABI](dev/emulator.md) |
| Emulator contributors | [Design](dev/design.md) | [TUI architecture](dev/tui-architecture.md), [ABI conformance matrix](dev/abi-conformance.md) |

## Document Status

Normative documents define guest-visible behavior. Informative documents
explain usage, implementation, rationale, or evidence and must defer to the
normative platform contract when they disagree.

The current [Emulator ABI](dev/emulator.md) remains the normative contract until
the planned versioned documents under `docs/platform/` replace it. Those
versioned platform documents will be explicitly marked **Normative**. Every
other guide is **Informative**.

## Current Documents

| Document | Status | Audience and scope |
|---|---|---|
| [Emulator ABI](dev/emulator.md) | **Normative, transitional** | Current machine, boot, image, exception, MMU, and device contract |
| [CLI and headless tests](dev/cli.md) | **Informative** | Image packaging, interactive/headless execution, and test manifests |
| [TUI](dev/tui.md) | **Informative** | Terminal UI views, controls, commands, and inspection behavior |
| [Design](dev/design.md) | **Informative** | Emulator goals, components, and architectural decisions |
| [TUI architecture](dev/tui-architecture.md) | **Informative** | Controller, widget, event, and inspection implementation |
| [ABI conformance matrix](dev/abi-conformance.md) | **Informative** | Mapping from platform requirements to test evidence |
| [Feasibility spike results](dev/spike-results.md) | **Informative, historical** | Early prototype findings; not a current implementation contract |
| [Historical TUI visual specification](../tui-design.md) | **Informative, historical** | Original interaction and layout design |
