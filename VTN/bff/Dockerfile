# syntax=docker/dockerfile:1
# 1.91, not 1.90: `lab-core` pulls in openleadr-wire, whose MSRV is 1.91, and
# an older base fails with a toolchain error rather than a type error. Must
# track the VEN's own Dockerfile, which depends on the same crate.
FROM rust:1.91-alpine AS base
# mold links several times faster than the default ld.
RUN apk add --no-cache alpine-sdk openssl-dev openssl-libs-static mold
ENV RUSTFLAGS="-C link-arg=-fuse-ld=mold"

FROM base AS builder
# The container mirrors the repo: /repo/VTN/bff beside /repo/lab-core, so
# Cargo.toml's `path = "../../lab-core"` resolves here exactly as it does on
# disk. The build context is therefore the repo root, not VTN/bff.
WORKDIR /repo/VTN/bff

COPY VTN/bff/Cargo.toml VTN/bff/Cargo.lock ./
COPY lab-core/Cargo.toml /repo/lab-core/Cargo.toml
COPY lab-core/src /repo/lab-core/src
COPY VTN/bff/src ./src

# Registry and `target/` in BuildKit cache mounts (same reasoning as VEN/Dockerfile):
# cargo rebuilds only what changed. The binary is copied out in the same step
# because a cache mount is not part of the image.
# `release` for production; the E2E stack passes `ci` (see Cargo.toml).
ARG CARGO_PROFILE=release
RUN --mount=type=cache,id=bff-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=bff-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=bff-target,target=/repo/VTN/bff/target,sharing=locked \
    cargo build --profile "$CARGO_PROFILE" && cp "target/$CARGO_PROFILE/vtn-bff" /vtn-bff

FROM alpine:latest
RUN apk add --no-cache ca-certificates curl

ARG user=nonroot
ARG group=nonroot
ARG uid=2000
ARG gid=2000
RUN addgroup -g ${gid} ${group} && \
    adduser -u ${uid} -G ${group} -s /bin/sh -D ${user}

WORKDIR /app
COPY --from=builder --chown=nonroot:nonroot /vtn-bff /app/vtn-bff
RUN chmod 755 /app/vtn-bff

EXPOSE 8090
USER $user

ENTRYPOINT ["/app/vtn-bff"]
