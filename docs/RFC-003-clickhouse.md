# RFC-003 (Agent-Oriented)

## ClickHouse Schema, Partitioning, and Query Model

## Status

**Implemented**
- Schema: `server/etc/clickhouse/init/001-schema.sql`
- Config: `server/etc/clickhouse/config.xml`, `users.xml`
- Rust client: `server/crates/clickhouse/src/lib.rs` (`hn-clickhouse` crate)

---

## 0. Scope

Define:

* ClickHouse table schemas
* partitioning strategy
* ordering keys
* query patterns
* insert model

Assumes:

* RFC-001 (tokenization)
* RFC-002 (n-gram generation + pruning)

---

# 1. Core Query Model (Anchor)

## Spec (mandatory)

Primary query:

* input:

  * `tokenizer_version`
  * `n ∈ {1,2,3}`
  * list of `ngram` (size ≤ 10)
  * date range

* output:

  * time series of relative frequency per `(bucket, ngram)`

---

## Rationale

All schema and indexing decisions must optimize for:

> **lookup of small sets of ngrams across large time ranges**

NOT:

* scanning all ngrams in a time window
* aggregating raw data at query time

---

## Flexibility

* Agent may propose alternate schema **only if** it preserves:

  * O(#ngrams × #buckets) scan behavior
  * no full-corpus scans for typical queries

---

## Non-goals

* arbitrary analytics queries
* full-text search
* heavy aggregations at query time

---

# 2. Table: `ngram_counts`

## Spec (mandatory)

```sql
CREATE TABLE ngram_counts (
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    bucket Date,
    count UInt32
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tokenizer_version, n, ngram, bucket)
SETTINGS index_granularity = 8192;
```

---

## Field Semantics

* `tokenizer_version`

  * must match tokenizer used during ingestion
  * required for correctness across tokenizer changes

* `n`

  * allowed values: 1, 2, 3

* `ngram`

  * space-separated tokens
  * must match tokenizer output exactly

* `bucket`

  * UTC date
  * represents one day

* `count`

  * total occurrences of ngram in bucket
  * aggregated before insert

---

## Constraints

* no duplicate rows for `(tokenizer_version, n, ngram, bucket)`
* no per-occurrence inserts
* only aggregated data allowed

---

## Rationale

* Pre-aggregation ensures query-time simplicity (no GROUP BY required)
* Ordering key supports direct lookup of ngrams across time

---

## Flexibility

* `count` type may be increased (e.g., UInt64) if overflow risk is demonstrated
* compression settings may be tuned
* index granularity may be tuned

---

## Non-goals

* storing raw comments
* storing per-comment counts

---

# 3. Table: `bucket_totals`

## Spec (mandatory)

```sql
CREATE TABLE bucket_totals (
    tokenizer_version LowCardinality(String),
    n UInt8,
    bucket Date,
    total_count UInt64
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tokenizer_version, n, bucket);
```

---

## Field Semantics

* `total_count`

  * total number of emitted n-grams of order `n`
  * computed before per-bucket pruning

---

## Constraints

* must include full denominator (unpruned)
* must align exactly with `ngram_counts` buckets

---

## Rationale

* required for correct normalization
* avoids recomputation at query time

---

## Flexibility

* agent may propose storing additional denominators (e.g., per-comment counts) if needed for new metrics

---

## Non-goals

* storing normalized values directly (v1)

---

# 4. Optional Table: `ngram_vocabulary`

## Spec

```sql
CREATE TABLE ngram_vocabulary (
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    global_count UInt64,
    admitted_at DateTime
)
ENGINE = ReplacingMergeTree(admitted_at)
ORDER BY (tokenizer_version, n, ngram);
```

---

## Rationale

* supports pruning logic (RFC-002)
* separates ingestion concerns from serving layer
* `ReplacingMergeTree` handles deduplication on re-ingestion (ClickHouse lacks `INSERT ... ON CONFLICT`)

---

## Flexibility

* may be omitted if ingestion system manages vocabulary externally

---

# 5. Partitioning Strategy

## Spec (mandatory)

```sql
PARTITION BY toYYYYMM(bucket)
```

---

## Rationale

* balances partition size and count
* supports efficient range scans
* avoids excessive small partitions

---

## Flexibility

* daily partitioning is explicitly disallowed (too many partitions)
* yearly partitioning is allowed only if data volume justifies it

---

# 6. Ordering Key

## Spec (mandatory)

```text
(tokenizer_version, n, ngram, bucket)
```

---

## Rationale

Optimizes for:

* equality filter on `(tokenizer_version, n, ngram)`
* sequential scan over `bucket`

Matches dominant query pattern.

---

## Flexibility

Agent may propose alternative ordering **only if**:

* lookup cost for specific ngrams remains low
* does not require scanning all ngrams in a bucket range

---

## Non-goals / Rejected Alternatives

Do NOT use:

```text
(bucket, n, ngram)
```

Reason:

* forces scanning all ngrams for time range queries

---

# 7. Query Model

## Spec (mandatory)

### N-gram counts query (no JOIN — RFC-007-optimizations §2)

```sql
SELECT bucket, ngram, count
FROM ngram_counts
WHERE
    tokenizer_version = ?
    AND n = ?
    AND ngram IN (...)
    AND bucket BETWEEN ? AND ?
ORDER BY bucket;
```

### Bucket totals query (separate endpoint, cached aggressively)

```sql
SELECT tokenizer_version, n, bucket, total_count
FROM bucket_totals
WHERE
    tokenizer_version = ?
    AND bucket BETWEEN ? AND ?
ORDER BY n, bucket;
```

Client computes `relative_frequency = count / total_count` using cached totals.

---

## Requirements

* must not require GROUP BY for base query
* must not require JOIN for base query (totals served separately)
* must return one row per `(bucket, ngram)`
* missing rows interpreted as zero by application layer

---

## Rationale

* pre-aggregation moves complexity to ingestion
* queries remain simple and fast
* separating totals eliminates JOIN overhead and enables aggressive caching

---

## Flexibility

* agent may introduce:

  * projections
  * materialized views
  * caching layers

Only if:

* they do not duplicate large amounts of data unnecessarily
* they preserve correctness

---

# 8. Derived Granularity

## Spec

Base storage: daily

Derived:

* week → `toStartOfWeek`
* month → `toStartOfMonth`
* year → `toStartOfYear`

---

## Aggregation Query

```sql
SELECT
    toStartOfMonth(c.bucket) AS bucket,
    c.ngram,
    sum(c.count) / sum(t.total_count) AS rel_freq
FROM ...
GROUP BY bucket, c.ngram
ORDER BY bucket;
```

---

## Rationale

* avoids storing redundant aggregates
* preserves flexibility

---

## Flexibility

* agent may precompute aggregates if performance demands it

---

# 9. Insert Model

## Spec (mandatory)

* inserts must be batched
* data must be pre-aggregated
* no duplicate rows for same key

---

## Rationale

* avoids write amplification
* aligns with ClickHouse design

---

## Flexibility

* batch size may be tuned
* ingestion concurrency may be tuned

---

## Non-goals

* row-by-row inserts
* deduplication inside ClickHouse

---

# 10. Idempotency

## Spec

System must ensure:

* no duplicate insertion for same `(tokenizer_version, n, ngram, bucket)`

---

## Rationale

* ClickHouse does not enforce uniqueness
* duplicate rows will corrupt counts

---

## Flexibility

* agent may implement:

  * upstream deduplication
  * staging tables
  * checksum-based validation

---

# 11. Performance Model

## Spec

Expected query complexity:

```text
O(#ngrams × #buckets)
```

---

## Rationale

* ensures predictable performance
* avoids dependence on corpus size

---

## Acceptance Requirement

* typical query latency <200ms

---

# 12. Low-Memory Configuration (Required for Small VPS)

## Spec (mandatory for 2-4GB RAM hosts)

ClickHouse must be configured for low-memory operation.

### Required settings

```xml
<clickhouse>
  <!-- Limit total server memory usage -->
  <max_server_memory_usage_to_ram_ratio>0.6</max_server_memory_usage_to_ram_ratio>

  <!-- Limit per-query memory (500MB) -->
  <max_memory_usage>500000000</max_memory_usage>

  <!-- Limit memory for all queries combined (1GB) -->
  <max_memory_usage_for_all_queries>1000000000</max_memory_usage_for_all_queries>

  <!-- Reduce background merge memory -->
  <background_pool_size>2</background_pool_size>
  <background_schedule_pool_size>2</background_schedule_pool_size>

  <!-- Limit mark cache (default 5GB is too high) -->
  <mark_cache_size>134217728</mark_cache_size> <!-- 128MB -->

  <!-- Limit uncompressed cache -->
  <uncompressed_cache_size>67108864</uncompressed_cache_size> <!-- 64MB -->
</clickhouse>
```

### User-level settings (in users.xml or via SET)

```xml
<profiles>
  <default>
    <max_memory_usage>500000000</max_memory_usage>
    <max_bytes_before_external_group_by>200000000</max_bytes_before_external_group_by>
    <max_bytes_before_external_sort>200000000</max_bytes_before_external_sort>
  </default>
</profiles>
```

---

## Rationale

* default ClickHouse settings assume large servers
* on 2-4GB VPS, defaults will cause OOM or swap thrashing
* these settings trade latency for memory safety
* pre-aggregated data means queries are lightweight anyway

---

## Flexibility

* values may be tuned based on actual workload
* if upgrading to 8GB+ host, these limits can be relaxed

---

# 13. Prohibited Designs

Agent must NOT implement:

* raw comment storage in ClickHouse
* wide tables (column per ngram)
* partitioning by ngram
* ORDER BY starting with `bucket`
* precomputed normalized tables (v1)

---

# 14. Versioning

## Spec

* all tables include `tokenizer_version`
* queries must filter by `tokenizer_version`

---

## Rationale

* tokenizer changes invalidate data

---

# 15. Acceptance Criteria

System is valid if:

* queries return correct normalized values
* performance meets latency targets
* storage remains bounded
* ingestion produces no duplicates
* schema aligns with query pattern

---

## Final Note for Agent

If proposing changes:

* must preserve query pattern efficiency
* must not introduce full scans over all ngrams
* must maintain correctness of normalization

---

# 16. Implementation Notes

## Rust `hn-clickhouse` crate

Located at `server/crates/clickhouse/`, provides:

### Row types (match schema exactly)
```rust
pub struct NgramCountRow { tokenizer_version, n, ngram, bucket, count }
pub struct BucketTotalRow { tokenizer_version, n, bucket, total_count }
pub struct NgramVocabularyRow { tokenizer_version, n, ngram, global_count, admitted_at }
```

### Client wrapper
```rust
pub struct HnClickHouse {
    // Insert operations (for ingestion)
    pub async fn insert_ngram_counts(&self, rows: &[NgramCountRow])
    pub async fn insert_bucket_totals(&self, rows: &[BucketTotalRow])
    pub async fn insert_vocabulary(&self, rows: &[NgramVocabularyRow])

    // Query operations (for API)
    pub async fn query_ngrams(&self, n, ngrams, start, end) -> Vec<NgramQueryResult>
    pub async fn query_ngrams_aggregated(&self, n, ngrams, start, end, granularity)
    pub async fn check_vocabulary(&self, ngrams) -> Vec<bool>
}
```

### Granularity enum
```rust
pub enum Granularity { Day, Week, Month, Year }
```

Uses `time` crate (not `chrono`) to match `clickhouse` crate's serde helpers.

All queries use parameterized binding (no SQL injection risk).

