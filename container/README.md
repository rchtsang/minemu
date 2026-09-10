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
they are omitted from the final student image.

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

## Planned Published Image

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
build the image locally with the supplied Just recipe before mounting a writable
workspace. The optional Dev Container workflow can instead update the remote
user automatically. The `:Z` mount option permits access on SELinux-enforcing
hosts.

Configure `rtsang1/cs492-stevens.edu` as a public Docker Hub repository before
announcing it to students, then verify that the pull command works without
authentication.

Course documentation should pin the published digest reported by Buildx, for
example `rtsang1/cs492-stevens.edu@sha256:...`. Version tags and base-image
tags can move; a digest is the immutable image identity. Do not make a graded
environment depend only on `latest`.

## Multi-Platform Publication

The container Justfile manages a reusable Buildx builder named
`minemu-multiarch`. Release images are built separately on native hosts rather
than compiling Rust and Unicorn through QEMU. Build ARM64 on Apple Silicon and
AMD64 on an x86-64 Windows or Linux host.

Run the local smoke and repository checks before publishing:

```sh
just ci
just --justfile container/justfile smoke
```

### Architecture Images

Authenticate to Docker Hub as `rtsang1` with a narrowly scoped access token on
each build host:

```sh
docker login --username rtsang1
```

On the Apple Silicon host, build and push the ARM64 image:

```sh
just --justfile container/justfile publish-platform arm64
```

On the x86-64 host, build and push the AMD64 image:

```sh
just --justfile container/justfile publish-platform amd64
```

These commands derive the version from the `minemu` package using
`cargo metadata` and push architecture-specific tags such as `:0.1.1-arm64`
and `:0.1.1-amd64`. The existing `build` and `smoke` recipes remain the fast
native-platform workflow.

### Manifest Publication

After both architecture-specific tags have been pushed, run this command from
either host to create the multi-platform version tag:

```sh
just --justfile container/justfile publish
```

The recipe verifies both source tags, creates one manifest-list tag containing
`linux/amd64` and `linux/arm64`, then inspects the result. Record the
multi-architecture digest from the inspection output. Do not put the Docker Hub
access token in the command line, Dockerfile, repository, or image.

Override the destination repository for a fork or another registry:

```sh
REPOSITORY=OWNER/REPOSITORY \
just --justfile container/justfile publish-platform arm64
REPOSITORY=OWNER/REPOSITORY \
just --justfile container/justfile publish-platform amd64
REPOSITORY=OWNER/REPOSITORY \
just --justfile container/justfile publish
```

## Optional Dev Container

Dev Container tooling is not required to build, run, or publish the image.
Editors and users that prefer it may use `container/devcontainer.json` through
the Dev Container CLI:

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

The HW1 Just workflow is implemented in the starter and course repositories and
has been validated against the completed course implementation.

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
