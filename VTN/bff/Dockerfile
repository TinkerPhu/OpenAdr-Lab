FROM rust:1.90-alpine AS base
RUN apk add --no-cache alpine-sdk openssl-dev openssl-libs-static

FROM base AS builder
WORKDIR /app

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release && rm -rf src target/release/deps/vtn_bff*

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
COPY --from=builder --chown=nonroot:nonroot /app/target/release/vtn-bff /app/vtn-bff
RUN chmod 755 /app/vtn-bff

EXPOSE 8090
USER $user

ENTRYPOINT ["/app/vtn-bff"]
