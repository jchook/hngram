# RFC-010: Streaming Shard Merge for Memory-Constrained Builds

## Status

**Proposed** (motivated by full-corpus rebuild OOMs on a 30GB Hetzner box, May 2026)

## 0. Scope

Refactor the parquet-output merge phase so peak RAM is bounded by *one* shard's deduplicated hashmap rather than *all shards* held in a `Vec<HashMap>`. This is the difference between a build that needs 64GB+ RAM and one that runs comfortably on 16-32GB.

Touches:
* `server/crates/ingest/src/vocabulary.rs` — new streaming API
* `server/crates/ingest/src/process.rs` — `process_parquet`'s post-merge logic

Out of scope: the producer/consumer write path, the ClickHouse insert path, the tokenizer.

---

## 1. Problem

`process_parquet` calls `merge_shards_parallel(data_dir, num_shards) -> Vec<HashMap<NgramKey, u32>>`. Internally, each shard is read in parallel and aggregated into its own hashmap. The Vec holds all `num_shards` hashmaps **alive simultaneously** for the entire post-merge phase, which:

1. Iterates all shards to derive globals
2. Builds vocabulary from globals
3. Iterates all shards again to filter counts and write `ngram_counts.parquet`
4. Drops the Vec

For an HN-scale `ingest.full.toml` build (n=1..5, monthly buckets, low pruning), the deduplicated total comes to ~280–400M entries × ~250 bytes/entry ≈ 70–100GB of RSS. Even with 32GB RAM + 32GB swap, the kernel OOM-killed the process at 6/8 shards (`anon-rss:31GB, total-vm:70GB`).

Limiting `RAYON_NUM_THREADS` does not help: parallelism only affects *peak during merge*. The completed shard maps remain live regardless. The architectural issue is that `Vec<HashMap>` is the wrong return type when the consumer can stream.

## 2. Proposal

### 2.1 New streaming API

```rust
// vocabulary.rs

pub fn merge_shards_streaming<F>(
    data_dir: &Path,
    num_shards: usize,
    mut on_shard: F,
) -> anyhow::Result<()>
where
    F: FnMut(usize, HashMap<NgramKey, u32>) -> anyhow::Result<()>,
{
    for shard in 0..num_shards {
        let files = find_shard_files(data_dir, shard, num_shards)?;
        let mut map = HashMap::new();
        let mut bucket_intern = HashMap::new();
        for file in &files {
            read_shard_file(file, &mut map, &mut bucket_intern)?;
        }
        on_shard(shard, map)?;
        // map dropped here, before next shard begins
    }
    Ok(())
}
```

Sequential by design — the whole point is bounding RAM. Parallelism is sacrificed for survivability.

### 2.2 Two-pass aggregation in `process_parquet`

```text
Pass 1: compute globals
    for each shard:
        load shard hashmap
        for each (NgramKey, count):
            globals[(n, ngram)] += count
        drop shard hashmap

build vocabulary from globals       // existing logic
write global_counts.parquet         // existing logic

Pass 2: filter and write counts
    for each shard:
        load shard hashmap
        for each (NgramKey, count):
            totals[bucket_key] += count
            if admitted(n, ngram, count): write to ngram_counts.parquet
        drop shard hashmap

write bucket_totals.parquet         // existing logic
write ngram_vocabulary.parquet      // existing logic
```

### 2.3 Memory budget

Peak RAM differs between the two passes — globals is built in Pass 1 and consumed by `build_vocabulary` to produce `vocabulary`; only `vocabulary` survives into Pass 2. `globals` and `vocab_counts` can both be dropped at the Pass 1 / Pass 2 boundary.

**Pass 1 peak**: `globals_hashmap + one_shard_hashmap + writer_buffers`
**Pass 2 peak**: `vocabulary + one_shard_hashmap + totals_hashmap + writer_buffers`

For a full-profile HN build:
* `globals_hashmap`: ~50–80M unique (n, ngram) × ~80B = 4–6GB
* `vocabulary` (Pass 2 only): admitted entries only, ~10–20M × ~50B = 0.5–1GB
* `one_shard_hashmap`: ~30–50M unique entries × ~250B = 7–12GB
* `totals_hashmap` (Pass 2 only): one entry per (bucket, n) — for monthly × n=5, ~1200 entries, negligible
* Writer buffers: low hundreds of MB
* **Pass 1 peak: ~12–18GB. Pass 2 peak: ~8–13GB** (lower because vocabulary is much smaller than globals)

This fits comfortably on a 30GB box without swap.

### 2.4 Cleanup: remove `vocab_counts`

The existing `vocab_counts: HashMap<(u8, String), u64>` (`process.rs:553, 583-585`) exists only to assemble vocabulary rows for the parquet writer at the end. Its contents can be written inline during the globals iteration: when a `(n, ngram)` is being written to `global_counts.parquet` AND it's in `vocabulary`, also append it to `ngram_vocabulary.parquet`'s writer. This removes a multi-million-entry hashmap from the live set across the entire post-merge phase.

## 3. Trade-offs

* **Wall-clock cost**: two passes over partial files = double the disk read. For ~24GB of partials on a Hetzner SSD, that's ~30–60s extra per pass — negligible against the ~30 min the merge already takes.
* **Lost parallelism**: existing `into_par_iter()` over shards is dropped. Per-shard processing is already CPU-light (mostly `HashMap::insert`), so the wall-clock penalty is small. If we ever need to recover parallelism, the right knob is *parallel reads of partial files within a single shard* — but those are already sorted serially today and that's fine.
* **No callback failures**: the API uses `FnMut` returning `Result`, propagating errors up. Behaviour matches the existing `?` chain.

## 4. Alternatives Considered

### 4.1 Inline globals during source processing (the better long-term design)

Maintain a second aggregator in the consumer (`process.rs:412-451`) alongside the per-bucket counts: `globals: HashMap<(u8, String), u64>`, updated on every batch *before* `min_flush_count` pruning. Persist it as a sibling of `.complete`. At merge time, globals are already built — only one streaming pass over shards is needed (filter against vocabulary, write counts).

**Why this is strictly better than the two-pass approach:**

* **Speed**: halves merge wall-clock and disk I/O. This is the primary motivation.
* **Better long-tail coverage**: produces unpruned true globals. The current pipeline derives globals from post-flush-pruned shard data, which means a low-frequency long-tail ngram appearing once per batch but across many batches has its true global count under-counted by `min_flush`. With ~20M-entry batches, this is a narrow class of ngrams (rare-per-batch but accumulating-across-batches), so the practical signal recovery is modest — but real, and free if we're inline-aggregating anyway.
* **Cost** (the constraint moves, not eliminates): the ~4–6GB `globals` hashmap lives in the consumer concurrently with the bucket-keyed counts, the producer NgramCounters in flight, and rayon worker stacks. Source processing today is RAM-light, so this likely fits — but it raises the floor during processing, where the two-pass design pays only at merge time. On a 16GB box this could matter; on a 30GB box it's still fine.

**Why we are NOT doing it for this rebuild:**

We already have 24GB of post-flush-pruned partial files on disk and a valid `.complete` marker, representing 7+ hours of source processing on a memory-constrained box. Adding inline globals to the consumer requires re-running source processing from scratch. The two-pass approach in §2 uses the existing partials as-is.

**Recommendation for follow-up RFC**: implement inline globals as RFC-013 (or fold it into the next ingest-pipeline change). It's strictly faster, gives better data, and replaces Pass 1 of this RFC entirely.

### 4.2 Bigger box, smaller shards

Rent a 64GB box for the merge. Doesn't compose: the merge memory grows with corpus + n range. Future profiles (e.g. n=6 or daily granularity) blow past 64GB the same way.

### 4.3 Limit `RAYON_NUM_THREADS=1`

Tried. Makes the merge sequential but `Vec<HashMap>` still holds all completed shards simultaneously. Same OOM, just slower.

### 4.4 Lower `merge_shards`

Reducing num_shards just makes each shard bigger. Peak RAM doesn't change — it just shifts between "many small maps" and "few big maps." And shard count is fixed at write-time, so changing it requires re-processing.

### 4.5 External merge sort

Sort each shard's binary output on `(n, ngram, bucket)` at write time, then heap-merge across shards at read time. The merge phase becomes a streaming pipeline with O(num_shards) live entries — no per-shard hashmap at all, and globals fall out of grouped iteration over the heap. Bounds RAM to a small read buffer regardless of corpus.

This is a real refactor — the existing partial format and `write_sharded` would change — but not a huge one, maybe 1–2 days of work. Worth considering for the next ingest-pipeline overhaul, especially if combined with §4.1's inline globals (which would make external sort even simpler since vocabulary admission can happen inline).

## 5. Implementation Notes

* `merge_shards_streaming` is added; `merge_shards_parallel` is kept around for **one PR cycle as `#[cfg(test)]` only**, used solely by the validation harness (§6.2) to assert byte-for-byte equivalence on synthetic data. Once the new merge has run end-to-end on real data and produced output that imports cleanly to ClickHouse, the old function is removed in a follow-up. This avoids the §5/§6 chicken-and-egg of "removed the thing we want to compare against."
* The two-pass pattern reads partial files twice. Each pass is a clean iteration; no mid-pass state to carry between them. `globals` lives across both passes (built in Pass 1, consumed in Pass 2's vocabulary check via the precomputed `vocabulary` HashMap).
* **Resumability is unchanged**: the `.complete` marker still gates source processing, which is the expensive phase. If Pass 2 crashes, only Passes 1 + 2 are redone — both of which are cheap enough (~10–20 min combined on the full corpus) that re-running on retry is not worth designing around. No new resume markers needed.
* **Bucket interning is per-shard**, same as today: `bucket_intern: HashMap<String, Arc<str>>` is created fresh inside `read_shard_file`'s caller scope, so a bucket date seen in shard 0 and shard 1 gets re-allocated. With ~240 monthly buckets × 8 shards × ~10 chars = a few thousand string allocations across the full merge — not a regression and not worth optimizing.
* No changes to the binary partial file format. The on-disk layout from existing builds is reused as-is.

## 6. Validation

1. Unit tests in `vocabulary.rs` for `merge_shards_streaming` (small synthetic data, deterministic input → deterministic output).
2. **Byte-for-byte invariant**: for the same partial-dir input and identical `PruningConfig`, the new merge must produce the same `total_count_rows`, `total_total_rows`, and `total_gc_rows` as the old `merge_shards_parallel`. Output parquet row counts should match exactly. (Set up by running the old merge to completion on a small synthetic dataset, then running the new merge on the same partials and diffing parquet metadata.)
3. End-to-end: re-run the in-flight HN full build using the existing 24GB partial dir. Should complete on the 30GB box without swap pressure.
4. Sanity: compare admitted bigrams and trigrams against the dev dataset — same shape, larger volume due to lower thresholds.

## 7. Rollout

This is an internal refactor of the ingest binary. No schema changes, no API changes, no client changes. Ship in a single PR with a test, validate against the existing partial dir on `hningest`, then resume the rebuild.
