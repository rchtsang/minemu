# minemu

`minemu` is a deterministic A32 teaching platform for operating-systems
coursework. This repository contains the Rust emulator, image packer, terminal
UI, headless test runner, platform documentation, and two related projects as
Git submodules.

## Repositories

| Project | Purpose | Start here |
|---|---|---|
| `minemu` | Emulator, image tooling, TUI, headless runner, and platform contract | [Documentation index](docs/README.md) |
| `minimum-template` | Student starter with reset firmware, kernel and user scaffolding, examples, and image definitions | [Template quickstart](minimum-template/README.md) |
| `minimum-tests` | Emulator conformance fork with focused guest programs, manifests, and assertions | [Conformance README](minimum-tests/README.md) |

Clone with submodules, or initialize them in an existing checkout:

```sh
git submodule update --init --recursive
```

## Choose A Starting Point

| Audience | Documentation |
|---|---|
| Students using the starter | [Template quickstart](minimum-template/README.md), [TUI guide](docs/dev/tui.md), and current [platform ABI](docs/dev/emulator.md) |
| Emulator users | [CLI and headless usage](docs/dev/cli.md) and [TUI guide](docs/dev/tui.md) |
| Conformance authors | [Conformance matrix](docs/dev/abi-conformance.md) and [minimum-tests](minimum-tests/README.md) |
| Emulator contributors | [Design](docs/dev/design.md), [TUI architecture](docs/dev/tui-architecture.md), and [documentation index](docs/README.md) |

The [documentation index](docs/README.md) identifies which documents are
normative platform contracts and which are informative guides.

## Development

The root [`justfile`](justfile) provides the main workflows:

```sh
just build
just test
just template
just conformance
just ci
```

`just ci` checks formatting and linting, runs the Rust workspace tests, builds
the student template, packages its image, and runs the conformance suite.
