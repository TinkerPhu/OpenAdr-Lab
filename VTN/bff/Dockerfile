# 1.91, not 1.90: `lab-core` pulls in openleadr-wire, whose MSRV is 1.91, and
# an older base fails with a toolchain error rather than a type error. Must
# track the VEN's own Dockerfile, which depends on the same crate.
FROM rust:1.91-alpine AS base
RUN apk add --no-cache alpine-sdk openssl-dev openssl-libs-static

FROM base AS builder
# The container mirrors the repo: /repo/VTN/bff beside /repo/lab-core, so
# Cargo.toml's `path = "../../lab-core"` resolves here exactly as it does on
# disk. The build context is therefore the repo root, not VTN/bff.
WORKDIR /repo/VTN/bff

# Cache dependency compilation by building against stubs first. This layer is
# reused whenever only source changes.
COPY VTN/bff/Cargo.toml VTN/bff/Cargo.lock ./
COPY lab-core/Cargo.toml /repo/lab-core/Cargo.toml
RUN mkdir src && echo 'fn main() {}' > src/main.rs     && mkdir -p /repo/lab-core/src && echo '' > /repo/lab-core/src/lib.rs
RUN cargo build --release

# Build the actual application. The stub artifacts must go, including the
# `.rlib` -- deleting only `deps/lab_core*` once left a stale stub satisfying
# the dependency, and the failure surfaced much later than the cause.
COPY lab-core/src /repo/lab-core/src
COPY VTN/bff/src ./src
RUN cargo clean --release -p lab-core -p vtn-bff && cargo build --release

FROM alpine:latest
RUN apk add --no-cache ca-certificates curl

ARG user=nonroot
ARG group=nonroot
ARG uid=2000
ARG gid=2000
RUN addgroup -g ${gid} ${group} && \
    adduser -u ${uid} -G ${group} -s /bin/sh -D ${user}

WORKDIR /app
COPY --from=builder --chown=nonroot:nonroot /repo/VTN/bff/target/release/vtn-bff /app/vtn-bff
RUN chmod 755 /app/vtn-bff

EXPOSE 8090
USER $user

ENTRYPOINT ["/app/vtn-bff"]
