# Cardinality-Based Partial Flush for Pass 1

## Context

Pass 1 currently writes one partial count file per monthly source file. This ties memory usage to the largest month's unique n-gram count (~1-2 GB for recent months). Problems:
- On a small VPS, even one large month may not fit
- If HN grows, a single month could exceed any reasonable limit
- On a workstation with plenty of RAM, we write too many small partials (244 files), increasing k in the k-way merge

Decoupling flush boundaries from months lets us adapt to available memory: fewer, larger partials on a beefy workstation; more, smaller partials on a constrained VPS.

## Approach

Replace the per-month partial write with a cardinality-limited accumulator that flushes when it gets too large.

### Pass 1 changes (process.rs)

Current flow (per month, concurrent):
1. Read month file → tokenize → extract global_counts HashMap → write partial → drop

New flow (bounded producers + serial consumer with backpressure):

**Producers** (small fixed count, e.g. 2):
- Process a source file in row-group batches, not all at once
- Each batch: read rows → tokenize → count n-grams → extract batch-level global counts
- Send batch-level global counts HashMap through channel (bounded capacity 1-2)
- **Backpressure**: producers block when channel is full — this bounds total memory to producer count × batch size + accumulator
- A producer never materializes the entire file's global counts — it streams chunks

**Consumer** (single, owns the accumulator):
- Receives batch-level global counts from channel
- Merges into running accumulator HashMap
- When accumulator cardinality exceeds limit: sort, flush to `partial/NNN.counts`, reset
- Partial file numbering: monotonically increasing counter (initialized by scanning `partial/` once at startup)
- After all producers finish: flush remaining accumulator

**Memory model:**
- Each producer holds at most one row-group batch's NgramCounter (~few MB)
- Channel capacity 1 — backpressure keeps at most 1 queued + 1 in-flight producer result
- Accumulator grows to working size, then plateaus (reuses capacity via `clear()`, no reallocation churn)
- Consumer checks flush threshold **during** merge of each incoming batch, not after — prevents overshoot when a single producer result would push past the limit
- Total peak memory ≈ `accumulator capacity` + `2 × producer batch size`

**`--max-ngrams` semantics:**
- A **flush threshold**, not a hard memory cap
- After first flush, the accumulator retains its allocated capacity for throughput (hot allocation)
- High-watermark behavior is intentional — documented, not surprising

Key differences from current approach:
- **Decoupled from months** — flush boundaries are based on cardinality, not file boundaries
- **Streaming within files** — producers don't need to hold an entire month in memory
- **Bounded memory** — tight backpressure + small channel + producer-side chunking
- **Numbered partial files** — `partial/000.counts`, `partial/001.counts`, etc.
- **Configurable limit** — `--max-ngrams` CLI flag, default 10M entries (~500MB at ~50 bytes/entry)

### Vocabulary module changes (vocabulary.rs)

- Remove `partial_path(data_dir, ym)` (month-based naming)
- `write_partial_counts` unchanged (takes path + HashMap)
- `merge_partial_counts_streaming` changes: instead of iterating months to find files, recursively glob `partial/**/*.counts`, **sort paths** before opening (deterministic merge order). Backwards-compatible — works with old `{YYYY-MM}.counts` naming, new `NNN.counts` naming, or any mix.
- Partial numbering: consumer scans `partial/` once at startup to find the highest existing number, then increments a local counter for each flush. No repeated directory scans.

### Resume logic

Current: skip months whose partial file exists.

New: atomic completion marker.
- After all source files are processed and the final accumulator is flushed, write `partial/.complete` atomically (write to `.complete.tmp`, rename)
- On startup: if `partial/.complete` exists, skip pass 1 entirely — partials are valid, go straight to merge
- If `partial/` exists but `.complete` does not: the set is incomplete — delete all partials and restart pass 1

This is simpler than trying to figure out "where did we leave off" in the accumulator.

### Why this is efficient

The accumulator is a `HashMap<(u8, String), u64>`. After processing each source file's global counts, we merge them in (`*accumulator.entry(key).or_insert(0) += count`) and check `accumulator.len()` — which is O(1) since HashMap stores its count internally. No traversal, no estimation.

When cardinality exceeds the limit, we sort the entries (O(n log n) on accumulated entries), flush to disk, and `clear()` the HashMap — memory is immediately reclaimed.

The same n-gram can appear in multiple partial files (it was in the accumulator before a flush AND appeared again in later source files). This is correct — the k-way merge sums counts for matching keys across all partials.

### CLI changes (main.rs)

Add to the Process subcommand:
```
--max-ngrams <N>       Flush threshold for accumulator cardinality [default: 10000000]
--producer-count <N>   Concurrent file processing workers [default: 2]
```

`--producer-count` controls how many source files are being read/tokenized concurrently. Default 2 is conservative — enough to overlap I/O with CPU while keeping memory bounded. Higher values increase throughput at the cost of more in-flight memory.

### Files to modify

| File | Change |
|------|--------|
| `server/crates/ingest/src/process.rs` | Pass 1: producer-consumer with backpressure, cardinality-based flush |
| `server/crates/ingest/src/parquet.rs` | Add row-group-level iteration for streaming global counts |
| `server/crates/ingest/src/vocabulary.rs` | Glob-based file discovery for merge, remove month-based naming |
| `server/crates/ingest/src/main.rs` | Add `--max-ngrams` CLI arg, pass to process |

### Parquet reader changes (parquet.rs)

Currently `process_comments_parallel` takes all comments and returns one NgramCounter. For streaming within files, we need the producer to iterate row groups from the Parquet reader, tokenize each batch, and send batch-level global counts through the channel. Two options:

**Option A**: New function `iter_row_group_counts(path)` that yields `HashMap<(u8, String), u64>` per row group. The producer calls this in a loop and sends each result through the channel.

**Option B**: Keep `process_comments_parallel` but call it on chunks of comments read per row group, not all at once.

Option A is cleaner — the producer reads one row group, filters comments, tokenizes via rayon, extracts global counts, sends. The Parquet reader already supports row-group-level iteration.

### What stays the same

- `write_partial_counts` function (sorts and writes a HashMap to TSV)
- `merge_partial_counts_streaming` k-way merge logic (just changes how it finds files)
- Pass 2 (concurrent producers, serial writer) — pass 2 needs the full NgramCounter per file for per-bucket counts, so it keeps the current approach
- ClickHouse mode (loads global counts from DB, not partials)

## Verification

1. `cargo check -p ingest` — compiles
2. `cargo test -p ingest` — existing tests pass
3. Manual: run `process --output parquet --start 2024-01 --end 2024-03` with default limit — check partial/ has fewer files than months
4. Manual: run with `--max-ngrams 100000` — check it produces more partial files
5. Check resume: kill mid-run, re-run — should detect incomplete partials and restart
