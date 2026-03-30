# Design Decisions

Living document of architectural choices and their rationale.

---

## Data is append-only and never dropped

Vocabulary and n-gram counts are living data that only grows. Once an n-gram crosses the admission threshold, it stays admitted forever. Once counts are inserted for a month, they stay. There is no mechanism to remove or recompute data — the full history of HN is the scope, and data accumulates monotonically.

This simplifies everything: no dedup logic on count tables, no reprocessing of old months, no versioning of vocabulary state. The `process` command is safe to re-run — in ClickHouse mode, the watermark ensures only new comments are processed. In Parquet mode, it starts from scratch every time.

A newly admitted n-gram won't have historical counts from months already backfilled; its chart starts from when it was first backfilled after admission. This is acceptable because n-grams interesting enough for trends cross thresholds quickly, and rare slow accumulators don't produce meaningful charts.

---

## Pruning thresholds are configurable via environment

N-gram pruning thresholds are loaded from environment variables (`PRUNE_MIN_{N}GRAM_GLOBAL`, `PRUNE_MIN_{N}GRAM_BUCKET`), falling back to coded defaults. Thresholds are keyed by n-gram order, not hardcoded per bigram/trigram.

High thresholds (e.g., 500) speed up dev testing by producing a small vocabulary. Production uses lower thresholds (20/10) for completeness. The n-keyed design supports future 4-gram/5-gram support without code changes.

Defaults: bigram global=20, trigram global=10, bigram bucket=3, trigram bucket=5.

---

## ClickHouse table engine choices

`ngram_counts` and `bucket_totals` use plain `MergeTree`. With watermark tracking, comments are never processed twice, so there's no need for dedup. `ngram_vocabulary` and `global_counts` use `ReplacingMergeTree` because they represent unique entities that get updated over time — vocabulary entries with updated `global_count`, and global counts that grow with each incremental run. Eventual consistency via background merges is acceptable everywhere.

---

## Dev runs Rust locally, Docker only for ClickHouse

In development, only ClickHouse runs in Docker (`docker compose up -d`). The API server and ingestion pipeline run as local `cargo run` processes via `process-compose` or `just`.

Local Rust builds are faster to iterate on, support debuggers, and avoid the Docker build cache invalidation dance. The `Dockerfile` and `docker-compose.prod.yml` exist for production deployment where everything runs in containers.

---

## `.env` loaded via dotenvy in all Rust entry points

Both the API server and ingestion CLI load `.env` via `dotenvy::dotenv().ok()` at startup. `cargo run` doesn't source `.env` files. The `.ok()` means missing `.env` is silently ignored (fine for prod/CI where env vars are set externally).

---

## Early ClickHouse connection check before CPU work

The `process` (ClickHouse mode) and `import` commands ping ClickHouse immediately after creating the client, before any processing. Without this, the pipeline would spend minutes tokenizing data only to fail on the first insert because ClickHouse is unreachable.

---

## API returns single phrase per request

The API accepts one phrase per request. The frontend makes parallel requests (one per phrase) for independent caching. Single-phrase requests are independently cacheable by CDN/browser, simpler to reason about, and the parallel requests are fast enough. See RFC-007-optimizations section 7.

---

## Watermark-based incremental ingestion

The `process` command (ClickHouse mode) reads a watermark from the `ingestion_log` table: the timestamp of the newest comment processed. Each run reads parquet files and skips comments with `time <= watermark`, processing only new data. This supports any update cadence — daily, hourly, or monthly — with the same command. All state on prod lives in ClickHouse — no local manifest files.

The HN dataset is archived and immutable. Timestamps are monotonic. Data is never corrected retroactively. These properties make a simple watermark sufficient — no checksums, row offsets, or dedup logic needed.

---

## Sharded binary merge for count aggregation

The ingestion pipeline accumulates n-gram counts in memory and periodically flushes them to partial files. These partials must later be merged (summing counts for the same key across flushes) to produce the final output.

The original approach used sorted TSV files with a single-threaded k-way heap merge. This was correct but slow: ~100GB of partial data on a 32-core machine used only 1 core, and TSV parsing added overhead.

The current approach uses **sharded binary partials**. At flush time, each entry is routed to shard `hash(key) % N` and written in a compact binary format (length-prefixed strings + fixed-width integers). At merge time, each shard is processed independently in parallel via rayon — read all files for that shard into a HashMap and sum counts. Since shards are disjoint by key, no cross-shard coordination is needed.

Additionally, only one partial file type (counts) is written. Globals (corpus-wide totals per n-gram) and bucket totals (denominators) are derived from counts during the merge phase, since the sharded approach holds each shard in memory anyway. This eliminated 2 of the original 3 partial file types.

The key insight enabling parallelism is that the merged output does not need to be sorted — it feeds into HashMap-based vocabulary building and Parquet writers that accept entries in any order. The original k-way merge was single-threaded precisely because it maintained sorted order; dropping that requirement unlocked embarrassingly parallel merging.

Correctness depends on deterministic hash-based sharding: `hash(NgramKey) % N` routes every occurrence of a given n-gram to the same shard number, regardless of which flush produced it. This guarantees that summing counts within a shard produces the correct global total for every key — no entries are split across shards.

Key properties:
- **N-way parallelism** during merge (default N = CPU count)
- **Binary format** avoids TSV parsing overhead
- **Deterministic hash sharding** ensures all occurrences of a key land in the same shard
- **Unsorted output** is what enables parallel independent processing per shard
- **Single file type** simplifies the pipeline and reduces I/O by ~60%

---

## ClickHouse client uses HTTP, not native protocol

The `clickhouse` Rust crate communicates over ClickHouse's HTTP interface (port 8123), not the native TCP protocol (port 9000). The native protocol is faster (binary framing, no HTTP overhead, supports multi-statement), but the only mature Rust crate for it (`clickhouse-rs`) is less actively maintained.

One consequence: the HTTP API only accepts a single SQL statement per request. Multi-statement operations (like re-running a schema file) require splitting on `;` and sending each statement individually. The `reset-db` command handles this by stripping comments and splitting the schema SQL before executing.

For bulk data import, we bypass the `clickhouse` crate entirely and POST Parquet files directly to the HTTP API (`INSERT INTO table FORMAT Parquet`). ClickHouse's native Parquet reader handles deserialization server-side, avoiding the overhead of Rust → RowBinary serialization. Large files are chunked into batches (read with Arrow, re-encoded as small Parquet buffers) to bound ClickHouse's memory usage during import.

---

## `time` crate everywhere, never `chrono`

All date/time handling uses the `time` crate. The `clickhouse` crate's serde helpers use `time::Date`. Mixing crates creates conversion friction and subtle bugs.

When binding `time::Date` values in ClickHouse queries, format as `"YYYY-MM-DD"` strings. The default serde for `time::Date` serializes as a tuple, which ClickHouse rejects.
