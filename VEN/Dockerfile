# syntax=docker/dockerfile:1

# Must track VEN/rust-toolchain.toml: `openleadr-wire` sets an MSRV of 1.91, and a base
# image older than that fails the build with a version error rather than a type error.
FROM rust:1.91-bookworm AS builder
# The container mirrors the repo: /repo/VEN beside /repo/lab-core, so
# VEN/Cargo.toml's `path = "../lab-core"` resolves here exactly as it does on
# disk. Building from a flattened /app would leave that path pointing outside
# the image.
WORKDIR /repo/VEN

# HiGHS bindings build native C++ code via cmake + bindgen (needs libclang).
# mold links the large ven-app binary several times faster than the default ld.
RUN apt-get update     && apt-get install -y --no-install-recommends         build-essential         cmake         pkg-config         libclang-dev         libssl-dev         mold     && rm -rf /var/lib/apt/lists/*
ENV RUSTFLAGS="-C link-arg=-fuse-ld=mold"

# The build context is the repo root, not VEN/, because `lab-core` is a path
# dependency living beside VEN/ -- the rules the VEN shares with the VTN's BFF
# (D-06). Everything below is therefore VEN/-prefixed, and the root
# .dockerignore keeps the context small.
COPY VEN/Cargo.toml VEN/Cargo.lock ./
COPY lab-core/Cargo.toml /repo/lab-core/Cargo.toml
COPY lab-core/src /repo/lab-core/src
COPY VEN/src ./src
COPY VEN/profiles ./profiles

# The cargo registry and `target/` live in BuildKit cache mounts, which survive
# between builds while image layers do not: cargo itself decides what is stale,
# so a source edit recompiles only the crates it touches, and a Cargo.lock change
# rebuilds only the dependencies that changed -- not HiGHS and every other crate,
# as a layer keyed on the lockfile would. The binary is copied out in the same
# step because a cache mount is not part of the image.
RUN --mount=type=cache,id=ven-cargo-registry,target=/usr/local/cargo/registry     --mount=type=cache,id=ven-cargo-git,target=/usr/local/cargo/git     --mount=type=cache,id=ven-target,target=/repo/VEN/target,sharing=locked     cargo build --release && cp target/release/ven-app /ven-app

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
COPY --from=builder --chown=nonroot:nonroot /ven-app /app/ven-app
RUN chmod 755 /app/ven-app

# Persistence volume mount point
RUN mkdir -p /data && chown nonroot:nonroot /data
VOLUME ["/data"]

# Config mount point for profile YAML
RUN mkdir -p /config && chown nonroot:nonroot /config

EXPOSE 8080
USER $user

ENTRYPOINT ["/app/ven-app"]
