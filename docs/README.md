# Documentation

This index routes readers by audience and records document status. Start with
the row matching the work you are doing rather than the repository layout. See
the [project overview](../README.md) for the relationship among the repositories.

## By Audience

| Audience | Start here | Continue with |
|---|---|---|
| Students | [`minimum-template` quickstart](../minimum-template/README.md) | [TUI guide](dev/tui.md), [platform ABI v1](platform/abi-v1.md) |
| Emulator users | [Command-line interface](user/cli.md) | [TUI guide](dev/tui.md) |
| Conformance authors | [`minimum-tests` overview](../minimum-tests/README.md) | [Conformance authoring](../minimum-tests/docs/conformance-authoring.md), [ABI matrix](dev/abi-conformance.md), [versioned specifications](#normative-platform-specifications) |
| Emulator contributors | [Architecture](dev/architecture.md) | [Development workflows](dev/workflows.md), [TUI architecture](dev/tui-architecture.md), [ABI conformance matrix](dev/abi-conformance.md) |

## Document Status

Normative documents define guest-visible behavior. Informative documents
explain usage, implementation, rationale, or evidence and must defer to the
normative platform contract when they disagree.

The versioned documents under `docs/platform/` are the normative platform
contract. Every other guide is informative and defers to those specifications.

## Normative Platform Specifications

| Document | Status | Audience and scope |
|---|---|---|
| [Platform ABI v1](platform/abi-v1.md) | **Normative** | CPU, reset, physical map, terminology, and virtual time |
| [Boot ABI v1](platform/boot-v1.md) | **Normative** | Reset firmware, kernel loading, handoff, and boot info |
| [System-image format v1](platform/system-image-v1.md) | **Normative** | Exact serialized records and image validity |
| [Exceptions and MMU ABI v1](platform/exceptions-and-mmu-v1.md) | **Normative** | Machine exception entry, CP15, translation, and faults |
| [Device ABI v1](platform/devices-v1.md) | **Normative** | MMIO registers, reset values, transitions, and interrupts |

## Informative Guides

| Document | Status | Audience and scope |
|---|---|---|
| [Template memory layout](student/template-memory-layout.md) | **Informative** | Supplied bootstrap tables, mappings, and stack policy |
| [Supplied kernel runtime](student/kernel-runtime.md) | **Informative** | Supplied vector stubs, trap frame, and IRQ flow |
| [Module format and loading](student/module-format-and-loading.md) | **Informative** | Kernel policy for packaged fixed-VA modules |
| [Command-line interface](user/cli.md) | **Informative** | Image packaging, interactive and headless runs, paths, output, and exit behavior |
| [Headless testing](dev/headless-testing.md) | **Informative** | Complete test-manifest schema, timing, assertions, and failure behavior |
| [TUI](dev/tui.md) | **Informative** | Terminal UI views, controls, commands, and inspection behavior |
| [Architecture](dev/architecture.md) | **Informative** | Implemented crate boundaries, ownership, execution, runtime, inspection, and persistence |
| [Development workflows](dev/workflows.md) | **Informative** | Exact Rust, template, conformance, CI, and clean commands |
| [Release readiness](dev/release-readiness.md) | **Informative** | Outstanding performance, development-container, and release-image gates |
| [TUI architecture](dev/tui-architecture.md) | **Informative** | Controller, widget, event, and inspection implementation |
| [ABI conformance matrix](dev/abi-conformance.md) | **Informative** | Mapping from platform requirements to test evidence |
