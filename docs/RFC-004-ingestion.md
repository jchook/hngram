# RFC-004

## Rust Ingest + Processing Pipeline

## Status

**In Progress** — redesigning for parquet output + import workflow

---

## 0. Scope

Define:

* data source and schema
* download and storage of raw Parquet files
* tokenization integration (RFC-001)
* n-gram generation + pruning (RFC-002)
* aggregation strategy with bounded memory
* dual output: ClickHouse-ready Parquet files or direct DB insertion
* import with atomic table swap
* ingest log for watermark tracking
* idempotency guarantees
* CLI interface
* progress reporting

---

## 1. Pipeline Overview

Three subcommands handle the full lifecycle:

```text
ingest download   — fetch raw Parquet from HuggingFace
ingest process    — tokenize, count, prune → output parquet or direct to ClickHouse
ingest import     — load parquet into staging tables, atomic swap to live
```

### Why two output modes?

Processing the full corpus (~41M comments) is CPU- and disk-intensive. The prod environment is a small, inexpensive VPS that cannot handle this workload in reasonable time. We assume the developer has a local workstation that is significantly faster. The Parquet output mode exists to **bootstrap prod once**: process the full corpus locally, transfer the output files, and import them. After that, all ongoing ingest happens directly on the VPS via ClickHouse mode — daily or monthly deltas are small enough to process on modest hardware.

### Bootstrap (one-time, local workstation → prod)

```text
download → process --output parquet → scp → import
```

### Incremental (ongoing, on prod VPS)

```text
download → process
```

### `process` output modes

| | `--output parquet` | `--output clickhouse` (default) |
|---|---|---|
| **Vocabulary** | Built from scratch (two-pass) | Read from DB, expand with new admissions |
| **Watermark source** | 0 (full corpus) | `ingest_log` table |
| **ClickHouse needed** | No | Yes |
| **Use case** | One-time bootstrap | Ongoing incremental updates |

---

## Rationale

* full-corpus processing is too heavy for a small VPS — run it on the developer's workstation
* Parquet mode is a bootstrap transport artifact, not a long-term operating mode
* Parquet is the natural interchange format — ClickHouse reads it natively
* incremental updates are lightweight enough for the VPS
* single `process` command with output flag avoids duplicating logic

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

* processing is parallelized within each file
* files are processed **sequentially** (one at a time) to bound memory

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

Aggregation occurs **before any output** (database write or Parquet file write).

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

# 7. Vocabulary and Pruning

## What vocabulary is

There are far too many unique bigrams and trigrams to store counts for all of them. Vocabulary is the **admission list** — only n-grams whose total count across the entire corpus exceeds a threshold are tracked. Unigrams are always admitted (no threshold).

The purpose of the vocabulary table is to answer one question: **"is this n-gram important enough to track?"** The `global_count` column records the total corpus-wide count at the time of the most recent update.

## Append-only semantics

Vocabulary **only grows, never shrinks**. Once an n-gram is admitted, it stays admitted permanently. This follows from the nature of the data: comment counts only increase or stay the same over time, so an n-gram that crossed the admission threshold will never fall back below it.

During incremental processing, as new comments are ingested:
* cumulative global counts increase
* n-grams that newly cross admission thresholds are admitted (inserted)
* already-admitted n-grams have their `global_count` updated via re-insert — ReplacingMergeTree keeps the latest version (by `admitted_at`)
* no n-grams are ever removed

## Pruning levels

Two levels of pruning, applied in sequence:

1. **Global vocabulary admission** (n >= 2): n-gram must appear at least X times across the entire corpus to be admitted to the vocabulary
2. **Per-bucket pruning**: within a given day's bucket, n-gram must appear at least Y times to be stored

Bucket totals (denominators) are always computed from **all** n-grams before pruning, ensuring correct normalization.

## Configuration

Thresholds are loaded from environment via `PruningConfig::from_env()`:

| Variable | Default | Description |
|----------|---------|-------------|
| `PRUNE_MIN_2GRAM_GLOBAL` | 20 | Min global count for bigram admission |
| `PRUNE_MIN_3GRAM_GLOBAL` | 10 | Min global count for trigram admission |
| `PRUNE_MIN_1GRAM_BUCKET` | 1 | Min per-bucket count for unigrams |
| `PRUNE_MIN_2GRAM_BUCKET` | 3 | Min per-bucket count for bigrams |
| `PRUNE_MIN_3GRAM_BUCKET` | 5 | Min per-bucket count for trigrams |

## Application order

```text
aggregate → filter_to_vocabulary(admitted, config)
```

`filter_to_vocabulary` applies both vocabulary filtering (for n >= 2) and per-bucket minimum count thresholds in one pass.

---

## Rationale

* vocabulary admission bounds storage to n-grams that matter
* per-bucket pruning further reduces noise in daily counts
* append-only semantics are natural — corpus counts only increase
* ReplacingMergeTree on `ngram_vocabulary` handles re-inserts with updated `global_count` cleanly

---

# 8. Flush Strategy

## Spec (mandatory)

Flush unit is **one file** (one month of data).

After processing all rows in a Parquet file:

1. Compute per-bucket aggregates for all comments in that file
2. Apply pruning (per-bucket thresholds + vocabulary filter)
3. Write output (append to Parquet files or batch insert into ClickHouse)

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

# 9. Output

## 9.1 Parquet Output (`--output parquet`)

### Output structure

```text
{data-dir}/output/
  ngram_counts.parquet
  bucket_totals.parquet
  ngram_vocabulary.parquet
  global_counts.parquet
  ingest_log.parquet
```

All output Parquet schemas match the ClickHouse table schemas exactly (see §9.3). This allows `import` to load every file uniformly. There is no separate manifest — the `ingest_log.parquet` file contains a single row with the watermark, tokenizer version, processing stats, and duration.

### Vocabulary strategy

In Parquet mode, vocabulary is built from scratch using a two-pass approach:

1. **Pass 1**: Process all files, write per-file partial count files (sorted by key)
2. **K-way merge**: Stream-merge all sorted partial files, apply vocabulary admission thresholds, and write `global_counts.parquet` incrementally. Memory during merge is O(num_files), not O(total unique n-grams). See `docs/design-decisions.md` for rationale.
3. **Pass 2**: Re-process all files, filter against admitted vocabulary, write output Parquet

This ensures the vocabulary reflects the full corpus.

## 9.2 ClickHouse Output (`--output clickhouse`, default)

Direct batch inserts into `ngram_counts`, `bucket_totals`, and `ngram_vocabulary` tables.

### Vocabulary strategy

Vocabulary and cumulative global counts are both loaded from ClickHouse at startup. As new comments are processed, global counts increase, and:

* n-grams that newly cross admission thresholds are inserted into the vocabulary table
* all vocabulary entries are re-inserted with updated `global_count` — ReplacingMergeTree keeps the latest
* updated global counts are written back to the `global_counts` table
* vocabulary only grows, never shrinks (see §7)

After bootstrap via `import`, the VPS has the full `global_counts` table from the bootstrap run. Subsequent incremental runs load it, extend it, and write it back. All state lives in ClickHouse — no local files on prod.

### Watermark

Read from the `ingest_log` table (0 if no entries). Only comments with `time > watermark` are processed. After successful processing, a new `ingest_log` entry is written.

## 9.3 ClickHouse Table Schemas

### `ngram_counts`

```sql
CREATE TABLE ngram_counts (
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    bucket Date,
    count UInt32
) ENGINE = MergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tokenizer_version, n, ngram, bucket);
```

### `bucket_totals`

```sql
CREATE TABLE bucket_totals (
    tokenizer_version LowCardinality(String),
    n UInt8,
    bucket Date,
    total_count UInt64
) ENGINE = MergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tokenizer_version, n, bucket);
```

### `ngram_vocabulary`

```sql
CREATE TABLE ngram_vocabulary (
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    global_count UInt64,
    admitted_at DateTime
) ENGINE = ReplacingMergeTree(admitted_at)
ORDER BY (tokenizer_version, n, ngram);
```

### `global_counts`

```sql
CREATE TABLE global_counts (
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    count UInt64
) ENGINE = ReplacingMergeTree(count)
ORDER BY (tokenizer_version, n, ngram);
```

### `ingest_log`

```sql
CREATE TABLE ingest_log (
    id UUID DEFAULT generateUUIDv7(),
    tokenizer_version LowCardinality(String),
    command LowCardinality(String),
    last_ingested_ts Int64,
    comments_processed UInt64,
    ngram_counts_inserted UInt64,
    bucket_totals_inserted UInt64,
    vocabulary_inserted UInt64,
    start_month String,
    end_month String,
    duration_ms UInt64,
    created_at DateTime64(3, 'UTC') DEFAULT now64()
) ENGINE = MergeTree()
ORDER BY id;
```

The `ingest_log` is an append-only audit trail. Each `process` (ClickHouse mode) and `import` run appends one row. The watermark is read as:

```sql
SELECT last_ingested_ts FROM ingest_log ORDER BY id DESC LIMIT 1
```

---

## Rationale

* Parquet output decouples heavy processing from ClickHouse — no DB needed on the workstation
* ClickHouse reads Parquet natively — no custom format or protocol
* `ingest_log` replaces the manifest for prod state — single source of truth in the DB
* UUIDv7 is time-sortable, so `ORDER BY id DESC LIMIT 1` always gives the latest entry
* audit trail is free and useful for debugging

---

# 10. Import

## Spec (mandatory)

The `import` command loads Parquet output files into ClickHouse with an atomic table swap.

### Steps

1. Create staging tables with `_staging` suffix (same schemas as live tables)
2. Stream Parquet files batch-by-batch into their corresponding staging tables (bounded memory):
   - `ngram_counts.parquet` → `ngram_counts_staging`
   - `bucket_totals.parquet` → `bucket_totals_staging`
   - `ngram_vocabulary.parquet` → `ngram_vocabulary_staging`
   - `global_counts.parquet` → `global_counts_staging`
3. Swap each data table atomically:
   ```sql
   EXCHANGE TABLES ngram_counts AND ngram_counts_staging
   EXCHANGE TABLES bucket_totals AND bucket_totals_staging
   EXCHANGE TABLES ngram_vocabulary AND ngram_vocabulary_staging
   EXCHANGE TABLES global_counts AND global_counts_staging
   ```
4. Drop the old tables (now named `_staging`)
5. Load `ingest_log.parquet` into the `ingest_log` table (append, not swap — preserves audit history)

### Consistency

`EXCHANGE TABLES` swaps one pair at a time. There is a brief window between swaps where tables are mismatched. For a low-QPS site this is acceptable — queries during the swap may see slightly inconsistent data for a fraction of a second.

### After import

Once `import` completes, incremental `process` runs on the VPS pick up from the watermark recorded in `ingest_log`. Run `process` covering through the present day to fill any gap.

---

## Rationale

* atomic swap means zero downtime — the old data serves queries until the swap instant
* staging tables prevent partial loads from being visible
* `ingest_log.parquet` is loaded like any other table — no special manifest parsing
* the log entry ensures subsequent `process` runs know where the bootstrap left off

---

# 11. Idempotency

## Spec (mandatory)

Pipeline must guarantee:

```text
no duplicate (tokenizer_version, n, ngram, bucket)
```

### ClickHouse mode

Watermark from `ingest_log` ensures only new comments are processed. Re-running is a no-op if no new comments exist.

### Parquet mode

Full corpus processing from timestamp 0. Output files are a complete, self-contained snapshot. `import` does a full table swap, so there is no duplicate risk.

### Parquet mode crash recovery

Pass 1 writes sorted partial count files per month. Files with existing partials are skipped on re-run — delete a partial manually to reprocess that month. Pass 2 always re-runs from scratch (output directory is wiped).

ClickHouse mode has **no local files on prod**. All state lives in the database: watermark in `ingest_log`, vocabulary in `ngram_vocabulary`, global counts in `global_counts`.

### Tokenizer version guard

ClickHouse mode checks `ingest_log` for entries from a different tokenizer version and refuses to proceed. A tokenizer change requires a full rebuild via `process --output parquet` + `import`.

---

## Rationale

* ClickHouse does not enforce uniqueness at insert time — duplicates corrupt counts
* watermark provides simple, global progress tracking for incremental mode
* Parquet mode + atomic swap guarantees a clean slate
* prod has no manifest files to manage — all state is in ClickHouse

---

# 12. Process Flow Detail

### Parquet mode (`--output parquet`)

Two-pass pipeline. Always starts from scratch — no resume. If interrupted, re-run.

**Pass 1: Vocabulary Build (concurrent)**

For each Parquet file (concurrent, bounded to 2 files at a time):
1. Read and filter all comments (no watermark — full corpus)
2. Tokenize in parallel
3. Count n-grams
4. Write sorted partial global counts to disk
5. Skip files that already have partials (delete manually to reprocess)

**K-way merge + vocabulary admission (streaming)**

6. Stream-merge all sorted partial files using a BinaryHeap — O(num_files) memory
7. For each unique (n, ngram, total_count): apply admission thresholds, write to `global_counts.parquet` incrementally
8. Only admitted entries are kept in memory for pass 2

**Pass 2: Output (concurrent producers, serial writer)**

For each Parquet file (concurrent read/tokenize, serial parquet write):
1. Read and filter all comments
2. Tokenize in parallel
3. Count n-grams
4. Apply vocabulary filter + per-bucket pruning
5. Write to output Parquet files

After all files:
6. Write `ngram_vocabulary.parquet` from the admitted vocabulary
7. Write `ingest_log.parquet` with watermark, tokenizer version, stats, duration

### ClickHouse mode (`--output clickhouse`)

Single-pass incremental pipeline:

```text
process(data_dir, months, ch):
    check for tokenizer version mismatch in ingest_log
    watermark = latest ingest_log entry (0 if none)
    global_counts = ch.load_global_counts()
    vocabulary = ch.load_vocabulary()

    for each month in months:
        read comments after watermark from parquet file
        if empty: skip

        tokenize and count n-grams in parallel

        // Accumulate global counts across all runs
        global_counts += this month's global counts

        // Admit new vocabulary and update existing entries
        rebuild vocabulary from global_counts
        re-insert all vocabulary rows into ClickHouse (ReplacingMergeTree deduplicates)

        // Filter and insert counts
        filter counts against vocabulary + per-bucket thresholds
        insert ngram_counts and bucket_totals into ClickHouse

    write updated global_counts to ClickHouse
    insert ingest_log entry (watermark, stats, duration)
```

---

## Rationale

* Parquet mode: two-pass is required because global vocabulary thresholds need the full corpus
* K-way sorted merge keeps memory bounded during the merge of ~244 partial files (see `docs/design-decisions.md`)
* ClickHouse mode: single-pass with global_counts table enables vocabulary growth on prod
* all state on prod lives in ClickHouse — no local files
* both modes share the same tokenization, counting, and pruning logic — only the output sink and vocabulary source differ

---

# 13. Failure Handling

## Spec

* pipeline must be restartable at any point
* partial failures must not corrupt data

---

## Mechanism

**Parquet mode:** local manifest tracks completed files. Re-running skips completed files. If a file fails mid-processing, no partial output is written (flush at file boundary). Output Parquet files are only finalized after all files are processed.

**ClickHouse mode:** watermark from `ingest_log` provides the restart point. If processing fails before the log entry is written, no watermark advances — re-run retries from the same point.

**Import:** staging tables are loaded before the swap. If import fails mid-load, staging tables can be dropped and retried. The swap itself is atomic per table.

---

## Rationale

* long-running batch jobs are failure-prone
* all three commands provide simple, correct restart semantics

---

# 14. Memory Model

## Spec

* system must operate within bounded memory
* must not load entire corpus into memory
* peak memory = one Parquet file's comments + their n-gram aggregates

---

## Estimates

* largest monthly file: ~2M comments
* per-file n-gram aggregates: ~1-2 GB (dominated by bigram/trigram string keys)
* cumulative global counts: bounded by total unique n-grams seen across all runs
* total working memory target: < 4 GB

---

## Rationale

* bootstrap runs on a local dev machine with plenty of RAM
* incremental updates on the VPS process small monthly deltas
* 4 GB is comfortable for both environments

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
* recompute normalization at ingest time
* mutate historical data without full rebuild
* use `chrono` crate — use `time` crate to match `hn-clickhouse`

---

# 19. CLI Interface

## Spec (mandatory)

The ingest binary provides three subcommands.

### `ingest download`

Download Parquet files from HuggingFace.

```text
ingest download [OPTIONS]

Options:
  --data-dir <PATH>    Local storage directory [default: ./data/hn]
  --start <YYYY-MM>    First month to download [default: 2006-10]
  --end <YYYY-MM>      Last month to download [default: current month]
```

### `ingest process`

Tokenize, count, prune, and output results. Output mode determines behavior:

```text
ingest process [OPTIONS]

Options:
  --data-dir <PATH>    Directory with downloaded Parquet files [default: ./data/hn]
  --start <YYYY-MM>    First month to process [default: 2006-10]
  --end <YYYY-MM>      Last month to process [default: current month]
  --output <MODE>      Output mode: "clickhouse" or "parquet" [default: clickhouse]
```

**Parquet mode** (`--output parquet`):
* No ClickHouse connection required
* Builds vocabulary from scratch (two-pass)
* Writes output to `{data-dir}/output/`
* Processes full corpus (no watermark filtering)

**ClickHouse mode** (`--output clickhouse`):
* Reads watermark from `ingest_log` table
* Loads vocabulary and global counts from ClickHouse, expands with new admissions
* All state in ClickHouse — no local files on prod
* Inserts directly into ClickHouse
* Logs run to `ingest_log`

### `ingest import`

Load Parquet output files into ClickHouse with atomic table swap.

```text
ingest import [OPTIONS]

Options:
  --data-dir <PATH>    Directory containing output/ folder [default: ./data/hn]
```

Loads all Parquet files from `{data-dir}/output/` into staging tables, swaps atomically, and appends the `ingest_log.parquet` entry to the log table.

### Environment

ClickHouse connection (for `process --output clickhouse` and `import`):

```text
CLICKHOUSE_HOST     [default: localhost]
CLICKHOUSE_PORT     [default: 8123]
CLICKHOUSE_USER     [default: default]
CLICKHOUSE_PASSWORD [default: ""]
CLICKHOUSE_DATABASE [default: hn_ngram]
```

---

## Operational examples

### Bootstrap (local workstation → prod)

```bash
# On workstation
ingest download --start 2006-10 --end 2026-03
ingest process --output parquet --start 2006-10 --end 2026-03

# Transfer to prod
scp -r data/hn/output/ prod:/path/to/data/hn/output/

# On prod
ingest import --data-dir /path/to/data/hn
```

### Incremental update (on prod VPS)

```bash
ingest download --start 2026-03 --end 2026-03
ingest process --start 2026-03 --end 2026-03
```

### Full rebuild

Same as bootstrap. `import` does a full swap — the new data completely replaces the old. Run `process` after import to fill any gap between the bootstrap end date and now.

---

## Rationale

* three commands cover the full lifecycle with no redundancy
* `process` with output flag avoids duplicating core logic
* reasonable defaults mean `ingest download && ingest process` works out of the box
* `--start`/`--end` allows processing a subset for testing

---

# 20. Progress Reporting

## Spec (mandatory)

All output goes to stderr via `tracing` (structured logging).

### Per-file progress

```text
[INFO] Processing: data/2024/2024-01.parquet (1/12) — 1,832,451 comments
[INFO]   Comments: 1832451 | Counts: 234567 | Totals: 90 | Elapsed: 12.3s
```

### Vocabulary build (parquet mode)

```text
[INFO] Global counts: 1234567 unigrams, 12345678 bigram candidates, 8765432 trigram candidates
[INFO] Admitted: 456789 bigrams (of 12345678), 123456 trigrams (of 8765432)
```

### Watermark display (clickhouse mode)

```text
[INFO] Watermark: 1705318200000 (2024-01-15)
```

### Download progress

```text
[INFO] Downloading data/2024/2024-01.parquet (142/244) ... 45.2 MB in 3.1s
```

### Import progress

```text
[INFO] Loading ngram_counts.parquet into staging ... 12,345,678 rows in 8.2s
[INFO] Swapping ngram_counts ... done
[INFO] Ingestion log entry written — watermark: 1711584000000
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
* guarantees idempotency via watermark and/or atomic swap
* scales to full 41M-comment dataset
* operates within bounded memory (< 4 GB)
* produces deterministic output
* completes full processing in reasonable time (< 2 hours target)
* Parquet output can be imported with atomic swap on a fresh ClickHouse instance
* incremental updates via ClickHouse mode process only new comments

---

## Final Note for Agent

If proposing improvements:

* must preserve correctness of tokenization, aggregation, and normalization
* must not introduce duplicate counting
* must not increase asymptotic storage or query cost
* must use `time` crate, not `chrono`
