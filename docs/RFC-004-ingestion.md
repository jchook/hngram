# RFC-004 (Agent-Oriented)

## Rust Ingestion + Processing Pipeline

## Status

**Not Started** — stub at `server/crates/ingestion/src/main.rs`

---

## 0. Scope

Define:

* data source and schema
* download and storage of raw Parquet files
* tokenization integration (RFC-001)
* n-gram generation + pruning (RFC-002)
* two-pass aggregation strategy with bounded memory
* ClickHouse loading
* idempotency guarantees
* CLI interface
* progress reporting

Future (not v1):

* incremental updates from live HN stream

---

## 1. Pipeline Overview

## Spec (mandatory)

Three discrete phases:

```text
Phase 0: Download
  Download Parquet files from HuggingFace to local disk

Phase 1: Vocabulary Build (pass 1)
  For each file:
    Parse → Filter → Tokenize → Count n-grams
    → Write partial global counts to temp file
  Merge all partial counts → Build admitted vocabulary

Phase 2: Backfill (pass 2)
  For each file:
    Parse → Filter → Tokenize → Generate n-grams
    → Aggregate per-bucket (local)
    → Apply pruning (per-bucket + vocabulary)
    → Insert into ClickHouse
```

---

## Rationale

* separates concerns into testable phases
* download-then-process decouples network from CPU-bound work
* two-pass design enables global threshold pruning with bounded memory
* each phase is independently restartable

---

## Flexibility

* phases 1 and 2 may be fused if memory allows (not recommended for full corpus)
* stages within a phase may be fused for performance

---

# 2. Input Data Source

## Spec (mandatory)

### Dataset

**`open-index/hacker-news`** on HuggingFace

Download URL pattern:

```text
https://huggingface.co/datasets/open-index/hacker-news/resolve/main/data/{YYYY}/{YYYY-MM}.parquet
```

### Parquet Schema

| Column | Type | Description |
|--------|------|-------------|
| `id` | `uint32` | Unique item ID, monotonically increasing |
| `deleted` | `uint8` | 1 if deleted, 0 otherwise |
| `type` | `int8` | 1=story, 2=comment, 3=poll, 4=pollopt, 5=job |
| `by` | `string` | Author username |
| `time` | `timestamp[ms, tz=UTC]` | Creation time in UTC |
| `text` | `string` | HTML body text |
| `dead` | `uint8` | 1 if flagged/killed, 0 otherwise |
| `parent` | `uint32` | Parent item ID |
| `poll` | `uint32` | Associated poll ID (pollopt only) |
| `kids` | `list<uint32>` | Direct child item IDs |
| `url` | `string` | External URL (stories only) |
| `score` | `int32` | Upvotes minus downvotes |
| `title` | `string` | Title (stories/jobs/polls only) |
| `parts` | `list<uint32>` | Poll option IDs (polls only) |
| `descendants` | `int32` | Total comment count in thread |
| `words` | `list<string>` | Pre-tokenized words (not used by us) |

### File Partitioning

Historical files are partitioned by month:

```text
data/2006/2006-10.parquet
data/2006/2006-12.parquet
data/2007/2007-01.parquet
...
data/2026/2026-03.parquet
```

~244 files total. Zstandard compression level 22. ~11.7 GB total.

### Corpus Size

* ~47.4M total items
* ~41.3M comments (87.2%) — these are what we process
* Time span: October 2006 to present

### Row Filtering

Only process rows where:

```text
type = 2          (comment)
deleted = 0       (not deleted)
dead = 0          (not flagged/killed)
text IS NOT NULL  (has body text)
```

### Bucket Derivation

```text
bucket = UTC date from the `time` column
```

All timestamps are UTC. No timezone conversion. The `time` column is `timestamp[ms, tz=UTC]` — extract the date component directly.

---

## Rationale

* `type = 2` selects only comments (87% of dataset)
* filtering deleted/dead ensures only visible comments are counted
* UTC everywhere avoids timezone ambiguity
* monthly file partitioning provides natural processing units

---

## Flexibility

* agent may push row filtering into Parquet scan predicates if the reader supports it
* the `words` column is ignored — we use our own tokenizer (RFC-001)

---

# 2.1 Download Phase

## Spec (mandatory)

Download is a discrete step that runs before processing.

### Behavior

1. List expected files for the requested date range
2. For each file, download to local `data-dir` preserving the `data/YYYY/YYYY-MM.parquet` structure
3. Skip files that already exist locally (by path — no hash check in v1)
4. Log progress: file name, download size, speed

### Storage

Files are stored at:

```text
{data-dir}/data/YYYY/YYYY-MM.parquet
```

Default `data-dir`: `./hn-data/` (relative to working directory)

### File listing

The expected file set is computed from a start month and end month. For each month in range, the expected path is `data/YYYY/YYYY-MM.parquet`. Not all months exist (e.g. 2006-11 is missing) — download failures for individual files should warn and continue, not abort.

---

## Rationale

* decouples network I/O from CPU-bound processing
* local files can be re-processed without re-downloading
* restartable — skips already-downloaded files
* simple to verify and debug (files on disk)

---

# 3. Execution Model

## Spec

* processing must be parallelized within each file
* unit of parallelism: row group or chunk of rows within a file
* files are processed **sequentially** (one at a time) to bound memory

---

## Requirements

* aggregation must be **commutative and deterministic**
* final results must be identical regardless of thread scheduling

---

## Rationale

* sequential file processing bounds memory to one file's worth of data
* parallel row processing within a file saturates CPU cores
* commutative merge ensures correctness

---

## Flexibility

* agent may choose: thread pool (rayon), async pipeline, or work-stealing
* agent may process multiple files concurrently if memory allows

---

# 4. Tokenization

## Spec

* must use RFC-001 tokenizer exactly
* tokenizer version must be attached to all output
* `text` column contains HTML — tokenizer handles HTML stripping (RFC-001 §5.1)

---

## Constraints

* no alternative tokenization allowed
* no NLP libraries
* must use `time` crate (not `chrono`) for all date/time operations, matching `hn-clickhouse`

---

## Rationale

* consistency is required for correctness
* `time` crate is used throughout the ClickHouse client and API — standardize to avoid conversion friction

---

# 5. N-gram Generation

## Spec

* must follow RFC-002
* generate n in {1,2,3}
* sliding window only
* no cross-comment ngrams

---

## Rationale

* ensures deterministic and correct counts

---

# 6. Aggregation Model

## Spec

Aggregation must occur **before any database write**

---

### Required data structures (conceptual)

```text
HashMap<(bucket, n, ngram), count>
HashMap<(bucket, n), total_count>
```

Use the existing `NgramCounter` from `tokenizer::counter`.

---

## Requirements

* aggregation must be per-bucket
* denominators must include all ngrams (pre-pruning)

---

## Rationale

* reduces write amplification
* preserves correct normalization

---

## Flexibility

* agent may shard maps per thread, then merge
* agent may use lock-free structures

---

# 7. Pruning

## Spec

Apply both:

1. per-bucket pruning (RFC-002 §8.4)
2. global vocabulary filtering (for n >= 2)

---

## Order

Either:

```text
aggregate → per-bucket prune → vocabulary filter
```

or:

```text
aggregate → vocabulary filter → per-bucket prune
```

must produce identical results

---

## Rationale

* ensures storage remains bounded
* enforces vocabulary constraints

---

## Flexibility

* agent may reorder operations for performance

---

# 8. Flush Strategy

## Spec (mandatory)

Flush unit is **one file** (one month of data).

After processing all rows in a Parquet file:

1. Compute per-bucket aggregates for all comments in that file
2. Apply pruning (per-bucket thresholds + vocabulary filter)
3. Batch insert into ClickHouse
4. Record file as completed in manifest

---

## Requirements

* flush must align with file boundaries — no partial-file flushes
* since each monthly file contains complete daily buckets, this guarantees no partial bucket aggregates
* a single file's aggregates must fit in memory (one month of HN data is manageable)

---

## Rationale

* file-aligned flushes prevent duplicate rows from partial writes
* simplifies idempotency — each file is either fully processed or not
* one month of HN data produces a bounded number of n-gram aggregates

---

# 9. ClickHouse Insert

## Spec

* insert using batch inserts
* insert only aggregated rows
* insert size >= 10k rows (recommended)

---

## Requirements

* no duplicate keys
* consistent ordering not required

---

## Rationale

* ClickHouse optimized for large batch inserts

---

## Flexibility

* agent may use HTTP interface or native protocol
* agent may use buffered writers

---

# 10. Idempotency

## Spec (mandatory)

Pipeline must guarantee:

```text
no duplicate (tokenizer_version, n, ngram, bucket)
```

### Mechanism: file-level manifest

Maintain a JSON manifest file at `{data-dir}/manifest.json`:

```json
{
  "tokenizer_version": "1",
  "phase1_completed": ["data/2024/2024-01.parquet", ...],
  "phase2_completed": ["data/2024/2024-01.parquet", ...],
  "vocabulary_built": true
}
```

Before processing a file in either phase:
* check if the file path is in the corresponding completed list
* if yes, skip it

After successfully processing a file:
* append the file path to the completed list
* write manifest to disk

### Tokenizer version change

If `tokenizer_version` in the manifest does not match the current `TOKENIZER_VERSION`, the manifest is invalid. The pipeline must:

* warn the user
* require explicit `--force` flag or manual manifest deletion to proceed
* a tokenizer change requires full rebuild (all data is invalidated)

---

## Rationale

* ClickHouse does not enforce uniqueness — duplicates corrupt counts
* file-level tracking is simple and aligns with flush boundaries
* manifest is human-readable and debuggable

---

# 11. Historical Backfill

## Spec (mandatory)

Two-pass pipeline with chunk-and-merge strategy for bounded memory.

### Pass 1: Vocabulary Build

For each Parquet file:

1. Read and filter comments
2. Tokenize each comment
3. Count n-grams using `NgramCounter`
4. Extract global counts via `NgramCounter::global_counts()`
5. Write partial global counts to a temp file: `{data-dir}/partial/{YYYY-MM}.counts`

After all files are processed:

6. Merge all partial count files into total global counts
7. Apply `build_vocabulary()` with `PruningConfig` thresholds
8. Write admitted vocabulary to `{data-dir}/vocabulary.json`
9. Insert vocabulary into ClickHouse `ngram_vocabulary` table
10. Mark `vocabulary_built: true` in manifest

### Partial count file format

Simple binary or JSON lines format:

```text
{n}\t{ngram}\t{count}\n
```

One line per unique (n, ngram) pair seen in that file.

### Memory model

Peak memory during pass 1 = one file's worth of `NgramCounter` data. After processing each file, global counts are flushed to disk and the counter is dropped. The merge step streams partial files and accumulates only the final totals.

### Pass 2: Backfill

For each Parquet file:

1. Read and filter comments
2. Tokenize each comment
3. Count n-grams using `NgramCounter`
4. Apply vocabulary filter + per-bucket pruning
5. Batch insert `ngram_counts` and `bucket_totals` into ClickHouse
6. Mark file as completed in manifest

---

## Rationale

* chunk-and-merge bounds memory to one file at a time during pass 1
* partial count files are cheap to write and merge
* two-pass is required because global vocabulary thresholds need the full corpus
* pass 2 re-reads files but this is fast from local SSD

---

## Flexibility

* partial count files may use binary format for speed
* agent may hold multiple files in memory if RAM allows (but not required)

---

# 12. Incremental Updates

## Spec

Not implemented in v1. Noted here for future design.

The HuggingFace dataset provides a `today` configuration with 5-minute Parquet snapshots:

```text
today/YYYY/MM/DD/HH/MM.parquet
```

Future incremental updates would:

* process only new `today` files
* use the existing admitted vocabulary (no expansion)
* count only admitted bigrams/trigrams
* insert into current day's bucket

---

## Constraints (future)

* must use existing vocabulary for n >= 2
* must not expand vocabulary during incremental updates
* periodic vocabulary re-admission via full rebuild

---

## Rationale

* v1 ships with historical backfill only — data freshness is not a launch blocker
* incremental design is straightforward once backfill is working

---

# 13. Failure Handling

## Spec

* pipeline must be restartable at any point
* partial failures must not corrupt data

---

## Mechanism

File-level idempotency (§10) provides automatic restart:

* re-running the pipeline skips completed files
* if a file fails mid-processing, no data is written (flush is at file boundary)
* re-run will retry the failed file from scratch

---

## Rationale

* long-running batch jobs are failure-prone
* file-level atomicity is simple and correct

---

# 14. Memory Model

## Spec

* system must operate within bounded memory
* must not load entire corpus into memory
* peak memory = one Parquet file's comments + their n-gram aggregates

---

## Estimates

* largest monthly file: ~2M comments
* `NgramCounter` for 2M comments: ~1-2 GB (dominated by bigram/trigram string keys)
* pass 1 partial count merge: streams from disk, bounded by final vocabulary size
* total working memory target: < 4 GB

---

## Rationale

* historical backfill runs on a local development machine (not the VPS)
* 4 GB is comfortable for a modern dev machine

---

## Flexibility

* agent may reduce memory by processing row groups within a file instead of the whole file
* agent may use a more compact representation for n-gram keys

---

# 15. Determinism

## Spec

* same input -> identical output
* independent of thread scheduling or execution order

---

## Rationale

* required for reproducibility and debugging

---

# 16. Performance Targets

## Spec

* must saturate CPU cores during tokenization and n-gram generation
* target: process full corpus in under 1 hour on a modern dev machine (8+ cores)

---

## Rationale

* ensures feasible full rebuild time
* 41M comments / 60 min = ~680K comments/min = ~11K comments/sec

---

## Flexibility

* agent may optimize: batching, SIMD, parallel aggregation
* exact timing depends on hardware — the target is a guideline, not a hard requirement

---

# 17. Output Guarantees

For every processed bucket:

* correct counts for all retained ngrams
* correct denominators (unpruned totals)
* no duplicates
* all rows tagged with `tokenizer_version`

---

# 18. Prohibited Designs

Agent must NOT:

* write per-comment ngram rows to ClickHouse
* rely on ClickHouse for aggregation of raw data
* use non-deterministic tokenization
* recompute normalization at ingestion time
* mutate historical data without full rebuild
* use `chrono` crate — use `time` crate to match `hn-clickhouse`

---

# 19. CLI Interface

## Spec (mandatory)

The ingestion binary provides subcommands for each phase.

### `ingestion download`

Download Parquet files from HuggingFace.

```text
ingestion download [OPTIONS]

Options:
  --data-dir <PATH>    Local storage directory [default: ./hn-data]
  --start <YYYY-MM>    First month to download [default: 2006-10]
  --end <YYYY-MM>      Last month to download [default: current month]
```

### `ingestion vocabulary`

Run pass 1: build vocabulary from global counts.

```text
ingestion vocabulary [OPTIONS]

Options:
  --data-dir <PATH>    Directory with downloaded Parquet files [default: ./hn-data]
  --start <YYYY-MM>    First month to process [default: 2006-10]
  --end <YYYY-MM>      Last month to process [default: current month]
```

### `ingestion backfill`

Run pass 2: generate daily aggregates and insert into ClickHouse.

Requires vocabulary to be built first.

```text
ingestion backfill [OPTIONS]

Options:
  --data-dir <PATH>    Directory with downloaded Parquet files [default: ./hn-data]
  --start <YYYY-MM>    First month to process [default: 2006-10]
  --end <YYYY-MM>      Last month to process [default: current month]
```

ClickHouse connection is configured via environment variables (same as API):

```text
CLICKHOUSE_HOST     [default: localhost]
CLICKHOUSE_PORT     [default: 8123]
CLICKHOUSE_USER     [default: default]
CLICKHOUSE_PASSWORD [default: ""]
CLICKHOUSE_DATABASE [default: hn_ngram]
```

### `ingestion status`

Show current state of the manifest.

```text
ingestion status [OPTIONS]

Options:
  --data-dir <PATH>    [default: ./hn-data]
```

---

## Rationale

* discrete subcommands make each phase independently runnable and testable
* reasonable defaults mean `ingestion download && ingestion vocabulary && ingestion backfill` works out of the box
* `--start`/`--end` allows processing a subset for testing

---

# 20. Progress Reporting

## Spec (mandatory)

All output goes to stderr via `tracing` (structured logging).

### Per-file progress

```text
[INFO] Processing data/2024/2024-01.parquet (142/244)
[INFO]   Comments: 1,832,451 | Filtered: 1,790,200
[INFO]   Unigrams: 12,345,678 | Bigrams: 9,876,543 | Trigrams: 7,654,321
[INFO]   Inserted: 234,567 ngram_counts rows, 90 bucket_totals rows
[INFO]   Elapsed: 12.3s
```

### Phase summary

```text
[INFO] Vocabulary build complete
[INFO]   Files processed: 244
[INFO]   Unique unigrams: 1,234,567
[INFO]   Admitted bigrams: 456,789 (of 12,345,678 candidates)
[INFO]   Admitted trigrams: 123,456 (of 8,765,432 candidates)
[INFO]   Total elapsed: 32m 15s
```

### Download progress

```text
[INFO] Downloading data/2024/2024-01.parquet (142/244) ... 45.2 MB in 3.1s
```

---

## Rationale

* long-running pipeline needs visible progress
* structured logging via `tracing` is already in the crate
* stderr keeps stdout clean for any future piped output

---

# 21. Acceptance Criteria

Pipeline is valid if:

* produces correct aggregates matching RFC-002 golden tests
* respects pruning rules (per-bucket + global vocabulary)
* guarantees idempotency via file-level manifest
* scales to full 41M-comment dataset
* operates within bounded memory (< 4 GB)
* produces deterministic output
* completes full backfill in reasonable time (< 2 hours target)

---

## Final Note for Agent

If proposing improvements:

* must preserve correctness of tokenization, aggregation, and normalization
* must not introduce duplicate counting
* must not increase asymptotic storage or query cost
* must use `time` crate, not `chrono`
