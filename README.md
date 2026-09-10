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
| Students using the starter | [Template quickstart](minimum-template/README.md), [TUI guide](docs/dev/tui.md), and [platform ABI v2](docs/platform/abi-v2.md) |
| Emulator users | [Command-line interface](docs/user/cli.md) and [TUI guide](docs/dev/tui.md) |
| Conformance authors | [Conformance matrix](docs/dev/abi-conformance.md) and [minimum-tests](minimum-tests/README.md) |
| Emulator contributors | [Architecture](docs/dev/architecture.md), [development workflows](docs/dev/workflows.md), and [documentation index](docs/README.md) |
| Container users | [Student development image](container/README.md) and [AI-assisted development](container/ai-development.md) |

The [documentation index](docs/README.md) identifies which documents are
normative platform contracts and which are informative guides.

## Development

The root [`justfile`](justfile) provides the main workflows:

```sh
just build
just test
just docs
just template
just conformance
just ci
```

`just ci` checks formatting, linting, and CLI help; runs the Rust workspace
tests; builds the student template; packages its image; and runs the
conformance suite.

## Student Development Images

The multi-stage [`container/Dockerfile`](container/Dockerfile) builds a current
lightweight student environment containing an installed `minemu`, the GNU Arm
Embedded toolchain, common development tools, and optional OpenCode support.
The previously published Assignment 1 image remains pinned to `minemu` 0.1.0
and Platform ABI v1. Local builds, the optional Dev Container, smoke tests, and
current publication instructions are in [`container/README.md`](container/README.md).
