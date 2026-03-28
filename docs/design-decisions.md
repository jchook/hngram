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

## K-way sorted merge for global count aggregation

The ingestion pipeline's vocabulary build (pass 1) processes ~244 monthly Parquet files and needs to compute total counts for every unique n-gram across the entire corpus. The naive approach — merge all partial count files into one big HashMap — requires holding every unique (n, ngram) pair in memory. With ~50-100M unique trigrams at ~50 bytes each, that's 5-8 GB just for the HashMap.

Instead, partial count files are written sorted by (n, ngram), then merged using a k-way sorted merge with a BinaryHeap. This streams through all files simultaneously, yielding one unique (n, ngram, total_count) at a time. Memory during merge is O(num_files) — one buffered line per open file (~244 entries in the heap). The merged stream feeds directly into vocabulary admission decisions and writes `global_counts.parquet` incrementally, so the full global counts map never materializes in memory.

Alternatives considered:
- **RocksDB merge operator**: Good fit for incremental accumulation, but adds a heavy dependency for a one-time bootstrap step. RocksDB's merge operator is designed for exactly this kind of associative aggregation, but the external sort approach achieves the same result with zero dependencies.
- **In-memory HashMap**: Simpler code but 5-8 GB peak memory for the full corpus. Unacceptable on the target VPS and uncomfortable even on a workstation.
- **SQLite**: Simpler operationally but slower for bulk write-heavy aggregation. Would need batched UPSERTs in large transactions.

The k-way merge is the right tool because n-gram counting is fundamentally a bulk aggregation problem, not an online KV problem. Sequential I/O + sorted merge is both faster and more memory-efficient than random-access hash table operations.

---

## `time` crate everywhere, never `chrono`

All date/time handling uses the `time` crate. The `clickhouse` crate's serde helpers use `time::Date`. Mixing crates creates conversion friction and subtle bugs.

When binding `time::Date` values in ClickHouse queries, format as `"YYYY-MM-DD"` strings. The default serde for `time::Date` serializes as a tuple, which ClickHouse rejects.
