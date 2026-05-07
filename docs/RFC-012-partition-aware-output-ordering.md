# RFC-012: Partition-Aware Output Ordering for Import-Friendly Parquet

## Status

**Proposed** (motivated by 73,768-part ClickHouse fragmentation post-import on the May 2026 full-corpus build)

## 0. Scope

Make the ingest's final parquet outputs sorted in an order that matches ClickHouse's `PARTITION BY toYYYYMM(bucket)`. Eliminates the post-import OPTIMIZE phase, which currently takes hours.

Touches:
* `server/crates/ingest/src/process.rs` — final-output sort step before parquet write
* (Optional) `server/crates/ingest/src/import.rs` — alternative server-side ORDER BY on import

Out of scope: source-processing/merge memory bounds (RFC-010 covered the immediate fix; RFC-011 covers the structural follow-up).

---

## 1. Problem

The full-corpus rebuild output (`ingest.full.toml`, n=1..5, monthly buckets) lands as a 3.4GB parquet with 317M rows in **unsorted-by-partition order**. Records inside the parquet are emitted in the order shards yield them — effectively random with respect to `bucket`.

ClickHouse's schema partitions by `toYYYYMM(bucket)` (`server/etc/clickhouse/init/001-schema.sql`):

```sql
CREATE TABLE hn_ngram.ngram_counts
(
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    bucket Date,
    count UInt32
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tokenizer_version, n, ngram, bucket)
```

Import streams the parquet to CH's HTTP endpoint in 500K-row batches (`import.rs:102 IMPORT_BATCH_SIZE`). Each batch is randomly distributed across all 233 monthly partitions, so each insert call creates **one part per touched partition** = up to 233 parts per batch. Cumulative, post-merge: 635 batches × ~120 parts each = ~75k parts.

Observed on the May 2026 build: **73,768 parts on `ngram_counts`**, which:

* Burned ~1.5–2GB of CH's RAM in idle part-metadata (causing `MEMORY_LIMIT_EXCEEDED` even on `SELECT count()`)
* Produced 5+ minute query latencies even after OPTIMIZE made significant progress
* Required hours of `OPTIMIZE TABLE ... FINAL` post-import to consolidate
* On a 30GB ingest box with `background_pool_size=2`, the OPTIMIZE itself takes 1–3 hours

The root cause is a **sort-order mismatch**: the parquet is sorted (or unsorted) for one purpose (merge correctness), but ClickHouse needs *partition-first* ordering for efficient insert.

## 2. Proposal

### 2.1 Approach A: sort the parquet at write time (recommended)

In `process.rs`'s Phase 2, before writing each row to `ngram_counts.parquet`, ensure rows are emitted in `(toYYYYMM(bucket), n, ngram, bucket)` order. The output remains a single parquet file, just with a different row order. Schema unchanged.

**Why partition-first**: with rows grouped by partition (`toYYYYMM`), ClickHouse's 500K-row batches each touch 1–3 partitions instead of 233. Estimated post-import part count: ~635 batches × ~2 parts = **~1,300 parts** instead of 75k. CH's auto-merge consolidates that to a few hundred within minutes, not hours.

**Implementation, integrated with current Phase 2 (post-RFC-010):**

The current `process_parquet` Phase 2 streams over shards (each shard → HashMap → iterate → write). Each shard's HashMap holds millions of rows. We can sort each shard's emitted rows by `(toYYYYMM, n, ngram, bucket)` before writing, then merge across shards via a small heap of per-shard iterators.

Pseudocode:

```rust
// Phase 2 (revised)
for shard in 0..num_shards {
    let mut shard_rows: Vec<NgramCountRow> = collect-from-shard(shard, vocab, config)?;
    shard_rows.par_sort_unstable_by_key(|r| (toYYYYMM(r.bucket), r.n, r.ngram.clone(), r.bucket));
    write_to_temp_parquet(format!("ngram_counts.shard_{:03}.parquet", shard), &shard_rows)?;
    drop(shard_rows);
}

// Phase 3: heap-merge the shard parquets → final ngram_counts.parquet
let writers = ngram_counts_writer;
let iters = (0..num_shards).map(|s| open_parquet_sorted_iter(s)).collect();
let heap = BinaryHeap::from_iters(iters);  // ordered by (yyyymm, n, ngram, bucket)
while let Some((row, idx)) = heap.pop() {
    writer.write_one(row)?;
    if let Some(next) = iters[idx].next() { heap.push((next, idx)); }
}
writer.finish()?;
```

Memory: each shard sort is bounded by one shard's rows (already the RFC-010 budget). Heap merge in Phase 3 holds one row per shard = ~8 rows. Negligible additional RAM.

Disk: temporary per-shard parquets cost ~3.4GB cumulative (same as the final file). Cleaned up after Phase 3.

### 2.2 Approach B: server-side ORDER BY on import (alternative)

Skip the in-process sort. Instead, change `import.rs` to use a single `INSERT INTO ... SELECT * FROM file(...) ORDER BY toYYYYMM(bucket)` query instead of streamed POSTs. ClickHouse handles the sort internally.

```rust
// import.rs
let query = format!(
    "INSERT INTO {table} SELECT * FROM file('{path}', 'Parquet') ORDER BY toYYYYMM(bucket)",
    ...
);
```

**Pros**: zero changes to `process.rs`. ClickHouse handles the sort.

**Cons**:
* Single big INSERT vs streamed batches. ClickHouse buffers the entire 3.4GB sort in memory by default — **OOMs the 3.7GB prod box**. Would need `max_bytes_before_external_sort` set explicitly to spill to disk, plus `optimize_on_insert=1` or similar.
* The HTTP endpoint streaming pattern is mature and tested; this rewrites the import path from streaming POST to query-based.
* Server-side sort is harder to debug if it fails — failures mid-INSERT leave staging tables in inconsistent state.

**Recommend Approach A** unless we want to defer all changes to the import side.

### 2.3 Approach C: sort-during-merge in RFC-011 (combined)

If RFC-011 (parallel external-sort merge) lands, its heap-merge already produces output in `(n, ngram, bucket)` order per shard. But that's **not** partition-first; it groups by ngram, not by month.

To extend RFC-011 with partition-aware output: change the heap-merge sort key from `(n, ngram, bucket)` to `(toYYYYMM(bucket), n, ngram, bucket)`. This breaks RFC-011's "globals fall out of grouped iteration per-shard" property, because records for a given `(n, ngram)` are now scattered across monthly groups within a shard.

**Workable but more complex**: have RFC-011's heap merge do two passes:
1. First pass: sort by `(n, ngram, bucket)` to compute globals + admit vocabulary (RFC-011 as-designed)
2. Second pass: re-sort to `(toYYYYMM, n, ngram, bucket)` for output

This is essentially RFC-012's Approach A grafted onto RFC-011. **See §4 for compatibility analysis.**

## 3. Memory and Performance

### 3.1 Memory budget

Approach A adds no significant memory cost over RFC-010:
* Per-shard sort: in-place sort of one shard's HashMap-drained-Vec. Same RAM as the existing per-shard hashmap.
* Phase 3 heap merge: O(num_shards) rows live = trivial.

### 3.2 Wall-clock impact

Adding the per-shard sort step:
* Sort 50M-row Vec parallel-sort: ~5–10s per shard
* 8 shards sequentially: ~40–80s total (not parallel, since RFC-010 streams them one at a time)
* Phase 3 heap merge: bounded by parquet write throughput, ~30s

**Total added: ~1–2 minutes** to the merge phase wall-clock.

### 3.3 Wall-clock saved at import + post-import

* Eliminate the 1–3 hour OPTIMIZE phase entirely
* Reduce import wall-clock slightly (fewer parts to create per batch)
* Eliminate the immediate-post-import query-latency catastrophe (~5 min/query → ~1 sec/query)

**Net win: hours saved per rebuild.**

## 4. Compatibility with RFC-011

**No conflict at the architectural level** — they address different layers:

| Concern | RFC-011 | RFC-012 |
|---------|---------|---------|
| Source-processing flush behavior | Sort during flush by `(n, ngram, bucket)` | Unchanged |
| Shard routing | Hash by `(n, ngram)`, not full key | Unchanged |
| Merge-phase memory | Bounded constant in corpus size | Unchanged |
| Output sort order in parquet | `(n, ngram, bucket)` per shard | `(toYYYYMM(bucket), n, ngram, bucket)` final |
| Eliminates post-import OPTIMIZE | No | Yes |

**Stacking interaction**: if both RFCs land, RFC-012's per-shard sort step happens *after* RFC-011's heap-merge step. RFC-011 yields rows in `(n, ngram, bucket)` order per shard; RFC-012 re-sorts them by `(toYYYYMM, n, ngram, bucket)` before writing parquet. The re-sort cost (~5–10s per shard) is paid in either design.

If RFC-011 lands first, RFC-012 becomes a small follow-up: change one sort key + add the heap-merge across per-shard sorted parquets in Phase 3.

If RFC-012 lands first (without RFC-011), it stacks cleanly on RFC-010 with the in-place per-shard sort + phase-3 heap merge as described in §2.1.

**Recommendation**: ship RFC-012 first. It's a smaller, more isolated change and addresses an immediate, measured pain (75k parts post-import). RFC-011 can land later when corpus growth pressures merge memory; their changes don't overlap.

## 5. Trade-offs

* **Phase 2 wall-clock +1-2 min.** Sort cost is real but small.
* **Disk: +3.4GB transient** for per-shard parquets, cleaned up after Phase 3 heap-merge. The current 24GB partial-files dominate disk usage anyway.
* **Phase 3 (heap merge across shards) introduces a new pipeline stage.** It's structurally simple but adds code.
* **No new resumability concerns.** Phase 2 is already not-resumable today; this doesn't change that. RFC-011 §2.5's resumability story applies.
* **Validation requirement**: byte-for-byte parquet content equivalence (ignoring row order) before and after this change. Easy to assert with a row-count + sum-of-counts diff.

## 6. Alternatives Considered

### 6.1 Larger IMPORT_BATCH_SIZE

Bumping `IMPORT_BATCH_SIZE` from 500K → 5M rows would reduce the batch count by 10×, so part count drops from 75k → ~7.5k. Better but still bad. Each batch would also balloon to ~500MB in-memory, which the prod box's 3.7GB RAM can't comfortably handle during the parquet re-encode.

### 6.2 Larger `max_bytes_to_merge_at_max_space_in_pool`

Raising CH's merge ceiling from 1GB → 10GB would let auto-merge consolidate larger parts, reducing the eventual stable part count. But the **immediate-post-import** part count is unchanged, so the catastrophic query latency right after import remains. Auto-merge takes hours to catch up. Not a fix; just a different post-hoc OPTIMIZE.

### 6.3 ClickHouse `optimize_on_insert=1`

CH has a setting that auto-merges per insert. Sounds promising but in practice it just shifts the merge from background to inline, slowing each insert by 5–10× and not solving the partition-mismatch root cause.

### 6.4 Don't partition by month

Drop `PARTITION BY toYYYYMM(bucket)`. Eliminates the fragmentation problem entirely. But partitioning is what makes `DROP PARTITION` and TTL-based deletion possible, and those are part of the operational toolkit for a long-running ingest system. Defer this if we ever decide the partition is unnecessary; for now, keep it.

## 7. Implementation Notes

* `toYYYYMM(date)` in Rust is `(date.year() * 100 + date.month() as i32)` for sort-key purposes — no floating-point, just an integer.
* The per-shard sort happens after RFC-010's Pass 2 collects each shard's data. Reuses the existing memory budget.
* Per-shard parquet temp files live in `output/.tmp/` and are deleted on Phase 3 success or process exit.
* Phase 3 heap merge uses the existing `parquet_writer::NgramCountsWriter` for the final output; only the row source changes.
* No schema change; the existing schema's `ORDER BY (tokenizer_version, n, ngram, bucket)` continues to drive query performance via the primary key index. The output sort is just for *part-creation efficiency* during insert, not query-time efficiency.

## 8. Validation

1. **Unit tests**: synthetic small-shard data, verify sort-then-merge produces output sorted by `(toYYYYMM, n, ngram, bucket)`.
2. **Byte-for-byte invariant** vs RFC-010 output (ignoring row order): same `total_count_rows`, same per-(n, ngram, bucket) counts. Use a parquet diff tool or DuckDB query.
3. **End-to-end**: re-run the May 2026 full build with this change; verify post-import part count is ~hundreds, not ~75k.
4. **Query latency**: spot-check 10 phrases (mix of common and rare) immediately after import; latency should be under 1s without any OPTIMIZE.

## 9. Rollout

Single PR. Deploy alongside the next canonical rebuild — there's no benefit to running this on an old partial dir.

After landing, the post-import OPTIMIZE step in any deployment runbook can be removed. The hours saved compound across every rebuild going forward.
