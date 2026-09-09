# Student Development Image

The `container/` directory defines the student development environment for
`minimum`. The default `student` image contains:

- A prebuilt `minemu` executable and the matching source tree at `/opt/minemu`.
- GNU Arm Embedded GCC and Binutils for freestanding A32 guest code.
- GNU Make, Git, Zsh, GDB, Python, Ripgrep, jq, and common command-line tools.
- `just` for repository workflows.
- OpenCode for optional AI-assisted development.

Rust, Clang, CMake, libclang, and the native Unicorn build dependencies remain
in build-only stages. Students do not need them to build and run `minimum`, so
they are omitted from the published student image.

## Local Build

From the repository root:

```sh
just --justfile container/justfile build
just --justfile container/justfile smoke
```

The local recipe creates `minemu-dev:local` with a `dev` user matching the
current host UID. It retains the image's non-conflicting default GID rather than
reusing host system-group IDs. Override the image name when needed:

```sh
IMAGE=minemu-dev:test just --justfile container/justfile build
```

Open a shell with the current repository mounted at `/workspace`:

```sh
just --justfile container/justfile shell
```

The build uses the repository root as its context. Docker automatically applies
`container/Dockerfile.dockerignore`, which excludes Git metadata, build outputs,
nested course repositories, common credential-file patterns, and files outside
the explicit emulator-source allowlist.

## Published Image

Maintainers manually publish `linux/amd64` and `linux/arm64` images to:

```text
rtsang1/cs492-stevens.edu
```

After an image has been published, replace `VERSION` with that version:

```sh
docker pull rtsang1/cs492-stevens.edu:VERSION
docker run --rm -it \
  --add-host host.docker.internal:host-gateway \
  --volume "$PWD:/workspace:Z" \
  --workdir /workspace \
  rtsang1/cs492-stevens.edu:VERSION
```

The published image uses UID and GID 1000. On a Linux host with different IDs,
use the Dev Container workflow, which updates the remote user, or build the
image locally with the supplied Just recipe before mounting a writable
workspace. The `:Z` mount option permits access on SELinux-enforcing hosts.

Configure `rtsang1/cs492-stevens.edu` as a public Docker Hub repository before
announcing it to students, then verify that the pull command works without
authentication.

Course documentation should pin the published digest reported by Buildx, for
example `rtsang1/cs492-stevens.edu@sha256:...`. Version tags and base-image
tags can move; a digest is the immutable image identity. Do not make a graded
environment depend only on `latest`.

## Multi-Platform Build

The container Justfile manages a reusable Buildx builder named
`minemu-multiarch`. Docker Desktop includes the required architecture emulation.
On a native Linux Docker Engine host, install QEMU/binfmt support first by
following Docker's
[multi-platform build documentation](https://docs.docker.com/build/building/multi-platform/).

Build `linux/amd64` and `linux/arm64` without publishing them:

```sh
just --justfile container/justfile multiarch VERSION
```

This exports a multi-platform OCI archive to
`container/build/cs492-stevens.edu.oci.tar`. Override the destination or
platform list when needed:

```sh
ARCHIVE=/tmp/cs492-stevens.edu.oci.tar \
PLATFORMS=linux/amd64,linux/arm64 \
just --justfile container/justfile multiarch VERSION
```

Docker's classic local image store cannot load a multi-platform image as one
tag, which is why this recipe uses an OCI archive. The existing `build` and
`smoke` recipes remain the fast native-platform workflow.

### Manual Publication

Run the local smoke and repository checks before publishing:

```sh
just ci
just --justfile container/justfile smoke
```

Authenticate to Docker Hub as `rtsang1` with a narrowly scoped access token,
then build and push both supported architectures:

```sh
docker login --username rtsang1
just --justfile container/justfile publish VERSION
```

The recipe pushes one manifest-list tag containing `linux/amd64` and
`linux/arm64`, then runs `docker buildx imagetools inspect` for that tag. Record
the multi-architecture digest from the inspection output. Do not put the Docker
Hub access token in the command line, Dockerfile, repository, or image.

Override the destination repository for a fork or another registry:

```sh
REPOSITORY=OWNER/REPOSITORY \
just --justfile container/justfile publish VERSION
```

## Dev Container

`container/devcontainer.json` is the canonical Dev Container configuration. The
Dev Container CLI can use it directly:

```sh
devcontainer up \
  --workspace-folder . \
  --config container/devcontainer.json
devcontainer exec \
  --workspace-folder . \
  --config container/devcontainer.json \
  zsh -l
```

Editors that require `.devcontainer/devcontainer.json` can use the same settings
while referencing `container/Dockerfile` with the repository root as build
context.
The configuration runs as `dev`, updates that user's UID for the host, and keeps
OpenCode configuration and authentication in named volumes rather than image
layers.

## Student Repository Check

Inside the container, a `minimum` checkout uses its normal build and test
workflow:

```sh
make clean
make
make image
just test-all hw1
```

The HW1 Just workflow is tracked separately in the HW1 release plan and must
exist before that homework goes live.

## Credentials And Host Access

The image definition does not add API keys, Git credentials, SSH keys, or a
Docker socket. Its allowlist-style build context also excludes common credential
file patterns. Maintainers must still inspect the build context before release.
Pass required credentials at runtime using a provider's login flow, a secret
store, or a narrowly scoped read-only secret mount. Do not add credentials with
Dockerfile `ARG`, `ENV`, or `COPY`, because those values may remain in image
layers or metadata.

The supplied shell and Dev Container commands map `host.docker.internal` to the
host gateway for optional local AI providers. Other `docker run` commands must
include the same `--add-host` option on Linux. The image does not expose or start
a provider by default.

See [AI-assisted development](ai-development.md) for the recommended OpenCode
setup.
