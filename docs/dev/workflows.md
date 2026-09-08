# Development Workflows

> **Status: Informative.** This guide documents the current repository build,
> test, template, conformance, and clean commands.

## Prerequisites

Initialize both public HTTPS submodules in a fresh checkout:

```sh
git submodule update --init --recursive
```

The workspace pins Rust `1.96.0` with the minimal rustup profile plus `clippy`
and `rustfmt`. Install or select the pinned toolchain through
`rust-toolchain.toml`.

Host development also requires `just`, `zsh`, GNU Make, native C/C++ build
tools, CMake, pkg-config, Clang/libclang, GLib development files, and the GNU Arm
Embedded tools (`arm-none-eabi-gcc`, `ar`, `objcopy`, `readelf`, and `nm`). The
repository `container/Dockerfile` records the complete build package set and
produces the smaller student runtime image. See
[`container/README.md`](../../container/README.md) for local image and Dev
Container workflows. Course documentation must pin a tested image digest rather
than assuming that an unpublished or moving tag exists.

Guest Makefiles use `ARM_PREFIX=arm-none-eabi-` by default. Override it when the
toolchain uses another prefix. Root Just recipes use `CARGO=cargo` by default;
the environment variable may select another Cargo executable.

## Rust Workspace

| Command | Exact scope |
|---|---|
| `just build` | `cargo build --workspace` |
| `just test` | `cargo test --workspace`; Rust tests only, not guest conformance |
| `just check` | `cargo check --workspace --all-targets` |
| `just lint` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `just fmt` | `cargo fmt --all -- --check` |
| `just docs` | Smoke-test generated CLI help |

The workspace contains `minemu-platform`, `minemu-core`, `minemu-image`,
`minemu-unicorn`, `minemu-runtime`, and `minemu`.

## Student Template

```sh
just template
```

This executes the equivalent of:

```sh
make -C minimum-template
make -C minimum-template \
  MINEMU="cargo run --manifest-path $PWD/Cargo.toml -p minemu --" image
```

The first Make invocation rebuilds and compares the canonical Boot ROM, builds
the starter kernel and user program, and builds all kernel examples. It does not
package the system image. The second invocation packages
`minimum-template/image/build/minimum.img` using the workspace CLI.

For standalone template commands and the normal student workflow, see the
[`minimum-template` quickstart](../../minimum-template/README.md).

## Conformance Suite

```sh
just conformance
```

This executes the equivalent of:

```sh
make -C minimum-tests \
  MINEMU="cargo run --manifest-path $PWD/Cargo.toml -p minemu --" test
```

The nested `make test` first builds/runs the baseline image test, including the
Boot ROM consistency check, then builds/runs every focused case registered in
`minimum-tests/headless/Makefile`. It is fail-fast unless Make is invoked with
different scheduling options.

See the [`minimum-tests` maintainer guide](../../minimum-tests/README.md) and
[conformance-authoring guide](../../minimum-tests/docs/conformance-authoring.md)
for individual targets and case construction.

## Full CI-Equivalent Check

```sh
just ci
```

The recipe runs, in order:

```text
fmt -> check -> lint -> test -> docs -> template -> conformance
```

It checks Rust formatting and all targets, denies Clippy warnings, runs all Rust
tests, smoke-tests CLI help, verifies/packages the template, and runs baseline
plus focused guest conformance. It does not build or run Docker automatically.

Container validation remains an explicit maintainer workflow:

```sh
just --justfile container/justfile smoke
just --justfile container/justfile ci
```

## Cleaning

```sh
just clean
```

Root clean runs only `cargo clean`. It does not remove generated files from
either submodule.

```sh
make -C minimum-template clean
make -C minimum-tests clean
```

Each submodule clean recursively removes its component-local `build/`
directories. The conformance clean also removes every registered focused-case
build directory and the block case's disposable disk. Neither command removes
the checked-in canonical `bootloader/bootloader.bin`.
