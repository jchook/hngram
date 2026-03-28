# RFC-004

## Rust Ingestion + Processing Pipeline

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
* ingestion log for watermark tracking
* idempotency guarantees
* CLI interface
* progress reporting

---

## 1. Pipeline Overview

Three subcommands handle the full lifecycle:

```text
ingestion download   — fetch raw Parquet from HuggingFace
ingestion process    — tokenize, count, prune → output parquet or direct to ClickHouse
ingestion import     — load parquet into staging tables, atomic swap to live
```

### Why two output modes?

Processing the full corpus (~41M comments) is CPU- and disk-intensive. The prod environment is a small, inexpensive VPS that cannot handle this workload in reasonable time. We assume the developer has a local workstation that is significantly faster. The Parquet output mode exists to **bootstrap prod once**: process the full corpus locally, transfer the output files, and import them. After that, all ongoing ingestion happens directly on the VPS via ClickHouse mode — daily or monthly deltas are small enough to process on modest hardware.

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
| **Watermark source** | 0 (full corpus) | `ingestion_log` table |
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

* processing is parallelized within each file using rayon
* unit of parallelism: 1024-comment chunks processed in parallel
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
3. Write output (append to Parquet files or batch insert into ClickHouse)
4. Record progress in local manifest

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
  manifest.json
```

Parquet schemas match the ClickHouse table schemas exactly (see §9.3). This allows `import` to load them with a direct `INSERT INTO ... SELECT * FROM file(...)`.

The `manifest.json` contains metadata for `import`:

```json
{
  "tokenizer_version": "1",
  "last_ingested_ts": 1711584000000,
  "start_month": "2006-10",
  "end_month": "2026-03"
}
```

### Vocabulary strategy

In Parquet mode, vocabulary is built from scratch using a two-pass approach:

1. **Pass 1**: Process all files, accumulate global counts, write partial TSVs
2. **Merge**: Combine all partials, apply `build_vocabulary()` thresholds
3. **Pass 2**: Re-process all files, filter against vocabulary, write output Parquet

This ensures the vocabulary reflects the full corpus.

## 9.2 ClickHouse Output (`--output clickhouse`, default)

Direct batch inserts into `ngram_counts`, `bucket_totals`, and `ngram_vocabulary` tables.

### Vocabulary strategy

In ClickHouse mode, vocabulary is read from the database at startup and cumulative global counts are maintained. As new comments are processed, global counts increase, and n-grams that newly cross admission thresholds are delta-inserted into the vocabulary table. Previously admitted n-grams are never dropped (append-only). Vocabulary only grows over time.

### Watermark

Read from the `ingestion_log` table (0 if no entries). Only comments with `time > watermark` are processed. After successful processing, a new `ingestion_log` entry is written.

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

### `ingestion_log`

```sql
CREATE TABLE ingestion_log (
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

The `ingestion_log` is an append-only audit trail. Each `process` (ClickHouse mode) and `import` run appends one row. The watermark is read as:

```sql
SELECT last_ingested_ts FROM ingestion_log ORDER BY id DESC LIMIT 1
```

---

## Rationale

* Parquet output decouples heavy processing from ClickHouse — no DB needed on the workstation
* ClickHouse reads Parquet natively — no custom format or protocol
* `ingestion_log` replaces the manifest for prod state — single source of truth in the DB
* UUIDv7 is time-sortable, so `ORDER BY id DESC LIMIT 1` always gives the latest entry
* audit trail is free and useful for debugging

---

# 10. Import

## Spec (mandatory)

The `import` command loads Parquet output files into ClickHouse with an atomic table swap.

### Steps

1. Read `manifest.json` from the output directory
2. Create staging tables with `_staging` suffix (same schemas as live tables)
3. Load Parquet files into staging tables:
   ```sql
   INSERT INTO ngram_counts_staging SELECT * FROM file('ngram_counts.parquet')
   INSERT INTO bucket_totals_staging SELECT * FROM file('bucket_totals.parquet')
   INSERT INTO ngram_vocabulary_staging SELECT * FROM file('ngram_vocabulary.parquet')
   ```
4. Swap each table atomically:
   ```sql
   EXCHANGE TABLES ngram_counts AND ngram_counts_staging
   EXCHANGE TABLES bucket_totals AND bucket_totals_staging
   EXCHANGE TABLES ngram_vocabulary AND ngram_vocabulary_staging
   ```
5. Drop the old tables (now named `_staging`)
6. Write an `ingestion_log` entry with the watermark from `manifest.json`

### Consistency

`EXCHANGE TABLES` swaps one pair at a time. There is a brief window between swaps where tables are mismatched. For a low-QPS site this is acceptable — queries during the swap may see slightly inconsistent data for a fraction of a second.

### After import

Once `import` completes, incremental `process` runs on the VPS pick up from the watermark recorded in `ingestion_log`. Run `process` covering through the present day to fill any gap.

---

## Rationale

* atomic swap means zero downtime — the old data serves queries until the swap instant
* staging tables prevent partial loads from being visible
* `ingestion_log` entry ensures `process` knows where the bootstrap left off

---

# 11. Idempotency

## Spec (mandatory)

Pipeline must guarantee:

```text
no duplicate (tokenizer_version, n, ngram, bucket)
```

### ClickHouse mode

Watermark from `ingestion_log` ensures only new comments are processed. Re-running is a no-op if no new comments exist.

### Parquet mode

Full corpus processing from timestamp 0. Output files are a complete, self-contained snapshot. `import` does a full table swap, so there is no duplicate risk.

### Local manifest

A local manifest file at `{data-dir}/manifest.json` tracks which input files have been processed, enabling resume after crashes. This file is local-only — it does not exist on prod.

```json
{
  "tokenizer_version": "1",
  "files_completed": ["data/2024/2024-01.parquet", ...]
}
```

### Tokenizer version change

If `tokenizer_version` in the local manifest does not match the current `TOKENIZER_VERSION`, the pipeline errors and requires manifest deletion. A tokenizer change requires a full rebuild.

---

## Rationale

* ClickHouse does not enforce uniqueness at insert time — duplicates corrupt counts
* watermark provides simple, global progress tracking for incremental mode
* Parquet mode + atomic swap guarantees a clean slate
* local manifest enables crash recovery without polluting prod state

---

# 12. Process Flow Detail

### Parquet mode (`--output parquet`)

Two-pass pipeline with chunk-and-merge strategy:

**Pass 1: Vocabulary Build**

For each Parquet file:
1. Read and filter all comments (no watermark — full corpus)
2. Tokenize in parallel (rayon, 1024-comment chunks)
3. Count n-grams using `NgramCounter`
4. Write partial global counts to `{data-dir}/partial/{YYYY-MM}.counts`
5. Mark file done in local manifest

After all files:
6. Merge all partial count files into total global counts
7. Apply `build_vocabulary()` with `PruningConfig` thresholds

**Pass 2: Output**

For each Parquet file:
1. Read and filter all comments
2. Tokenize in parallel
3. Count n-grams using `NgramCounter`
4. Apply vocabulary filter + per-bucket pruning
5. Append to output Parquet files (`ngram_counts.parquet`, `bucket_totals.parquet`)
6. Mark file done in local manifest

After all files:
7. Write `ngram_vocabulary.parquet` from the admitted vocabulary
8. Write `manifest.json` with watermark (max timestamp seen) and metadata

### ClickHouse mode (`--output clickhouse`)

Single-pass incremental pipeline:

```text
process(data_dir, months, ch):
    watermark = latest ingestion_log entry (0 if none)
    prev_vocab = ch.load_vocabulary()
    max_ts = watermark

    for each month in months:
        comments = read_comments_after(parquet_path, watermark)
        if comments.is_empty(): continue

        counter = process_comments_parallel(&comments)
        max_ts = max(max_ts, max timestamp in comments)

        // Update cumulative global counts
        global_counts += counter.global_counts()

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

    // Log the run
    ch.insert_ingestion_log(entry with max_ts, stats, duration)
```

---

## Rationale

* Parquet mode: two-pass is required because global vocabulary thresholds need the full corpus
* ClickHouse mode: single-pass with read-only vocabulary keeps prod updates simple
* both modes share the same tokenization, counting, and pruning logic — only the output sink differs

---

# 13. Failure Handling

## Spec

* pipeline must be restartable at any point
* partial failures must not corrupt data

---

## Mechanism

**Parquet mode:** local manifest tracks completed files. Re-running skips completed files. If a file fails mid-processing, no partial output is written (flush at file boundary). Output Parquet files are only finalized after all files are processed.

**ClickHouse mode:** watermark from `ingestion_log` provides the restart point. If processing fails before the log entry is written, no watermark advances — re-run retries from the same point.

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
* `NgramCounter` for 2M comments: ~1-2 GB (dominated by bigram/trigram string keys)
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
* recompute normalization at ingestion time
* mutate historical data without full rebuild
* use `chrono` crate — use `time` crate to match `hn-clickhouse`

---

# 19. CLI Interface

## Spec (mandatory)

The ingestion binary provides three subcommands.

### `ingestion download`

Download Parquet files from HuggingFace.

```text
ingestion download [OPTIONS]

Options:
  --data-dir <PATH>    Local storage directory [default: ./data/hn]
  --start <YYYY-MM>    First month to download [default: 2006-10]
  --end <YYYY-MM>      Last month to download [default: current month]
```

### `ingestion process`

Tokenize, count, prune, and output results. Output mode determines behavior:

```text
ingestion process [OPTIONS]

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
* Reads watermark from `ingestion_log` table
* Uses existing vocabulary (read-only, no expansion)
* Inserts directly into ClickHouse
* Logs run to `ingestion_log`

### `ingestion import`

Load Parquet output files into ClickHouse with atomic table swap.

```text
ingestion import [OPTIONS]

Options:
  --data-dir <PATH>    Directory containing output/ folder [default: ./data/hn]
```

Reads `{data-dir}/output/manifest.json` for metadata. Creates staging tables, loads Parquet files, swaps atomically, writes `ingestion_log` entry.

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
ingestion download --start 2006-10 --end 2026-03
ingestion process --output parquet --start 2006-10 --end 2026-03

# Transfer to prod
scp -r data/hn/output/ prod:/path/to/data/hn/output/

# On prod
ingestion import --data-dir /path/to/data/hn
```

### Incremental update (on prod VPS)

```bash
ingestion download --start 2026-03 --end 2026-03
ingestion process --start 2026-03 --end 2026-03
```

### Full rebuild

Same as bootstrap. `import` does a full swap — the new data completely replaces the old. Run `process` after import to fill any gap between the bootstrap end date and now.

---

## Rationale

* three commands cover the full lifecycle with no redundancy
* `process` with output flag avoids duplicating core logic
* reasonable defaults mean `ingestion download && ingestion process` works out of the box
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
