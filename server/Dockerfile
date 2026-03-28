# Build stage
FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev g++ pkgconf openssl-dev openssl-libs-static curl

WORKDIR /app

# Copy manifests and lock file for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/tokenizer/Cargo.toml crates/tokenizer/
COPY crates/clickhouse/Cargo.toml crates/clickhouse/
COPY crates/api/Cargo.toml crates/api/
COPY crates/ingestion/Cargo.toml crates/ingestion/

# Create dummy source files so cargo can resolve the workspace and cache deps
RUN mkdir -p crates/tokenizer/src crates/clickhouse/src \
             crates/api/src/bin crates/ingestion/src && \
    echo "pub fn dummy() {}" > crates/tokenizer/src/lib.rs && \
    echo "pub fn dummy() {}" > crates/clickhouse/src/lib.rs && \
    echo "pub fn dummy() {}" > crates/api/src/lib.rs && \
    echo "fn main() {}" > crates/api/src/main.rs && \
    echo "fn main() {}" > crates/api/src/bin/generate_openapi.rs && \
    echo "fn main() {}" > crates/ingestion/src/main.rs

# Build dependencies only
RUN cargo build --release --package api && \
    rm -rf crates/*/src

# Copy actual source and rebuild
COPY crates/ crates/
RUN touch crates/tokenizer/src/lib.rs crates/clickhouse/src/lib.rs \
          crates/api/src/lib.rs crates/api/src/main.rs && \
    cargo build --release --package api

# Runtime stage
FROM alpine:3
RUN apk add --no-cache ca-certificates && adduser -D -u 1000 app
COPY --from=builder /app/target/release/api /app/api
USER 1000:1000
EXPOSE 3000
CMD ["/app/api"]
