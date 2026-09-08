# Release Readiness

> **Status: Informative.** This document records unfinished release-engineering
> work. It is not part of the guest-visible platform contract.

The emulator, template, and conformance repositories are usable directly from
source. The following work remains before publishing supported development and
release container images:

- Maintain a representative throughput floor of 100,000 strict instructions
  per second.
- Finish `container/Dockerfile` with pinned Rust and Just versions, Zsh, the GNU
  Arm toolchain, and the native dependencies required to build Unicorn.
- Install the supplied Zsh files as `.zshenv` and `.zshrc`, retain the non-root
  development user, and configure a valid UTF-8 locale.
- Add a container smoke test that mounts the workspace and runs `just ci`.
- Document local development-image build, shell, test, UID/GID, and cache usage
  in `container/README.md`.
- Add a multi-stage release image containing a prebuilt `minemu` binary and
  version-matched platform artifacts.
- Publish versioned `linux/amd64` and `linux/arm64` images only after workspace,
  template, conformance, and container smoke checks pass.

Until those gates are complete, active documentation must not claim that a
student or release container image is available.
