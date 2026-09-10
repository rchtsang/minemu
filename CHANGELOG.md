# Changelog

All notable changes to `minemu` are documented in this file.

## 0.2.0 - 2026-09-10

### Added

- Two selectable block-media units on the existing serialized controller.
- Explicit unit-0 and unit-1 CLI and headless-manifest media paths.
- Unit-aware persisted-media assertions and per-unit TUI inspection.

### Changed

- Extended the guest block ABI with the `UNIT` register and Invalid Unit error.
- Published the two-unit device contract as Platform ABI v2 while retaining the
  Boot ABI and system-image wire format at version 1.
- Added a supplied synchronous guest block interface for filesystem and swap use.

## 0.1.1 - 2026-09-09

### Added

- Mouse selection for the runtime and inspect tabs and their visible panes.
- Immediate local echo of keyboard and pasted UART input in the TUI console.

### Fixed

- Declared Ninja as a container build dependency for Unicorn on ARM64.
- Replaced emulated multi-platform publication with native architecture builds
  and manifest assembly.
