# syntax=docker/dockerfile:1

FROM rust:1.96.0-bookworm AS minemu-builder

ARG JUST_VERSION=1.57.0

# Avoid stale apt indexes from Docker Desktop or an intervening proxy.
RUN printf '%s\n' \
      'Acquire::http::No-Cache "true";' \
      'Acquire::https::No-Cache "true";' \
      'Acquire::http::Pipeline-Depth "0";' \
      'Acquire::https::Pipeline-Depth "0";' \
      'Acquire::Retries "3";' \
      'Acquire::By-Hash "force";' \
      > /etc/apt/apt.conf.d/99minemu-network \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* \
    && apt-get update \
    && apt-get install --yes --no-install-recommends \
      build-essential \
      ca-certificates \
      clang \
      cmake \
      libclang-dev \
      libglib2.0-dev \
      pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates

RUN cargo build --locked --release -p minemu \
    && cargo install just --version "${JUST_VERSION}" --locked \
    && strip target/release/minemu

FROM node:22-bookworm-slim AS opencode-builder

ARG OPENCODE_VERSION=1.18.29

RUN npm install --global "opencode-ai@${OPENCODE_VERSION}" \
    && npm cache clean --force

FROM debian:bookworm-slim AS student

ARG DEV_UID=1000
ARG DEV_GID=1000

LABEL org.opencontainers.image.source="https://github.com/rchtsang/minemu"

# Each stage needs its own Docker Desktop/proxy-safe apt configuration.
RUN printf '%s\n' \
      'Acquire::http::No-Cache "true";' \
      'Acquire::https::No-Cache "true";' \
      'Acquire::http::Pipeline-Depth "0";' \
      'Acquire::https::Pipeline-Depth "0";' \
      'Acquire::Retries "3";' \
      'Acquire::By-Hash "force";' \
      > /etc/apt/apt.conf.d/99minemu-network \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* \
    && apt-get update \
    && apt-get install --yes --no-install-recommends \
      binutils-arm-none-eabi \
      build-essential \
      ca-certificates \
      curl \
      file \
      gcc-arm-none-eabi \
      gdb-multiarch \
      git \
      jq \
      less \
      make \
      nano \
      openssh-client \
      python3 \
      ripgrep \
      zsh \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid "${DEV_GID}" dev \
    && useradd \
      --uid "${DEV_UID}" \
      --gid dev \
      --create-home \
      --shell /usr/bin/zsh \
      dev \
    && mkdir -p \
      /home/dev/.config/opencode \
      /home/dev/.local/share/opencode \
      /workspace \
    && chown -R dev:dev /home/dev /workspace

COPY --from=minemu-builder /src/target/release/minemu /usr/local/bin/minemu
COPY --from=minemu-builder /usr/local/cargo/bin/just /usr/local/bin/just
COPY --from=opencode-builder /usr/local/lib/node_modules/opencode-ai /opt/opencode
RUN ln -s /opt/opencode/bin/opencode.exe /usr/local/bin/opencode

COPY --chown=dev:dev container/home/dev/zshenv /home/dev/.zshenv
COPY --chown=dev:dev container/home/dev/zshrc /home/dev/.zshrc
COPY --chown=dev:dev . /opt/minemu

ENV HOME=/home/dev \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

USER dev
WORKDIR /workspace

CMD ["zsh", "-l"]
