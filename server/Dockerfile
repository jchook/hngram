# Build stage
FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev g++ pkgconf openssl-dev openssl-libs-static curl

WORKDIR /app

# Copy manifests and lock file for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/tokenizer/Cargo.toml crates/tokenizer/
COPY crates/clickhouse/Cargo.toml crates/clickhouse/
COPY crates/api/Cargo.toml crates/api/
COPY crates/ingest/Cargo.toml crates/ingest/

# Create dummy source files so cargo can resolve the workspace and cache deps
RUN mkdir -p crates/tokenizer/src crates/clickhouse/src \
             crates/api/src/bin crates/ingest/src && \
    echo "pub fn dummy() {}" > crates/tokenizer/src/lib.rs && \
    echo "pub fn dummy() {}" > crates/clickhouse/src/lib.rs && \
    echo "pub fn dummy() {}" > crates/api/src/lib.rs && \
    echo "fn main() {}" > crates/api/src/main.rs && \
    echo "fn main() {}" > crates/api/src/bin/generate_openapi.rs && \
    echo "fn main() {}" > crates/ingest/src/main.rs

# Build dependencies only
RUN cargo build --release --package api --package ingest && \
    rm -rf crates/*/src

# Copy actual source and rebuild
COPY crates/ crates/
# Ingest crate compiles in the schema SQL via include_str!
COPY etc/clickhouse/init/ etc/clickhouse/init/
RUN touch crates/tokenizer/src/lib.rs crates/clickhouse/src/lib.rs \
          crates/api/src/lib.rs crates/api/src/main.rs \
          crates/ingest/src/main.rs && \
    cargo build --release --package api --package ingest

# API runtime stage
FROM alpine:3 AS api
RUN apk add --no-cache ca-certificates && adduser -D -u 1000 app
COPY --from=builder /app/target/release/api /app/api
USER 1000:1000
EXPOSE 3000
CMD ["/app/api"]

# Ingest runtime stage (CLI; invoked via `docker compose run --rm ingest ...`)
FROM alpine:3 AS ingest
RUN apk add --no-cache ca-certificates && adduser -D -u 1000 app
COPY --from=builder /app/target/release/ingest /app/ingest
USER 1000:1000
WORKDIR /data
ENTRYPOINT ["/app/ingest"]
