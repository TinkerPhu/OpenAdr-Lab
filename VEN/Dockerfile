FROM rust:1.90-alpine AS base
RUN apk add --no-cache alpine-sdk openssl-dev openssl-libs-static cmake g++

FROM base AS builder
WORKDIR /app

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release && rm -rf src target/release/deps/ven_app*

# Build the actual application
COPY src ./src
RUN cargo build --release

FROM alpine:latest
RUN apk add --no-cache ca-certificates curl

ARG user=nonroot
ARG group=nonroot
ARG uid=2000
ARG gid=2000
RUN addgroup -g ${gid} ${group} && \
    adduser -u ${uid} -G ${group} -s /bin/sh -D ${user}

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
