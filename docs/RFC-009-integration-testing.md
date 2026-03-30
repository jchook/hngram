# RFC-009: Integration Testing

## Status: Draft

## Problem

Unit tests cover tokenization and counting logic, but there are no integration tests verifying the full path from ingest through ClickHouse queries to API responses. Bugs like the `time::Date` bind serialization issue (dates sent as tuples instead of strings) can only be caught by hitting a real database.

## Goals

- Catch schema/query mismatches before they reach production
- Test the ingest → query round-trip with real ClickHouse
- Keep unit tests fast and dependency-free
- Integration tests should be easy to run locally and in CI

## Approach: Shared Test Database

Use the existing `docker compose` ClickHouse instance. Each test run gets an isolated database to avoid interference.

### Why not testcontainers?

The `testcontainers` crate spins up a fresh Docker container per test suite. This adds 5-10s startup overhead and requires Docker-in-Docker for CI. Since we already run ClickHouse via compose, a shared instance with per-run isolation is simpler and faster.

### Test isolation strategy

1. Each test run creates a randomly-named database (e.g., `hn_ngram_test_a1b2c3`)
2. Run the schema from `etc/clickhouse/init/001-schema.sql` against it
3. Tests use an `HnClickHouse` client pointed at the test database
4. Tear down the database after the test run (or on next run if it crashed)

### Directory structure

```
server/
├── tests/
│   ├── common/
│   │   └── mod.rs          # Test harness: create/drop DB, build client
│   ├── ingest_test.rs   # Ingest a small fixture, verify counts
│   └── api_test.rs         # Query endpoints against seeded data
```

### Test harness sketch

```rust
// tests/common/mod.rs
pub struct TestDb {
    pub ch: HnClickHouse,
    db_name: String,
}

impl TestDb {
    pub async fn new() -> Self {
        let db_name = format!("hn_ngram_test_{}", random_hex(6));
        let admin = HnClickHouse::new("http://localhost:8123", "default", "", "default");
        // CREATE DATABASE, run schema
        Self { ch, db_name }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // DROP DATABASE (best-effort, async drop is tricky — use a cleanup pass)
    }
}
```

### Running tests

```bash
# Unit tests only (no DB required)
cargo test --workspace

# Integration tests (requires `docker compose up -d`)
cargo test --test '*' -- --ignored
```

Integration tests are marked `#[ignore]` by default so `cargo test` stays fast. The justfile gets a dedicated recipe:

```just
# server/justfile
test-integration:
    cargo test --test '*' -- --ignored
```

### What to test

| Test | Verifies |
|------|----------|
| Insert + query round-trip | Schema matches Rust types, date serialization works |
| Vocabulary insert + check | ReplacingMergeTree dedup, vocabulary lookup |
| Aggregated query granularities | GROUP BY week/month/year produces correct buckets |
| API endpoint responses | HTTP layer, JSON serialization, error codes |
| Empty result handling | No panic on zero rows |

### Fixtures

A small synthetic fixture (10-20 comments across 2-3 days) is preferred over real Parquet files. This keeps tests fast and deterministic. The fixture feeds directly into `NgramCounter` rather than going through Parquet I/O.

### CI considerations

- CI runs `docker compose up -d` before integration tests
- A GitHub Actions service container for ClickHouse is an alternative
- Stale test databases older than 1 hour are cleaned up at the start of each run

## Open questions

- Should API tests use `axum::test` (in-process) or spawn a real server and hit it with HTTP?
- Do we need test coverage for the ingest Parquet reading path, or is that sufficiently covered by the round-trip test with synthetic data?
