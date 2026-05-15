# syntax=docker/dockerfile:1

FROM rust:1.90-bookworm AS builder
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

# Cache dependency compilation (including the slow HiGHS C++ build).
# This layer is reused on source-only changes.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release && rm -rf src target/release/deps/ven_app*

# Build the actual application
COPY src ./src
COPY profiles ./profiles
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
