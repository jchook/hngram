# RFC-011: Parallel External-Sort Merge for Constant-Memory Ingest

## Status

**Proposed** (follow-on to RFC-010, which shipped the tactical fix in May 2026)

## 0. Scope

Restructure the ingest merge phase so peak RAM is **constant in corpus size** — bounded by per-shard heap + group-buffer + writer buffers, regardless of vocabulary cardinality. Combines the parallelism of today's sharded design with the bounded-RAM property of streaming sort.

Touches:
* `server/crates/ingest/src/vocabulary.rs` — sort-during-flush, sorted-run reader, per-shard k-way merge
* `server/crates/ingest/src/process.rs` — Phase 2 becomes a parallel per-shard streaming pipeline; `Vec<HashMap>` and the two-pass globals derivation both go away

Out of scope: the producer/consumer source-processing path, ClickHouse insert path, tokenizer.

---

## 1. Problem

RFC-010 bounded merge-phase RAM to *one shard's hashmap + globals*. That works at HN-scale today, but **both terms still scale with vocabulary cardinality**, which scales with corpus size. The next 2–3× of corpus growth (4-grams, 5-grams, daily granularity, expanding HN data through 2030) puts us back into OOM territory on the same 30GB box.

The structural cause is the shard key. `vocabulary.rs:187-191` hashes the *entire* `NgramKey { bucket, n, ngram }`:

```rust
fn shard_for_key(key: &NgramKey, num_shards: usize) -> usize {
    let mut hasher = std::hash::DefaultHasher::new();
    key.hash(&mut hasher);  // includes bucket
    (hasher.finish() as usize) % num_shards
}
```

Including `bucket` means the same `(n, ngram)` lives in different shards across its bucket variants. To compute a global count, we must aggregate across shards — which forces all shards to be live simultaneously (the original OOM) or two passes over disk (RFC-010).

If `(n, ngram)` were the shard key, every record for a given n-gram would cluster in one shard. Each shard would be **self-contained for globals**: globals, vocabulary admission, count emission, and partial bucket totals all derivable from that one shard alone, in a single streaming pass. Shards become embarrassingly parallel.

## 2. Proposal

Two changes to the existing pipeline. The architectural skeleton (producer/consumer source processing, sharded partial files, rayon-parallel merge) stays.

### 2.1 Shard by `(n, ngram)`, not full key

```rust
fn shard_for_key(key: &NgramKey, num_shards: usize) -> usize {
    let mut hasher = std::hash::DefaultHasher::new();
    (key.n, &*key.ngram).hash(&mut hasher);  // bucket excluded
    (hasher.finish() as usize) % num_shards
}
```

All buckets of a given `(n, ngram)` cluster in the same shard. Globals are per-shard derivable.

### 2.2 Sorted runs instead of unsorted records

When the consumer flushes its hashmap, instead of writing each entry to its routed shard file as raw records, it:

1. Drains the hashmap to a `Vec<(NgramKey, u32)>`.
2. Partitions the Vec into N buckets by `hash(n, ngram) % N`.
3. Sorts each bucket by `(n, ngram, bucket)` *in parallel* (rayon over partitions).
4. Writes each sorted bucket as a single run file: `partial/NNNN.sNN`.

The on-disk binary record format is **unchanged** — same length-prefixed strings, same field order. Only the in-file ordering is now sorted, and the shard-routing rule changed.

### 2.3 Phase 2: parallel per-shard streaming merge

```text
for each shard in parallel (rayon):
    open all run files for this shard as sorted iterators
    heap = min-heap of (key, run_idx) for each run head
    current_group = (n, ngram) | None
    group_records = Vec<(bucket, count)>
    shard_totals: HashMap<(bucket, n), u64>

    while heap not empty:
        (key, run_idx, count) = heap.pop()
        if (key.n, key.ngram) != current_group:
            flush_group(group_records, current_group)
            current_group = (key.n, key.ngram)
            group_records.clear()
        group_records.push((key.bucket, count))
        advance run_idx; push next head if any

    flush_group(group_records, current_group)  // final group
    write shard_totals to bucket_totals.shard_NN.parquet

flush_group(records, (n, ngram)):
    global = sum(r.count for r in records)
    write_to(global_counts.shard_NN.parquet, n, ngram, global)
    if admitted(n, ngram, global):
        write_to(ngram_vocabulary.shard_NN.parquet, n, ngram, global)
        for (bucket, count) in records:
            shard_totals[(bucket, n)] += count
            if count >= min_bucket_count(n):
                write_to(ngram_counts.shard_NN.parquet, n, ngram, bucket, count)
    else:
        for (bucket, count) in records:
            shard_totals[(bucket, n)] += count  // totals are unpruned
```

Phase 3 (cheap): merge the N per-shard `bucket_totals` accumulators (each ~1200 entries) into a single `bucket_totals.parquet`. Optionally concatenate the other per-shard parquets into single files; ClickHouse can also import the directory directly.

### 2.4 Memory budget

Per worker (one shard, one rayon thread):

* Heap of run heads: O(num_runs_in_shard) — for HN-scale, ~3 entries
* `group_records`: bounded by buckets × 1 (single n,ngram) — monthly ≈ 228, daily ≈ 7000 — call it ~70KB
* `shard_totals`: O(buckets × n_orders) ≈ 1200 entries
* Parquet writer buffers: low hundreds of MB

Across N parallel workers: `N × ~500MB ≈ 4GB` for N=8.

**Bounded constant in corpus size.** Doubling the corpus changes nothing.

Want tighter RAM? Set `--merge-shards 1` and process one shard at a time: peak ~500MB. Want max throughput on a big box? `--merge-shards = num_cores`. Same code, different knob.

### 2.5 Disk budget

The architecture itself adds negligible disk overhead — sorting reorders bytes within a run but doesn't add any. Per-shard parquet outputs cost a few MB of metadata vs single-file outputs (footers, schemas, row-group statistics × N files × 4 file types). Trivial against multi-GB outputs. No temp files; sort happens in-RAM during flush, heap-merge streams runs without buffering.

Steady-state partial-file size is dominated by **`min_flush_count`**, the per-flush-batch pruning threshold (`process.rs:433, 455`). It drops `(bucket, n, ngram)` entries whose count *within a single flush batch* falls below `min_flush_count(n)`. N-gram counts follow a Zipf distribution — most n-grams appear once per batch — so the choice of threshold dominates partial-file volume.

#### Production stance: `min_flush_count ≤ 2`

For the canonical full rebuild, prefer `min_flush_count = 1` (effectively off) or `2`. Reasons:

* **Data quality.** Pruning at flush time is *destructive* — once an entry is dropped from a batch, the heap-merge in Phase 2 can never recover it. An n-gram that appears once per batch across 20 batches has true global=20 but post-flush-prune global=0. Higher thresholds destroy more of the long-tail signal.
* **RFC-011 makes large partials viable.** The OOM constraint that motivated aggressive flush pruning under the old sharded-merge design is gone. Constant-RAM merge means you can afford the disk cost of unpruned partials.
* **Disk is cheap; rebuilds are rare.** A canonical rebuild happens on tokenizer-version changes — every few months at most. Eating ~50–120GB of transient partials for one rebuild is a fine trade for better data.

#### Dev/test stance: higher thresholds for fast iteration

For local development and CI runs, raise `min_flush_count` (typical: 5–10, sometimes higher) to keep partials small. Iteration time matters more than rare-ngram fidelity when you're debugging tokenizer changes or testing pipeline behavior. Document the tradeoff explicitly: dev partials are *intentionally lossy* in the long tail.

#### Quantitative breakdown for HN-scale

| `min_flush_count` | Partial-file size | Use case |
|---|---|---|
| 1 (off) | ~50–120 GB | Canonical production build, max data quality |
| 2 | ~30–60 GB | Production with mild long-tail filter |
| ~5 (today's effective default) | ~24 GB | Dev iteration on full data |
| 10+ | ~10–20 GB | Fast smoke tests, CI |

Numbers are estimates from Zipf scaling of the current ~24GB run; validate empirically on the first canonical rebuild under RFC-011.

#### What this means for sizing the ingest box

A production ingest box for canonical rebuilds should provision:

* **RAM**: ~4–8GB (RFC-011's constant bound, plus headroom for the OS and tokenization phase)
* **Disk**: ~150–200GB free during the build, of which ~120GB is transient partials and the rest is source data + parquet outputs + headroom. Cleanup of `partial/` after a successful import returns most of it.

This is roughly the same disk budget as today (current Hetzner box has plenty); the change is that we can now use it without OOMing.

## 3. Trade-offs

* **Sort overhead at flush time.** Sorting an in-RAM `Vec<(NgramKey, u32)>` of ~2.5M entries (one partition out of 20M, with N=8) is O(N log N) comparison-sort, parallelized across partitions. Estimated 1–2s wall-clock per flush, ~20–60s total over a full HN ingest. <1% overhead.
* **No new RAM cost during flush.** The data is already in the consumer's HashMap; sort drains it to a Vec and sorts in place. Peak RAM during flush is unchanged from today.
* **Existing partials become unusable.** The shard-routing rule changes, so RFC-010-era partials cannot be reused. A full re-tokenization is required (~7 hours for HN-scale, one-time).
* **Heap-merge in Phase 2 is single-threaded *per shard*** but parallel *across shards*. For HN-scale (~3 runs per shard, ~50M records per shard), per-shard merge is ~5–10s wall-clock. Total Phase 2 wall-clock with 8 shards in parallel: maybe 30–60s, dominated by parquet writer flushes, not the merge logic.
* **Per-shard output files.** Each shard writes its own slice of each parquet. Three options at end-of-phase: (a) leave as-is and import the directory — ClickHouse handles multiple files; (b) concatenate via Arrow's parquet concat (cheap, no re-encoding); (c) write into a single shared writer with a mutex (serializes writers, defeats parallelism). Recommend (a); fall back to (b) if any consumer needs a single file.
* **Lost: load-balancing smoothing from bucket-in-hash.** Phase 2 wall-clock = `max(shard_time)`. With `hash(bucket, n, ngram)`, hot ngrams like `the` had their ~228 monthly bucket-records routed independently — each shard saw ~1/N of every hot key's records. With `(n, ngram)`-only sharding, each hot key clusters entirely on one shard, and per-shard load is determined by *which* hot keys land where.

  Loads balance in expectation (each shard gets ~1/N of unique keys, including ~1/N of the hot ones), but the tail variance increases because the law-of-large-numbers smoothing across 228 independent routings is gone. Statistical estimate: with ~100 hot keys driving the bulk of records, hot-keys-per-shard is roughly 12.5 ± 3.3 (1σ) for N=8, putting the worst shard at ~1.3–1.5× the mean. Total Phase 2 wall-clock penalty is on that order.

  Mitigations, in order of cost — none of them needed unless variance is actually observed:
  1. **Increase `num_shards`.** N=16 reduces relative variance by √2. Costs more output files and writer buffers; cheap to try.
  2. **Bin-pack assignment by record count.** Add a Phase-1.5 pass that tallies records-per-`(n, ngram)`, then assigns keys to shards greedily by load instead of by hash. Near-perfect balance; one extra pass over partial-file headers (cheap, no decode of records); needs a stored shard-assignment map.
  3. **Hybrid hot-key split.** For the top-K keys (K~10), revert to bucket-in-hash sharding so their records spread across shards; tail keys stay `(n, ngram)`-sharded for self-containedness. Most adaptive, most complex; defer unless (1) and (2) prove insufficient.

  A per-run **shard salt** (a u64 mixed into the hash) lets you re-roll the assignment without code changes, but it's a re-sample on the same distribution — same expected variance, just a different sample. Useful for one-off bad luck, not as a structural fix. Hash-function choice (`ahash` vs `DefaultHasher`) is *not* a fix — both distribute uniformly for non-adversarial input; ahash is only faster, which doesn't matter here.

## 4. Alternatives Considered

### 4.1 Stay with RFC-010

RFC-010 unblocked the immediate rebuild and is shipping today. Its Pass-1-then-Pass-2 design works at HN-scale, but peak RAM = `globals_hashmap + one_shard_hashmap`, both of which scale with corpus. At 2–3× current corpus we hit OOM again. RFC-010 is tactical; this is the structural follow-up.

### 4.2 Inline globals during source processing (RFC-010 §4.1)

Maintain a `globals: HashMap<(n, ngram), u64>` in the consumer alongside the bucket-keyed counts. At merge time globals are pre-built; only one streaming pass over shards is needed.

This is strictly better than RFC-010's two-pass design but still has a `globals` hashmap whose size scales with corpus. For HN-today fine; for HN-future, same OOM risk. The proposal in this RFC doesn't need a global aggregator at all — globals fall out of grouped iteration per-shard.

### 4.3 External merge sort without sharding (RFC-010 §4.5)

Sort runs globally (no shard routing), single heap-merge across all runs. Bounded RAM but **single-threaded**, which is exactly what dbac8c7 traded away for parallelism in March 2026. We'd be giving up the 8× speedup we got from `into_par_iter` over shards.

This RFC's design = (4.3) + sharding by `(n, ngram)`, which restores parallelism without sacrificing the bounded-RAM property.

### 4.4 Bigger box

Rent a 64GB box for the merge. Doesn't compose: vocabulary grows with corpus + n-range, and future profiles (n=6, daily granularity, larger HN data) blow past 64GB the same way RFC-010 noted. Not a structural fix.

## 5. Implementation Notes

* **Heap-merge skeleton can be lifted from `399f844:server/crates/ingest/src/vocabulary.rs`.** That commit's `BinaryHeap<Reverse<HeapEntry>>` + Ord/PartialOrd impl + pop-and-advance loop + drain-equal-keys pattern is exactly the structure we need. Update sort key to `(n, ngram, bucket)` (was `(n, ngram)`) and switch reader from TSV to the existing binary format. ~100 lines of adapted code.
* **Sort-during-flush** lives in `write_sharded` (vocabulary.rs:197-236). Replace the per-entry `writers[shard].write_all(...)` loop with: drain to Vec, `Vec::par_chunks_mut` partitioned by hash, parallel sort within each partition, write each partition sequentially. Reuses BufWriter-then-rename atomicity.
* **Run iterator** is a thin wrapper over the existing `read_shard_file` decode logic (vocabulary.rs:243-274), reshaped from "read into HashMap" to "yield records as iterator." Bucket interning per-run unchanged.
* **Phase 2 entry point** replaces `merge_shards_streaming` (RFC-010) with `process_shards_parallel`. Each shard's worker takes the data dir, shard index, vocabulary thresholds, and parquet writer paths; produces its slice of each output. No globals or vocabulary HashMap is passed in or out — those concepts disappear from the inter-shard interface.
* **Per-shard output paths**: `output/{global_counts,ngram_counts,bucket_totals,ngram_vocabulary}.shard_NN.parquet`. Phase 3 either concatenates or leaves them.
* **`min_flush_count` becomes a dev/test knob, not a production necessity.** See §2.5 for the full discussion. Default `≤ 2` for production canonical builds (preserves long-tail signal); higher values (5–10+) for fast dev iteration. The new design no longer *needs* aggressive flush pruning to fit in RAM — that constraint is what motivated the existing default, and it's gone.

## 6. Validation

1. **Unit tests in `vocabulary.rs`**: `sort_during_flush` produces sorted runs; per-shard heap merge produces correct grouped output on synthetic data with known globals.
2. **Memory profile invariant**: peak RSS during merge on a 2× synthetic corpus is within 10% of the 1× corpus. Repeat at 4×. If RAM scales with corpus, the design is wrong.
3. **Output equivalence vs RFC-010**: for the same source data and identical pruning config, total `ngram_counts`, `bucket_totals`, `global_counts`, and `ngram_vocabulary` row counts must match the RFC-010 design exactly. Per-row contents must match (modulo file ordering across shards). Run both pipelines on a small subset (one month) and diff.
4. **Wall-clock**: full HN ingest end-to-end should be within ±20% of the current RFC-010 ingest. Sort overhead is small; per-shard merge is fast; only risk is parquet-writer contention if N is high.
5. **Skew check**: histogram of records-per-shard *and* unique-keys-per-shard. Expected worst shard ~1.3–1.5× mean (see §3). If worst shard is >2× mean, apply mitigations from §3 in order: increase `num_shards`, then bin-pack assignment, then hot-key split. Re-rolling the shard salt is fine as a one-off; not a structural fix.

## 7. Rollout

Single PR, single re-tokenization. The full rebuild is the unit of work:

1. Land the code change (replaces shard-routing + Phase 2; old code paths deleted).
2. Wipe `data/partial/` and `data/output/` on `hningest`.
3. Re-run `cargo run -p ingest -- process --output parquet` from scratch (~7 hours).
4. Validate output against current ClickHouse: row counts and aggregate spot-checks.
5. Re-import to staging, atomic swap.

After this, the merge phase is structurally done. There's no follow-up RFC for ingest memory — the design is bounded constant in corpus size as long as disk capacity holds. Future ingest work (new pruning rules, different bucket granularities, new n-orders) is a tuning exercise, not a memory-architecture exercise.
