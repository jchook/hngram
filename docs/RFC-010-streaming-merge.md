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

Peak RAM at any point in either pass:

```
peak ≈ globals_hashmap + one_shard_hashmap + writer_buffers + OS
```

For a full-profile HN build:
* `globals_hashmap`: ~50–80M unique (n, ngram) entries × ~80B = 4–6GB
* `one_shard_hashmap`: ~30–50M unique entries × ~250B = 7–12GB
* Writer buffers: low hundreds of MB
* **Total: ~12–18GB** vs the current ~70–100GB

This fits comfortably on a 30GB box without swap.

## 3. Trade-offs

* **Wall-clock cost**: two passes over partial files = double the disk read. For ~24GB of partials on a Hetzner SSD, that's ~30–60s extra per pass — negligible against the ~30 min the merge already takes.
* **Lost parallelism**: existing `into_par_iter()` over shards is dropped. Per-shard processing is already CPU-light (mostly `HashMap::insert`), so the wall-clock penalty is small. If we ever need to recover parallelism, the right knob is *parallel reads of partial files within a single shard* — but those are already sorted serially today and that's fine.
* **No callback failures**: the API uses `FnMut` returning `Result`, propagating errors up. Behaviour matches the existing `?` chain.

## 4. Alternatives Considered

### 4.1 Bigger box, smaller shards

Rent a 64GB box for the merge. Doesn't compose: the merge memory grows with corpus + n range. Future profiles (e.g. n=6 or daily granularity) blow past 64GB the same way.

### 4.2 Limit `RAYON_NUM_THREADS=1`

Tried. Makes the merge sequential but `Vec<HashMap>` still holds all completed shards simultaneously. Same OOM, just slower.

### 4.3 Lower `merge_shards`

Reducing num_shards just makes each shard bigger. Peak RAM doesn't change — it just shifts between "many small maps" and "few big maps." And shard count is fixed at write-time, so changing it requires re-processing.

### 4.4 External merge sort

Write each shard to a sorted binary temp file, then linear-merge by sorted (n, ngram, bucket) keys. Bounds RAM to a small read buffer regardless of corpus. **The right answer for any future corpus 10x bigger** — but a much bigger refactor. Out of scope here.

## 5. Implementation Notes

* `merge_shards_streaming` replaces `merge_shards_parallel` rather than living alongside it. There's only one caller and the old API is the source of the OOM.
* The two-pass pattern reads partial files twice. Each pass is a clean iteration; no mid-pass state to carry between them.
* `globals` lives across both passes. It's the only thing that survives shard hashmap drops within Pass 1.
* No changes to the binary partial file format. The on-disk layout from existing builds is reused.

## 6. Validation

1. Unit tests in `vocabulary.rs` for `merge_shards_streaming` (small synthetic data).
2. End-to-end: re-run the in-flight HN full build using the existing 24GB partial dir. Should complete on the 30GB box without swap pressure.
3. Compare row counts of resulting parquet files against the in-progress dev dataset: same shape, larger volume.

## 7. Rollout

This is an internal refactor of the ingest binary. No schema changes, no API changes, no client changes. Ship in a single PR with a test, validate against the existing partial dir on `hningest`, then resume the rebuild.
