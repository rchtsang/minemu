# Release Readiness

> **Status: Informative.** This document records unfinished release-engineering
> work. It is not part of the guest-visible platform contract.

The emulator, template, and conformance repositories are usable directly from
source. The historical Assignment 1 student image was validated and published
with `minemu` 0.1.0 and Platform ABI v1 as the public `linux/amd64` and
`linux/arm64` OCI index
`rtsang1/cs492-stevens:0.1.0`. Its immutable digest is
`sha256:9b9c8be5ccdadc046ad4577107e087aee2dad21b8fb6e081238833a70a2ef7a7`.
This project uses manual native-platform publication rather than GitHub Actions.

The following release-engineering gates remain for `minemu` 0.2.0 and Platform
ABI v2:

- Publish and smoke-test native `linux/amd64` and `linux/arm64` images, then
  assemble and verify the multi-platform version tag.
- Maintain a representative throughput floor of 100,000 strict instructions
  per second.
