# Release Readiness

> **Status: Informative.** This document records unfinished release-engineering
> work. It is not part of the guest-visible platform contract.

The emulator, template, and conformance repositories are usable directly from
source. The Assignment 1 student image has also been validated and published as
the public `linux/amd64` and `linux/arm64` OCI index
`rtsang1/cs492-stevens:0.1.0`. Its immutable digest is
`sha256:9b9c8be5ccdadc046ad4577107e087aee2dad21b8fb6e081238833a70a2ef7a7`.
This project uses manual native-platform publication rather than GitHub Actions.

The following release-engineering gate remains:

- Maintain a representative throughput floor of 100,000 strict instructions
  per second.
