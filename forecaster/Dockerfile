# syntax=docker/dockerfile:1

# The lab-forecaster: publishes the PV forecast of a site as retained MQTT messages
# (openspec change lab-forecaster, docs/architecture/forecast_publication.md). It needs the broker and
# the VEN profile file it is given; no database, no other service.
#
# Same toolchain floor as the VEN image: one Rust version for the whole repo.
FROM rust:1.91-bookworm AS builder
# The container mirrors the repo: /repo/forecaster beside /repo/lab-forecast, so the
# `path = "../lab-forecast"` dependency resolves here exactly as it does on disk.
WORKDIR /repo/forecaster

# The build context is the repo root (lab-forecast is a path dependency beside forecaster/).
COPY forecaster/Cargo.toml forecaster/Cargo.lock ./
COPY lab-forecast/Cargo.toml /repo/lab-forecast/Cargo.toml
COPY lab-forecast/src /repo/lab-forecast/src
COPY forecaster/src ./src

# Registry and `target/` live in BuildKit cache mounts, as in the VEN image; the binary is copied out in
# the same step because a cache mount is not part of the image. Touching the sources first makes cargo
# compile what it was given even when a cached COPY layer brought back older file times.
RUN --mount=type=cache,id=forecaster-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=forecaster-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=forecaster-target,target=/repo/forecaster/target,sharing=locked \
    find src /repo/lab-forecast/src -name '*.rs' -exec touch {} + \
    && cargo build --release && cp target/release/lab-forecaster /lab-forecaster

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgcc-s1 \
    && rm -rf /var/lib/apt/lists/*

ARG user=nonroot
ARG group=nonroot
ARG uid=2000
ARG gid=2000
RUN groupadd -g ${gid} ${group} && \
    useradd -u ${uid} -g ${group} -s /bin/sh -M ${user}

WORKDIR /app
COPY --from=builder --chown=nonroot:nonroot /lab-forecaster /app/lab-forecaster
RUN chmod 755 /app/lab-forecaster

# The VEN profile YAML that declares the PV arrays is mounted here, read-only.
RUN mkdir -p /config && chown nonroot:nonroot /config

USER $user
ENTRYPOINT ["/app/lab-forecaster"]
