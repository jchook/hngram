# Single-Pass Processing with Three Partial Streams

## Context

The current two-pass architecture re-reads and re-tokenizes every source file twice — once for vocabulary (pass 1) and once for filtered counts (pass 2). Pass 2 also OOMs because it builds a full NgramCounter per file before filtering.

The key insight: the expensive work is reading and tokenizing source comments. Sequential merges over sorted partials on disk are cheap (sequential I/O, bounded memory). So we replace "two source-data passes" with "one source-data pass + multiple merge passes."

## Architecture

### Source processing (single pass)

Producers read source files in row-group batches and send NgramCounter data through a bounded channel. The consumer derives three sorted partial streams from the same data and flushes all three together when the global count accumulator hits the cardinality threshold.

**What the consumer accumulates (in memory):**
- `globals: HashMap<(u8, String), u64>` — for vocabulary admission (same as current)
- `counts: HashMap<NgramKey, u32>` — per-bucket n-gram counts
- `totals: HashMap<BucketKey, u64>` — per-bucket denominators

**On flush (when `globals.len() >= max_ngrams`):**
Write three sorted files atomically:
- `NNN.globals` — sorted by (n, ngram)
- `NNN.counts` — sorted by (bucket, n, ngram)
- `NNN.totals` — sorted by (bucket, n)

Then `clear()` all three accumulators (retaining capacity).

**Memory model:**
- globals: bounded by `--max-ngrams`
- counts: proportional to globals × avg number of unique buckets per n-gram. For a 10M globals threshold, counts could be 10-50x larger depending on how many days each n-gram appears in. This is the main memory concern.
- totals: tiny (at most ~days × 3 n-values)

**Important**: the counts accumulator can be much larger than globals. If this is a problem, we can flush more aggressively (e.g., flush when counts.len() exceeds a separate threshold, or when globals.len() exceeds threshold/10). For now, flushing on globals threshold is simplest.

### Merge phase (three sequential merges)

After source processing completes:

**1. Merge globals → build vocabulary**
- K-way merge all `.globals` files
- For each unique (n, ngram, total_count): check admission thresholds
- Build admitted vocabulary HashSet (only admitted entries in memory)
- Also write `global_counts.parquet` incrementally via streaming writer

**2. Merge counts → filter and write `ngram_counts.parquet`**
- K-way merge all `.counts` files
- For each unique (bucket, n, ngram, count): check if (n, ngram) is in admitted vocabulary
- If admitted and meets per-bucket threshold: write to `ngram_counts.parquet`
- If not admitted: skip

**3. Merge totals → write `bucket_totals.parquet`**
- K-way merge all `.totals` files
- For each unique (bucket, n, total): write to `bucket_totals.parquet`
- No filtering — totals are always included

These three merges are independent and could run sequentially or in parallel. Sequential is simplest and fine for I/O-bound work.

### Dependency graph

```
source files → partials (.globals, .counts, .totals)
                  ↓
            merge .globals → vocabulary (HashMap in memory)
                  ↓
            merge .counts + vocabulary → ngram_counts.parquet (filtered)

            merge .totals → bucket_totals.parquet (no filtering, independent)
```

### Partial file formats (TSV)

All sorted for k-way merge compatibility.

`.globals`: `n\tngram\tcount\n` — sorted by (n, ngram)
`.counts`: `bucket\tn\tngram\tcount\n` — sorted by (bucket, n, ngram)
`.totals`: `bucket\tn\ttotal\n` — sorted by (bucket, n)

### k-way merge implementation

Reusable `KWayMerge` struct that opens N sorted files and yields lines in sorted order via a BinaryHeap. Each specific merge (globals, counts, totals) wraps this with type-specific parsing and aggregation of duplicate keys.

The merge identifies duplicate keys by comparing the key prefix of each line (everything before the last `\t`). Lines with the same key prefix are summed.

### Files to modify

| File | Change |
|------|--------|
| `vocabulary.rs` | Three write functions (globals, counts, totals), three merge functions, reusable KWayMerge, file discovery by extension |
| `process.rs` | Single-pass producer-consumer with three accumulators, three sequential merges, remove pass 2 entirely |
| `parquet.rs` | `stream_counters()` replaces `stream_global_counts()` — sends full NgramCounter per batch |

### What stays the same

- Producer-consumer pattern with bounded channel and backpressure
- `--max-ngrams` threshold on globals accumulator
- `--producer-count` for concurrent workers
- Atomic `.complete` marker for resume
- ClickHouse mode (unaffected)
- Parquet writing helpers (streaming writers for output files)

### Disk space consideration

`.counts` partials will be significantly larger than `.globals` — each unique (bucket, n, ngram) vs just (n, ngram). For the full corpus this could be tens of GB of intermediate files. This is acceptable for a one-time bootstrap on a workstation with an SSD.

### Vocabulary representation during counts merge

The admitted vocabulary is a `HashMap<(u8, String), ()>` loaded in memory during the counts merge. This is the same structure used throughout the codebase. For the full corpus this is the set of admitted bigrams + trigrams — bounded by pruning thresholds, typically a few million entries at ~50 bytes each = a few hundred MB. Well within reason.

## Verification

1. `cargo check -p ingestion` — compiles
2. `cargo test -p ingestion` — existing tests pass
3. Manual: `process --output parquet --start 2024-01 --end 2024-03` — produces output/ with 5 parquet files
4. Manual: check partial/ has `.globals`, `.counts`, `.totals` files
5. Manual: verify output counts match expected values for a known month
6. Memory: monitor RSS during processing — should not exceed globals threshold + proportional counts
