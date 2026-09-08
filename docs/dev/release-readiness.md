# Release Readiness

> **Status: Informative.** This document records unfinished release-engineering
> work. It is not part of the guest-visible platform contract.

The emulator, template, and conformance repositories are usable directly from
source. The multi-stage Dockerfile, student-tool smoke check, local workflows,
Dev Container configuration, and manual publication instructions now exist
under `container/`. This project does not use GitHub Actions. The following
gates remain before publishing and recommending a supported image:

- Maintain a representative throughput floor of 100,000 strict instructions
  per second.
- Verify the student image and repository CI target on a clean Docker host.
- Verify that the image's GNU Arm compiler reproduces the checked-in canonical
  Boot ROM exactly.
- Confirm the release binary's runtime shared-library dependencies.
- Run the complete workspace, template, conformance, and container smoke checks
  manually.
- Manually publish a versioned `linux/amd64` and `linux/arm64` image with Docker
  Buildx.
- Make the GHCR package public and verify an unauthenticated pull.
- Record and publish the tested multi-architecture image digest; do not describe
  a mutable version tag as an immutable image identity.
- Pin the published image digest in the active student documentation and test
  it from a student account without repository or package-owner privileges.

Until those gates are complete, active documentation must not claim that a
student or release container image is available.
