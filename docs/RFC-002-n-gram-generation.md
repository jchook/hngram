# RFC-002: N-gram Generation and Pruning Strategy

## Status

Draft → target: Accepted

## Purpose

Define how tokens become n-grams, how counts are aggregated, and how the system limits storage growth while preserving useful query coverage.

This RFC assumes RFC-001 tokenization is fixed and versioned.

---

## 1. Goals

The n-gram subsystem must:

* generate deterministic 1-grams, 2-grams, and 3-grams from tokenized HN comments
* support normalized relative-frequency queries over time
* scale to the full HN comments corpus plus incremental updates
* keep storage and query cost bounded
* preserve the phrases users are most likely to search

---

## 2. Non-goals

This RFC does not cover:

* raw comment search
* semantic matching
* stemming or lemmatization
* synonym expansion
* approximate phrase matching
* frontend query parsing beyond basic phrase splitting

---

## 3. Definitions

A **token** is the output of RFC-001.

An **n-gram** is a contiguous sequence of `n` tokens.

A **bucket** is the base time unit used for storage. Per the PRD, this is **one day**.

A **series** is the per-bucket count history for a specific n-gram.

A **denominator** is the total number of n-grams of order `n` in a bucket.

---

## 4. Product decision

### Supported orders

v1 supports:

* unigrams
* bigrams
* trigrams

No 4-grams or above in v1.

Reason:

* most user value is in 1–3 grams
* storage grows sharply after 3
* query intent is usually captured by 1–3 token phrases

---

## 5. Generation rules

## 5.1 Input

For each eligible comment, the tokenizer emits:

```text
[t1, t2, t3, ..., tk]
```

Eligibility is defined elsewhere, but in practice this means comments with visible text after HTML stripping and normalization.

---

## 5.2 Sliding window generation

For a token sequence of length `k`:

* unigrams: emit `k`
* bigrams: emit `max(k - 1, 0)`
* trigrams: emit `max(k - 2, 0)`

Example:

```text
["machine", "learning", "is", "useful"]
```

Emits:

* 1-grams:

  * `machine`
  * `learning`
  * `is`
  * `useful`

* 2-grams:

  * `machine learning`
  * `learning is`
  * `is useful`

* 3-grams:

  * `machine learning is`
  * `learning is useful`

N-grams are contiguous only.

No skip-grams.

---

## 5.3 Join representation

N-grams are serialized as tokens joined by a single ASCII space:

* unigram: `"rust"`
* bigram: `"machine learning"`
* trigram: `"large language model"`

No other separator is allowed.

---

## 5.4 Comment boundaries

N-grams may not cross comment boundaries.

Each comment is processed independently.

---

## 6. Counting model

## 6.1 Numerators

For each daily bucket and each n-gram:

```text
count(bucket, n, ngram) = total occurrences of that ngram in all comments in that bucket
```

This is **occurrence count**, not unique-comment count.

If a comment contains `"ai"` five times, that contributes five unigram occurrences of `"ai"`.

---

## 6.2 Denominators

For each bucket and order `n`:

```text
total_ngrams(bucket, n) = total emitted n-grams of order n in all comments in that bucket
```

Examples:

If a bucket has comments whose token lengths are:

* comment A: 4 tokens
* comment B: 2 tokens

Then:

* total unigrams = `4 + 2 = 6`
* total bigrams = `(4-1) + (2-1) = 4`
* total trigrams = `(4-2) + max(2-2, 0) = 2`

These denominators are required for normalization.

---

## 6.3 Relative frequency

At query time:

```text
relative_frequency(bucket, n, ngram) = count(bucket, n, ngram) / total_ngrams(bucket, n)
```

This is the primary metric.

---

## 7. Processing model

## 7.1 Local aggregation before database insert

The indexer must not emit one database row per n-gram occurrence.

Instead it should:

* process a chunk of comments
* accumulate counts in memory
* flush aggregated counts periodically

This reduces write amplification dramatically.

### Recommended local structures

Per worker:

* `HashMap<(bucket, n, ngram), count>`
* `HashMap<(bucket, n), total_count>`

Then merge worker outputs before loading into ClickHouse.

---

## 7.2 Parallelism

Parallelization unit should be one of:

* one source file
* one file segment / row group
* one batch of comments

Workers must produce deterministic final counts regardless of execution order.

Since counting is commutative and associative, this is straightforward.

---

## 8. Pruning strategy

This is the main policy decision.

We want:

* good query coverage
* bounded storage
* fast queries

We do **not** want to store every rare bigram and trigram ever seen.

## 8.1 v1 pruning policy

### Unigrams

Store all unigrams with count ≥ 1.

No pruning in v1.

Reason:

* unigrams are highly queryable
* manageable cardinality
* useful for long-tail exploration

### Bigrams

Store only bigrams whose **global corpus count** is at least `MIN_BIGRAM_COUNT`.

Initial default:

```text
MIN_BIGRAM_COUNT = 20
```

### Trigrams

Store only trigrams whose **global corpus count** is at least `MIN_TRIGRAM_COUNT`.

Initial default:

```text
MIN_TRIGRAM_COUNT = 10
```

These defaults are intentionally conservative and should be revisited after measuring corpus cardinality.

---

## 8.5 Empirical Threshold Validation

### Spec (mandatory before production)

Before finalizing thresholds, run pass 1 on a representative sample (e.g., 1-2 years of data) and measure:

* total unique bigrams at thresholds: 5, 10, 20, 50
* total unique trigrams at thresholds: 5, 10, 20, 50
* estimated storage size at each threshold
* coverage: what percentage of query-likely phrases are retained?

Document final chosen values with justification.

### Acceptance criteria

* storage remains under target budget
* common technical phrases (e.g., "machine learning", "large language model") are retained
* rare garbage phrases are excluded

---

## 8.2 Why prune by global count

Prune by **global total count across all time**, not by per-day count.

Reason:

* preserves phrases that recur over long periods
* removes one-off garbage
* easy to compute in a two-pass or staged pipeline
* stable and intuitive

---

## 8.3 Why not TF-IDF or entropy pruning

Those methods are interesting for phrase discovery, but wrong for the main viewer index.

The viewer’s job is to answer:

* “show me this phrase over time”

not:

* “show me statistically interesting phrases”

So pruning should be simple, stable, and based on storage economics.


---

## 8.4 Per-Bucket Pruning (Sparse Series Filtering)

### Purpose

Reduce storage and query overhead by eliminating low-signal, low-frequency n-gram occurrences at the **per-bucket level**, after n-gram generation and aggregation.

This operates independently of (and in addition to) global vocabulary admission thresholds.

---

### Definition

For each `(bucket, n, ngram)` after aggregation:

Only retain rows where:

```text
count(bucket, n, ngram) ≥ MIN_BUCKET_COUNT[n]
```

Rows failing this condition are discarded and not written to the serving store.

---

### Default Thresholds (v1)

```text
MIN_BUCKET_COUNT:
  unigram: 1
  bigram: 3
  trigram: 5
```

Rationale:

* **Unigrams (1):** Preserve full signal; baseline corpus statistics depend on complete coverage.
* **Bigrams (3):** Removes incidental co-occurrences while preserving meaningful usage.
* **Trigrams (5):** Filters out the vast majority of noise; trigrams are highly sparse and otherwise explode row count.

---

### Important Constraint

Per-bucket pruning **must not affect denominator calculations**.

* `total_ngrams(bucket, n)` must be computed from the **full, unpruned set of emitted n-grams**
* Only numerator rows (`count(bucket, n, ngram)`) are pruned

Failure to preserve full denominators will produce incorrect relative frequencies.

---

### Processing Order

Per-bucket pruning is applied **after local aggregation** and **before persistence**:

```text
Tokenize
→ Generate n-grams
→ Aggregate (per bucket)
→ Apply per-bucket pruning
→ Apply global vocabulary filter (for n ≥ 2)
→ Store results
```

Order of the final two steps may be swapped if more efficient, provided semantics remain identical.

---

### Effects

#### Benefits

* Significantly reduces row count in `ngram_counts`
* Improves query latency and scan efficiency
* Eliminates low-value noise from time series
* Improves compression characteristics in ClickHouse

#### Tradeoffs

* Early or rare occurrences of phrases may be omitted
* Very sparse trends may appear to “start later” than they actually did

This is an intentional tradeoff in favor of performance and clarity.

---

### Configuration

Thresholds must be configurable at build time:

```text
MIN_BUCKET_COUNT_BIGRAM
MIN_BUCKET_COUNT_TRIGRAM
```

Unigram threshold is fixed at 1 in v1.

---

### Future Considerations (Non-v1)

* Adaptive thresholds based on bucket size
* Separate storage tier for sparse series
* Optional “include sparse data” query mode

Not required for initial implementation.


---

## 9. Build strategy

Because pruning depends on global counts, v1 should use a staged build.

## 9.1 Historical backfill

### Pass 1: global frequency estimation

Compute total counts for all candidate n-grams:

* all unigrams
* all bigrams
* all trigrams

Output:

* `global_ngram_counts(n, ngram, total_count)`

### Pass 2: filtered daily series build

Using thresholds from pass 1:

* keep all unigrams
* keep only admitted bigrams/trigrams
* produce daily series and denominators

This two-pass design is simpler and produces a cleaner final index.

---

## 9.2 Incremental updates after backfill

For live updates:

* process only new data (e.g., "today" partitions)
* only count n-grams that already exist in the admitted vocabulary for 2-grams and 3-grams
* all unigrams continue to be counted

This avoids vocabulary explosion during steady-state updates.

---

## 9.3 Pending Vocabulary (Recommended)

### Problem

Incremental updates only count admitted n-grams. New phrases that emerge after historical build (e.g., "llama 3" appearing in 2024) will not be indexed until vocabulary re-admission.

### Solution

Maintain a **pending vocabulary** table:

```text
pending_ngrams(
  tokenizer_version,
  n,
  ngram,
  first_seen,
  running_count
)
```

During incremental updates:

* for n-grams NOT in admitted vocabulary, increment `running_count` in pending table
* do NOT write to `ngram_counts` yet

### Promotion job (periodic)

Run weekly or monthly:

* scan `pending_ngrams` where `running_count >= threshold`
* promote to `admitted_ngrams`
* backfill historical buckets if desired (optional)
* clear promoted entries from pending table

### Rationale

* new technical terms get indexed eventually
* avoids unbounded vocabulary growth during normal operation
* keeps incremental path fast

### Flexibility

This is recommended but not required for v1. Initial deployment may simply re-run full admission periodically.

---

## 10. Vocabulary admission model

To support pruning cleanly, define a vocabulary table:

```text
admitted_ngrams(
  tokenizer_version,
  n,
  ngram,
  admitted_at,
  global_count_at_admission
)
```

Rules:

* all unigrams are implicitly admitted
* bigrams/trigrams must be explicitly admitted after historical pass
* incremental jobs consult this vocabulary

This keeps the ingest rules explicit and versionable.

---

## 11. Handling query misses

If a user queries a bigram or trigram not present in the admitted vocabulary:

* return a zero-valued series
* include metadata indicating it is not indexed

Do not do raw fallback scanning in v1.

Reason:

* keeps API behavior simple
* prevents surprise slow queries
* avoids a second query engine path

Future versions may add "slow path" raw scans, but not now.

---

## 11.1 Query Result Status (Required)

### Spec

API response must distinguish between:

1. **`indexed`**: phrase is in vocabulary, data returned (may be zero in some buckets)
2. **`not_indexed`**: phrase is NOT in vocabulary (too rare historically)
3. **`invalid`**: phrase failed validation (e.g., > 3 tokens)

### Response structure

```json
{
  "series": [
    {
      "phrase": "machine learning",
      "status": "indexed",
      "points": [...]
    },
    {
      "phrase": "xyzzy foobar baz",
      "status": "not_indexed",
      "points": []
    }
  ]
}
```

### Rationale

Frontend must clearly communicate to users:

* "This phrase had zero occurrences in this time range" (indexed, legitimately zero)
* "This phrase is not indexed because it was historically too rare" (not_indexed)

These are different user experiences and require different messaging.

---

## 12. Canonical examples

## Example A

Input tokens:

```text
["rust", "is", "fast"]
```

Emits:

* 1:

  * `rust`
  * `is`
  * `fast`
* 2:

  * `rust is`
  * `is fast`
* 3:

  * `rust is fast`

Denominator contributions:

* unigram total += 3
* bigram total += 2
* trigram total += 1

---

## Example B

Input tokens:

```text
["ai"]
```

Emits:

* 1:

  * `ai`

Denominator contributions:

* unigram total += 1
* bigram total += 0
* trigram total += 0

---

## Example C

Input tokens:

```text
[]
```

Emits nothing.

Denominator contributions:

* all += 0

---

## 13. Storage-shaping rules

## 13.1 No duplicate serialization variants

The same token sequence must always map to the same serialized n-gram string.

This follows directly from RFC-001 plus single-space join.

---

## 13.2 No per-comment storage in serving DB

The serving DB stores only:

* aggregate n-gram counts
* denominators
* optionally admission metadata

It does not store raw per-comment n-gram events in v1.

---

## 13.3 Optional compression improvement

During build, repeated n-grams may be interned locally or dictionary-encoded before flush.

This is an implementation detail, not part of the logical data model.

---

## 14. API-facing behavior

The query layer receives phrases from users.

For each phrase:

1. tokenize it using the exact same tokenizer version
2. count tokens
3. reject if token count is not 1, 2, or 3
4. serialize with single spaces
5. query the corresponding series

This guarantees query/build alignment.

### Important

User input is not matched as raw string equality. It is matched through the tokenizer.

So:

* query `"Node.js"` becomes `"node.js"`
* query `"DON'T"` becomes `"don't"`

---

## 15. Error handling and edge cases

## 15.1 Empty query after tokenization

If a user query tokenizes to zero tokens:

* reject as invalid query

## 15.2 Query token count > 3

Reject in v1.

## 15.3 Repeated tokens

Allowed.

Examples:

* `"ha ha"`
* `"very very good"`

No special treatment.

---

## 16. Testing strategy

## 16.1 Golden generation tests

Given a token vector, emitted n-grams and denominator contributions must match exactly.

## 16.2 Aggregation tests

Given a set of comments in one bucket, resulting counts and totals must match expected values.

## 16.3 Pruning tests

Given a synthetic corpus with known global counts:

* admitted bigrams/trigrams must match thresholds exactly

## 16.4 Query normalization tests

Ensure user phrases go through the same tokenizer and map to the same stored form.

---

## 17. Versioning

This RFC depends on `tokenizer_version`.

Any tokenizer change invalidates the n-gram vocabulary and aggregate counts.

Therefore:

* n-gram data must be tagged with tokenizer version
* vocabulary must be tagged with tokenizer version
* a tokenizer change requires either:

  * full rebuild, or
  * parallel storage of a new corpus version

Silent tokenizer drift is forbidden.

---

## 18. Open questions

### Q1. Should we store all bigrams too?

Tentative answer: no. Start with thresholding.

### Q2. Should trigrams have a lower or higher threshold than 10?

Needs corpus measurement. The default is a placeholder.

### Q3. Should we support unique-comment frequency in addition to occurrence frequency?

Not in v1. Could be a future alternate metric.

### Q4. Should we support stopword filtering?

No. Filtering stopwords would distort phrase semantics and denominators.

---

## 19. Acceptance criteria

This RFC is accepted when the implementation can:

* generate deterministic 1/2/3-grams from RFC-001 tokens
* compute exact daily denominators
* perform historical two-pass admission for bigrams/trigrams
* support incremental updates against admitted vocabulary
* reject unsupported query phrases cleanly
* pass golden tests for generation, aggregation, and pruning

---

## 20. Recommended defaults

For v1, lock in:

* base bucket: day
* supported n: 1, 2, 3
* unigram pruning: none
* bigram admission threshold: 20 global occurrences
* trigram admission threshold: 10 global occurrences
* counting model: total occurrences
* normalization denominator: total n-grams of same order in bucket
* no raw fallback query path

---

## Final recommendation

This should stay boring.

The winning design is:

* deterministic tokenizer
* simple sliding windows
* exact denominators
* aggressive enough pruning for storage sanity
* no cleverness at query time

That will make the system fast, explainable, and stable.

Next should be **RFC-003: ClickHouse schema, partitioning, and query model**.

