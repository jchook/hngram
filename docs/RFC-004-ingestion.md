# RFC-004 (Agent-Oriented)

## Rust Ingestion + Processing Pipeline

---

## 0. Scope

Define:

* data ingestion from Parquet (HN dataset)
* tokenization integration (RFC-001)
* n-gram generation + pruning (RFC-002)
* aggregation strategy
* ClickHouse loading
* incremental updates
* idempotency guarantees

---

## 1. Pipeline Overview

## Spec (mandatory)

Pipeline stages:

```text
Fetch → Parse → Filter → Tokenize → Generate ngrams
      → Aggregate (local)
      → Apply pruning (per-bucket + vocabulary)
      → Flush aggregates
      → Insert into ClickHouse
```

---

## Rationale

* separates concerns
* enables parallelism
* ensures aggregation happens before DB writes

---

## Flexibility

* stages may be fused for performance
* streaming or batch execution allowed

---

# 2. Input Data Source

## Spec

* source: Hugging Face Parquet dataset
* read only rows where:

  * `type = comment`
  * `deleted = false`
  * `dead = false`
  * `text IS NOT NULL`

---

## Rationale

* ensures only valid visible comments are processed

---

## Flexibility

* agent may push filtering into Parquet scan if supported

---

# 3. Execution Model

## Spec

* processing must be parallelized
* unit of parallelism:

  * file OR
  * row group OR
  * chunk of rows

---

## Requirements

* aggregation must be **commutative and deterministic**
* final results must be identical regardless of execution order

---

## Rationale

* ensures correctness under concurrency

---

## Flexibility

* agent may choose:

  * thread pool (rayon)
  * async pipeline
  * work-stealing scheduler

---

# 4. Tokenization

## Spec

* must use RFC-001 tokenizer exactly
* tokenizer version must be attached to all output

---

## Constraints

* no alternative tokenization allowed
* no NLP libraries

---

## Rationale

* consistency is required for correctness

---

# 5. N-gram Generation

## Spec

* must follow RFC-002
* generate n ∈ {1,2,3}
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

* agent may:

  * shard maps per thread
  * use lock-free structures
  * spill to disk if memory constrained

---

# 7. Pruning

## Spec

Apply both:

1. per-bucket pruning (RFC-002 §8.4)
2. global vocabulary filtering (for n ≥ 2)

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

## Spec

Aggregates must be flushed periodically to avoid unbounded memory usage

---

## Trigger conditions

* memory threshold exceeded OR
* row count threshold exceeded OR
* end of input partition

---

## Output format

* rows grouped by:

  * `(tokenizer_version, n, ngram, bucket)`
* rows for `bucket_totals`

---

## Rationale

* prevents OOM
* enables streaming ingestion

---

## Flexibility

* agent may tune thresholds dynamically

---

# 9. ClickHouse Insert

## Spec

* insert using batch inserts
* insert only aggregated rows
* insert size ≥ 10k rows (recommended)

---

## Requirements

* no duplicate keys
* consistent ordering not required

---

## Rationale

* ClickHouse optimized for large batch inserts

---

## Flexibility

* agent may use:

  * HTTP interface
  * native protocol
  * buffered writers

---

# 10. Idempotency

## Spec (mandatory)

Pipeline must guarantee:

```text
no duplicate (tokenizer_version, n, ngram, bucket)
```

---

## Required mechanism

At least one of:

### Option A (preferred)

* track processed partitions/files
* never process same data twice

### Option B

* deterministic rebuild + overwrite

---

## Rationale

* ClickHouse does not enforce uniqueness
* duplicates corrupt counts

---

## Flexibility

* agent may implement:

  * manifest DB
  * checkpoint files
  * hash-based verification

---

# 11. Historical Backfill

## Spec

Two-pass pipeline:

### Pass 1

* compute global ngram counts
* build vocabulary (bigrams + trigrams)

### Pass 2

* generate daily aggregates
* apply vocabulary + pruning
* insert into ClickHouse

---

## Rationale

* required for global threshold pruning

---

## Flexibility

* agent may approximate pass 1 if memory constrained

---

# 12. Incremental Updates

## Spec

* process only new data (e.g. “today” partitions)
* update current day bucket

---

## Constraints

* must use existing vocabulary for n ≥ 2
* must not expand vocabulary during incremental updates

---

## Rationale

* prevents unbounded growth
* keeps system stable

---

## Flexibility

* periodic re-admission of vocabulary allowed (offline job)

---

# 13. Failure Handling

## Spec

* pipeline must be restartable
* partial failures must not corrupt data

---

## Required

* checkpointing OR partition-level atomicity

---

## Rationale

* long-running batch jobs are failure-prone

---

## Flexibility

* agent may use:

  * per-partition completion markers
  * transactional staging tables

---

# 14. Memory Model

## Spec

* system must operate within bounded memory
* must not load entire corpus into memory

---

## Rationale

* dataset is large (tens of millions of rows)

---

## Flexibility

* agent may:

  * chunk input
  * spill aggregates to disk
  * merge partial aggregates

---

# 15. Determinism

## Spec

* same input → identical output
* independent of:

  * thread scheduling
  * execution order

---

## Rationale

* required for reproducibility and debugging

---

# 16. Performance Targets

## Spec

* must saturate CPU cores during batch processing
* must process millions of comments per minute (target)

---

## Rationale

* ensures feasible full rebuild time

---

## Flexibility

* agent may optimize:

  * batching
  * SIMD tokenization
  * parallel aggregation

---

# 17. Output Guarantees

For every processed bucket:

* correct counts for all retained ngrams
* correct denominators
* no duplicates
* all rows tagged with tokenizer_version

---

# 18. Prohibited Designs

Agent must NOT:

* write per-comment ngram rows to ClickHouse
* rely on ClickHouse for aggregation
* use non-deterministic tokenization
* recompute normalization at ingestion
* mutate historical data without full rebuild

---

# 19. Acceptance Criteria

Pipeline is valid if:

* produces correct aggregates
* respects pruning rules
* guarantees idempotency
* scales to full dataset
* supports incremental updates
* produces deterministic output

---

## Final Note for Agent

If proposing improvements:

* must preserve correctness of:

  * tokenization
  * aggregation
  * normalization
* must not introduce duplicate counting
* must not increase asymptotic storage or query cost

