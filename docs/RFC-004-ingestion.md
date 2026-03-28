# RFC-004

## Rust Ingestion + Processing Pipeline

## Status

**Implemented** — `server/crates/ingestion/`

---

## 0. Scope

Define:

* data source and schema
* download and storage of raw Parquet files
* tokenization integration (RFC-001)
* n-gram generation + pruning (RFC-002)
* aggregation strategy with bounded memory
* ClickHouse loading
* idempotency guarantees
* watermark-based incremental ingestion
* CLI interface
* progress reporting

---

## 1. Pipeline Overview

### Primary: Single-Pass Incremental (`ingest`)

The recommended command. Combines vocabulary update and count insertion in one tokenization pass per file, with watermark-based filtering for incremental updates.

```text
For each file:
  Read comments after watermark → Tokenize → Count n-grams
  → Update cumulative global counts
  → Rebuild vocabulary, delta-insert new admissions to ClickHouse
  → Apply pruning (per-bucket + vocabulary filter)
  → Insert ngram_counts + bucket_totals into ClickHouse
  → Advance watermark
```

### Legacy: Two-Pass (`vocabulary` + `backfill`)

Still available for explicit control over each phase.

```text
Phase 1 — vocabulary:
  For each file:
    Read → Tokenize → Count n-grams → Write partial counts to TSV
  Merge all partials → Build admitted vocabulary → Insert to ClickHouse

Phase 2 — backfill:
  For each file:
    Read → Tokenize → Count n-grams
    → Apply vocabulary filter + per-bucket pruning
    → Insert ngram_counts + bucket_totals into ClickHouse
```

---

## Rationale

* single-pass avoids re-reading files, halving I/O for the common case
* watermark enables daily/hourly updates without re-processing
* legacy two-pass remains for full rebuilds or debugging
* each phase/command is independently restartable

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
text != ""        (non-empty)
time IS NOT NULL  (has timestamp)
```

### Bucket Derivation

```text
bucket = UTC date from the `time` column → "YYYY-MM-DD" string
```

All timestamps are UTC. No timezone conversion. The `time` column is `timestamp[ms, tz=UTC]` — extract the date component directly using the `time` crate.

---

## Rationale

* `type = 2` selects only comments (87% of dataset)
* filtering deleted/dead ensures only visible comments are counted
* UTC everywhere avoids timezone ambiguity
* monthly file partitioning provides natural processing units

---

# 2.1 Download Phase

## Spec (mandatory)

Download is a discrete step that runs before processing.

### Behavior

1. List expected files for the requested date range
2. For each file, download to local `data-dir` preserving the `data/YYYY/YYYY-MM.parquet` structure
3. Skip files that already exist locally (by path — no hash check)
4. Stream to temp file then rename atomically
5. Log progress: file name, download size, speed

### Storage

Files are stored at:

```text
{data-dir}/data/YYYY/YYYY-MM.parquet
```

Default `data-dir`: `./data/hn/` (relative to working directory)

### File listing

The expected file set is computed from a start month and end month (`YearMonth` struct). For each month in range, the expected path is `data/YYYY/YYYY-MM.parquet`. Not all months exist (e.g. 2006-11 is missing) — download failures for individual files should warn and continue, not abort.

---

## Rationale

* decouples network I/O from CPU-bound processing
* local files can be re-processed without re-downloading
* restartable — skips already-downloaded files
* atomic write prevents corrupt partial downloads

---

# 3. Execution Model

## Spec

* processing is parallelized within each file using rayon
* unit of parallelism: 1024-comment chunks processed in parallel
* files are processed **sequentially** (one at a time) to bound memory
* legacy `vocabulary` command processes files concurrently with bounded parallelism (semaphore sized to available CPU cores)

---

## Requirements

* aggregation must be **commutative and deterministic**
* final results must be identical regardless of thread scheduling

---

## Rationale

* sequential file processing bounds memory to one file's worth of data
* parallel chunk processing within a file saturates CPU cores
* commutative merge ensures correctness

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
* `NgramCounter::process_comment(bucket, tokens)` handles generation + counting

---

# 6. Aggregation Model

## Spec

Aggregation occurs **before any database write**.

---

### Data structures

`NgramCounter` from `tokenizer::counter` maintains:

```text
HashMap<NgramKey{bucket, n, ngram}, u32>   — per-bucket n-gram counts
HashMap<TotalKey{bucket, n}, u64>          — per-bucket total counts (denominators)
```

Global counts (for vocabulary admission) are tracked separately:

```text
HashMap<(u8, String), u64>                 — (n, ngram) → total count across all buckets
```

---

## Requirements

* aggregation must be per-bucket
* denominators must include all ngrams (pre-pruning)

---

## Rationale

* reduces write amplification
* preserves correct normalization

---

# 7. Pruning

## Spec

Apply both:

1. **Global vocabulary admission** (for n >= 2): n-grams must meet min global count thresholds to be admitted
2. **Per-bucket pruning**: n-grams must meet min per-bucket count thresholds

Unigrams are always admitted (no global threshold). Vocabulary is **append-only** — previously admitted n-grams are never dropped within a run.

---

## Configuration

Thresholds are loaded from environment via `PruningConfig::from_env()`:

| Variable | Default | Description |
|----------|---------|-------------|
| `PRUNE_MIN_2GRAM_GLOBAL` | 20 | Min global count for bigram admission |
| `PRUNE_MIN_3GRAM_GLOBAL` | 10 | Min global count for trigram admission |
| `PRUNE_MIN_1GRAM_BUCKET` | 1 | Min per-bucket count for unigrams |
| `PRUNE_MIN_2GRAM_BUCKET` | 3 | Min per-bucket count for bigrams |
| `PRUNE_MIN_3GRAM_BUCKET` | 5 | Min per-bucket count for trigrams |

---

## Order

```text
aggregate → filter_to_vocabulary(admitted, config)
```

`filter_to_vocabulary` applies both vocabulary filtering (for n >= 2) and per-bucket minimum count thresholds in one pass. Bucket totals remain unpruned (all n-grams contribute to denominators).

---

## Rationale

* ensures storage remains bounded
* enforces vocabulary constraints
* append-only vocabulary prevents loss of historical n-grams when re-ingesting

---

# 8. Flush Strategy

## Spec (mandatory)

Flush unit is **one file** (one month of data).

After processing all rows in a Parquet file:

1. Compute per-bucket aggregates for all comments in that file
2. Apply pruning (per-bucket thresholds + vocabulary filter)
3. Batch insert into ClickHouse
4. Record progress (manifest save for legacy, watermark advance for `ingest`)

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
* three table targets: `ngram_counts`, `bucket_totals`, `ngram_vocabulary`

---

### Row types

```rust
NgramCountRow    { tokenizer_version, n, ngram, bucket, count }
BucketTotalRow   { tokenizer_version, n, bucket, total_count }
NgramVocabularyRow { tokenizer_version, n, ngram, global_count, admitted_at }
```

---

## Requirements

* tables use ReplacingMergeTree — dedup happens at merge time
* no duplicate keys (enforced by manifest/watermark tracking)
* consistent ordering not required

---

## Rationale

* ClickHouse optimized for large batch inserts
* ReplacingMergeTree provides eventual dedup as safety net

---

# 10. Idempotency

## Spec (mandatory)

Pipeline must guarantee:

```text
no duplicate (tokenizer_version, n, ngram, bucket)
```

### Mechanism: watermark (`ingest` command)

The manifest stores `last_ingested_ts: i64` (milliseconds since epoch) — the timestamp of the newest comment processed. On each run, only comments with `time > watermark` are processed.

This means:
* re-running is a no-op if no new comments exist
* new comments in existing month files are picked up automatically
* a single global value tracks progress across all files

### Mechanism: file-level manifest (legacy commands)

For `vocabulary` and `backfill`, the manifest tracks per-file completion:

```json
{
  "tokenizer_version": "1",
  "phase1_completed": ["data/2024/2024-01.parquet", ...],
  "phase2_completed": ["data/2024/2024-01.parquet", ...],
  "vocabulary_built": true,
  "last_ingested_ts": 0
}
```

Before processing a file:
* check if the file path is in the corresponding completed list
* if yes, skip it

After successfully processing:
* append file path to completed list
* save manifest atomically (write tmp, rename)

### Tokenizer version change

If `tokenizer_version` in the manifest does not match the current `TOKENIZER_VERSION`, the pipeline errors and requires manual manifest deletion to proceed. A tokenizer change requires full rebuild (all data is invalidated).

---

## Rationale

* ClickHouse does not enforce uniqueness at insert time — duplicates corrupt counts
* watermark provides simple, global progress tracking for incremental mode
* file-level tracking is simple and aligns with flush boundaries for legacy mode
* manifest is human-readable and debuggable

---

# 11. Watermark-Based Incremental Ingestion

## Spec (mandatory)

The `ingest` command uses a single-pass pipeline with watermark filtering and cumulative global count tracking.

### Watermark

A single `i64` value in the manifest: `last_ingested_ts` (milliseconds since epoch). This is the timestamp of the newest comment processed.

On each run:
1. Read the watermark from manifest (0 if first run)
2. For each month's Parquet file, read comments with `time > watermark`
3. Months with no new comments are skipped
4. After processing, watermark advances to the max timestamp seen

### Cumulative global counts

Global n-gram counts are persisted in a binary snapshot (`{data-dir}/cumulative.bin`, bincode format) so vocabulary can be incrementally updated without re-reading all files.

```rust
struct CumulativeSnapshot {
    version: u8,
    counts: HashMap<(u8, String), u64>,
}
```

If the snapshot is missing or corrupt, it is rebuilt from partial TSV files.

### Ingest flow

```text
ingest(data_dir, months, manifest, ch):
    global_counts = load_cumulative_snapshot(data_dir)
    prev_vocab = ch.load_vocabulary()
    watermark = manifest.last_ingested_ts
    max_ts = watermark

    for each month in months:
        comments = read_comments_after(parquet_path, watermark)
        if comments.is_empty(): continue

        counter = process_comments_parallel(&comments)
        max_ts = max(max_ts, max timestamp in comments)

        // Update cumulative global counts
        global_counts += counter.global_counts()

        // Write partial TSV for recovery
        write_partial_counts(partial_path, month_globals)

        // Rebuild vocabulary, delta-insert new admissions
        vocabulary = build_vocabulary(&global_counts, &config)
        new_admissions = vocabulary.keys() - prev_vocab.keys()
        if new_admissions: ch.insert_vocabulary(new_rows)
        prev_vocab.merge(new_admissions)
        vocabulary.merge(prev_vocab)  // append-only

        // Filter and insert counts
        filtered = counter.filter_to_vocabulary(&vocabulary, &config)
        ch.insert_ngram_counts(filtered)
        ch.insert_bucket_totals(counter.totals())

    manifest.last_ingested_ts = max_ts
    save_cumulative_snapshot(global_counts)
```

### Vocabulary is append-only

Previously admitted n-grams are always retained. The vocabulary set from ClickHouse is loaded once at start and merged with newly admitted n-grams after each file. This prevents loss of historical vocabulary when global counts shift.

---

## Operational examples

First full ingest:
```
cargo run -p ingestion -- ingest --start 2024-01 --end 2024-12
```
Watermark set to the max timestamp across all of 2024.

Daily update (re-download current month, run ingest):
```
cargo run -p ingestion -- download --start 2025-03 --end 2025-03
cargo run -p ingestion -- ingest --start 2025-03 --end 2025-03
```
Only comments newer than the watermark are processed.

Re-run is a no-op:
```
cargo run -p ingestion -- ingest --start 2024-01 --end 2024-12
```
All comments <= watermark, nothing is processed.

---

## Edge cases

- **Out-of-order timestamps**: Comments with timestamps older than the watermark are skipped. The dataset is append-only and we don't retroactively adjust.
- **Parquet file re-downloaded with corrections**: Older modified comments won't be picked up. A full rebuild handles this case.
- **Watermark corruption**: If the manifest is deleted, watermark resets to 0 and everything re-processes. Since ClickHouse tables use MergeTree, this creates duplicates. Clear ClickHouse tables before a full rebuild.

---

## Rationale

* single tokenization pass halves I/O vs two-pass
* watermark enables any-frequency incremental updates
* cumulative snapshot avoids re-reading all partials on each run
* partial TSV files provide recovery if snapshot corrupts

---

# 12. Legacy Two-Pass Pipeline

### Pass 1: Vocabulary Build (`vocabulary` command)

For each Parquet file (concurrent, semaphore-bounded):

1. Read and filter comments
2. Tokenize each comment
3. Count n-grams using `NgramCounter`
4. Extract global counts via `NgramCounter::global_counts()`
5. Write partial global counts to `{data-dir}/partial/{YYYY-MM}.counts`
6. Mark file as phase 1 done in manifest

After all files:

7. Merge all partial count files into total global counts
8. Apply `build_vocabulary()` with `PruningConfig` thresholds
9. Load previous vocabulary from ClickHouse (append-only merge)
10. Insert vocabulary into ClickHouse `ngram_vocabulary` table
11. Mark `vocabulary_built: true` in manifest

### Partial count file format

TSV, one line per unique (n, ngram) pair:

```text
{n}\t{ngram}\t{count}\n
```

### Pass 2: Backfill (`backfill` command)

Requires `vocabulary_built == true`.

For each Parquet file:

1. Read and filter comments
2. Tokenize each comment
3. Count n-grams using `NgramCounter`
4. Apply vocabulary filter + per-bucket pruning
5. Batch insert `ngram_counts` and `bucket_totals` into ClickHouse
6. Mark file as phase 2 done in manifest

---

## Rationale

* two-pass is useful for full rebuilds where vocabulary must be computed from the entire corpus before any counts are inserted
* each phase is independently restartable
* concurrent file processing in phase 1 speeds up vocabulary building

---

# 13. Failure Handling

## Spec

* pipeline must be restartable at any point
* partial failures must not corrupt data

---

## Mechanism

**`ingest` command:** watermark only advances after successful processing. Re-run picks up from the last watermark. If a file fails mid-processing, no watermark advance occurs, so it retries from the same point.

**Legacy commands:** file-level idempotency provides automatic restart. Re-running skips completed files. If a file fails mid-processing, no data is written (flush is at file boundary).

---

## Rationale

* long-running batch jobs are failure-prone
* both mechanisms provide simple, correct restart semantics

---

# 14. Memory Model

## Spec

* system must operate within bounded memory
* must not load entire corpus into memory
* peak memory = one Parquet file's comments + their n-gram aggregates
* `ingest` command also holds cumulative global counts in memory

---

## Estimates

* largest monthly file: ~2M comments
* `NgramCounter` for 2M comments: ~1-2 GB (dominated by bigram/trigram string keys)
* cumulative global counts: bounded by vocabulary size
* total working memory target: < 4 GB

---

## Rationale

* historical backfill runs on a local development machine (not the VPS)
* 4 GB is comfortable for a modern dev machine

---

# 15. Determinism

## Spec

* same input → identical output
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

The ingestion binary provides subcommands for each operation.

### `ingestion download`

Download Parquet files from HuggingFace.

```text
ingestion download [OPTIONS]

Options:
  --data-dir <PATH>    Local storage directory [default: ./data/hn]
  --start <YYYY-MM>    First month to download [default: 2006-10]
  --end <YYYY-MM>      Last month to download [default: current month]
```

### `ingestion ingest`

Single-pass ingestion: tokenize, update vocabulary, insert counts. Uses watermark for incremental updates.

```text
ingestion ingest [OPTIONS]

Options:
  --data-dir <PATH>    Directory with downloaded Parquet files [default: ./data/hn]
  --start <YYYY-MM>    First month to process [default: 2006-10]
  --end <YYYY-MM>      Last month to process [default: current month]
```

### `ingestion vocabulary`

Legacy pass 1: build vocabulary from global counts.

```text
ingestion vocabulary [OPTIONS]

Options:
  --data-dir <PATH>    Directory with downloaded Parquet files [default: ./data/hn]
  --start <YYYY-MM>    First month to process [default: 2006-10]
  --end <YYYY-MM>      Last month to process [default: current month]
```

### `ingestion backfill`

Legacy pass 2: generate daily aggregates and insert into ClickHouse. Requires vocabulary to be built first.

```text
ingestion backfill [OPTIONS]

Options:
  --data-dir <PATH>    Directory with downloaded Parquet files [default: ./data/hn]
  --start <YYYY-MM>    First month to process [default: 2006-10]
  --end <YYYY-MM>      Last month to process [default: current month]
```

### `ingestion status`

Show current state of the manifest: tokenizer version, watermark, phase completion counts.

```text
ingestion status [OPTIONS]

Options:
  --data-dir <PATH>    [default: ./data/hn]
```

### Environment

ClickHouse connection is configured via environment variables (same as API):

```text
CLICKHOUSE_HOST     [default: localhost]
CLICKHOUSE_PORT     [default: 8123]
CLICKHOUSE_USER     [default: default]
CLICKHOUSE_PASSWORD [default: ""]
CLICKHOUSE_DATABASE [default: hn_ngram]
```

---

## Rationale

* `ingest` is the recommended single command for most use cases
* `vocabulary` + `backfill` remain for explicit control during full rebuilds
* reasonable defaults mean `ingestion download && ingestion ingest` works out of the box
* `--start`/`--end` allows processing a subset for testing

---

# 20. Progress Reporting

## Spec (mandatory)

All output goes to stderr via `tracing` (structured logging).

### Per-file progress (`ingest`)

```text
[INFO] Ingesting: data/2024/2024-01.parquet (1/12) — 1,832,451 new comments
[INFO]   Comments: 1832451 | Counts: 234567 | Totals: 90 | Vocab: +2345 | Elapsed: 12.3s
```

### Per-file progress (legacy)

```text
[INFO] Phase 1: data/2024/2024-01.parquet (1/244)
[INFO]   data/2024/2024-01.parquet — Comments: 1832451 | Unique n-grams: 9876543 | Elapsed: 12.3s
```

### Phase summary (legacy)

```text
[INFO] Global counts: 1234567 unigrams, 12345678 bigram candidates, 8765432 trigram candidates
[INFO] Admitted: 456789 bigrams (of 12345678), 123456 trigrams (of 8765432)
```

### Watermark display

```text
[INFO] Watermark: 1705318200000 (2024-01-15)
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
* guarantees idempotency via watermark and/or file-level manifest
* scales to full 41M-comment dataset
* operates within bounded memory (< 4 GB)
* produces deterministic output
* completes full backfill in reasonable time (< 2 hours target)
* supports incremental updates via watermark (no re-processing)

---

## Final Note for Agent

If proposing improvements:

* must preserve correctness of tokenization, aggregation, and normalization
* must not introduce duplicate counting
* must not increase asymptotic storage or query cost
* must use `time` crate, not `chrono`
