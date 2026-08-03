FROM rust:1.96.0-bookworm

ARG DEV_UID=1000
ARG DEV_GID=1000

# docker desktop or intervening proxy can serve stale apt index data
# preserve package signature/hash verification; request fresh, non-pipelined
#   index downloads instead of disabling validation
RUN printf '%s\n' \
      'Acquire::http::No-Cache "true";' \
      'Acquire::https::No-Cache "true";' \
      'Acquire::http::Pipeline-Depth "0";' \
      'Acquire::https::Pipeline-depth "0";' \
      'Acquire::Retries "3";' \
      'Acquire::By-Hash "force";' \
      > /etc/apt/apt.conf.d/99minemu-network \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* \
    && apt-get update \
    && apt-get install --yes --no-install-recommends \
      binutils-arm-none-eabi \
      gcc-arm-none-eabi \
      build-essential \
      ca-certificates \
      pkg-config \
      zsh \
      clang \
      cmake \
      make \
      file \
      git \
      less \
      libclang-dev \
      libglib2.0-dev \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/*

RUN cargo install just --version 1.57.0 --locked

RUN groupadd \
      --gid "${DEV_GID}" dev \
    && useradd \
      --uid "${DEV_UID}" \
      --gid dev \
      --create-home \
      --shell /usr/bin/zsh \
      dev \
    && mkdir -p /home/dev/.cargo \
    && chown -R dev:dev /home/dev

COPY --chown=dev:dev home/dev /home/dev

ENV HOME=/home/dev

USER dev
WORKDIR /workspace

SHELL ["/usr/bin/zsh", "-c"]
CMD ["zsh", "-l"]

