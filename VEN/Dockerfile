# syntax=docker/dockerfile:1

# Must track VEN/rust-toolchain.toml: `openleadr-wire` sets an MSRV of 1.91, and a base
# image older than that fails the build with a version error rather than a type error.
FROM rust:1.91-bookworm AS builder
WORKDIR /app

# HiGHS bindings build native C++ code via cmake + bindgen (needs libclang).
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        cmake \
        pkg-config \
        libclang-dev \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# The build context is the repo root, not VEN/, because `lab-core` is a path
# dependency living beside VEN/ -- the rules the VEN shares with the VTN's BFF
# (D-06). Everything below is therefore VEN/-prefixed, and the root
# .dockerignore keeps the context small.

# Cache dependency compilation (including the slow HiGHS C++ build).
# This layer is reused on source-only changes.
#
# `lab-core` is stubbed alongside the app: cargo needs a path dependency to
# exist to resolve the graph, but copying its *source* here would mean every
# edit to it rebuilt HiGHS from scratch. Its manifest rarely changes; its
# source changes land in the cheap layer below.
COPY VEN/Cargo.toml VEN/Cargo.lock ./
COPY lab-core/Cargo.toml ./lab-core/Cargo.toml
RUN mkdir -p src lab-core/src     && echo 'fn main() {}' > src/main.rs     && : > lab-core/src/lib.rs
RUN cargo build --release     && rm -rf src lab-core/src target/release/deps/ven_app* target/release/deps/lab_core*

# Build the actual application
COPY lab-core/src ./lab-core/src
COPY VEN/src ./src
COPY VEN/profiles ./profiles
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libgcc-s1 \
        libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

ARG user=nonroot
ARG group=nonroot
ARG uid=2000
ARG gid=2000
RUN groupadd -g ${gid} ${group} && \
    useradd -u ${uid} -g ${group} -s /bin/sh -M ${user}

WORKDIR /app
COPY --from=builder --chown=nonroot:nonroot /app/target/release/ven-app /app/ven-app
RUN chmod 755 /app/ven-app

# Persistence volume mount point
RUN mkdir -p /data && chown nonroot:nonroot /data
VOLUME ["/data"]

# Config mount point for profile YAML
RUN mkdir -p /config && chown nonroot:nonroot /config

EXPOSE 8080
USER $user

ENTRYPOINT ["/app/ven-app"]
