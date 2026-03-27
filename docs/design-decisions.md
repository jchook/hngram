# Design Decisions

Living document of architectural choices and their rationale.

---

## Data is append-only and never dropped

Vocabulary and n-gram counts are living data that only grows. Once an n-gram crosses the admission threshold, it stays admitted forever. Once counts are inserted for a month, they stay. There is no mechanism to remove or recompute data — the full history of HN is the scope, and data accumulates monotonically.

This simplifies everything: no dedup logic on count tables, no reprocessing of old months, no versioning of vocabulary state. The `vocabulary` command is safe to re-run at any time — it generates partial counts only for new months, merges all partials, and inserts any newly admitted n-grams. The `backfill` command only processes months not yet in the manifest.

A newly admitted n-gram won't have historical counts from months already backfilled; its chart starts from when it was first backfilled after admission. This is acceptable because n-grams interesting enough for trends cross thresholds quickly, and rare slow accumulators don't produce meaningful charts.

---

## Pruning thresholds are configurable via environment

N-gram pruning thresholds are loaded from environment variables (`PRUNE_MIN_{N}GRAM_GLOBAL`, `PRUNE_MIN_{N}GRAM_BUCKET`), falling back to coded defaults. Thresholds are keyed by n-gram order, not hardcoded per bigram/trigram.

High thresholds (e.g., 500) speed up dev testing by producing a small vocabulary. Production uses lower thresholds (20/10) for completeness. The n-keyed design supports future 4-gram/5-gram support without code changes.

Defaults: bigram global=20, trigram global=10, bigram bucket=3, trigram bucket=5.

---

## ClickHouse counts tables use plain MergeTree

`ngram_counts` and `bucket_totals` use `MergeTree`, not `ReplacingMergeTree`. Only `ngram_vocabulary` uses `ReplacingMergeTree`.

With manifest tracking, a month is never inserted twice. There's no need for dedup on count tables. Vocabulary uses `ReplacingMergeTree` because it's rebuilt from scratch on each run — the `admitted_at` column serves as the version for dedup.

If the manifest is deleted and months are re-backfilled, counts will be duplicated. A manifest reset implies a full rebuild (delete counts first).

---

## Dev runs Rust locally, Docker only for ClickHouse

In development, only ClickHouse runs in Docker (`docker compose up -d`). The API server and ingestion pipeline run as local `cargo run` processes via `process-compose` or `just`.

Local Rust builds are faster to iterate on, support debuggers, and avoid the Docker build cache invalidation dance. The `Dockerfile` and `docker-compose.prod.yml` exist for production deployment where everything runs in containers.

---

## `.env` loaded via dotenvy in all Rust entry points

Both the API server and ingestion CLI load `.env` via `dotenvy::dotenv().ok()` at startup. `cargo run` doesn't source `.env` files. The `.ok()` means missing `.env` is silently ignored (fine for prod/CI where env vars are set externally).

---

## Early ClickHouse connection check before CPU work

The `vocabulary` and `backfill` commands ping ClickHouse immediately after creating the client, before any Parquet processing. Without this, the pipeline would spend minutes tokenizing data only to fail on the first insert because ClickHouse is unreachable.

---

## API returns single phrase per request

The API accepts one phrase per request. The frontend makes parallel requests (one per phrase) for independent caching. Single-phrase requests are independently cacheable by CDN/browser, simpler to reason about, and the parallel requests are fast enough. See RFC-007-optimizations section 7.

---

## `time` crate everywhere, never `chrono`

All date/time handling uses the `time` crate. The `clickhouse` crate's serde helpers use `time::Date`. Mixing crates creates conversion friction and subtle bugs.

When binding `time::Date` values in ClickHouse queries, format as `"YYYY-MM-DD"` strings. The default serde for `time::Date` serializes as a tuple, which ClickHouse rejects.
