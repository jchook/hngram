# Ingest Sizing: Settings, Output, and Memory

A working theory of how the knobs in `ingest.*.toml` translate to output size, peak RAM, and wall-clock time. Calibrated against two real builds on the HN comment corpus (2006-10 → 2026-05, ~50M comments, ~5B tokens).

## 1. Observed Data Points

### 1.1 Dev profile (currently on prod)

```toml
max_n              = 3
bucket_granularity = daily
min_global         = 0 / 500 / 200    (n=1/2/3)
min_bucket         = 5 / 10 / 15
min_flush          = 1 / 2  / 3
```

| Metric | Value |
|--------|-------|
| Source corpus | ~12GB raw HF parquet |
| Output `ngram_counts` | **20.5M rows** |
| ClickHouse on-disk | ~1.5GB |
| Peak RAM during build | ~6GB |
| Build wall-clock | ~30 min on dev workstation |

### 1.2 Full profile (completed 2026-05-07)

```toml
max_n              = 5
bucket_granularity = monthly
min_global         = 0 / 15 / 8 / 5 / 3
min_bucket         = 1 / 2  / 2 / 2 / 1
min_flush          = 1 / 2  / 2 / 2 / 2
max_entries        = 25_000_000   (override; toml says 200M — overridden for 30GB RAM box)
```

**Pipeline observables:**

| Metric | Value |
|--------|-------|
| Source corpus | 12GB raw HF parquet |
| Partial files on disk | 24GB in 1,912 shard files |
| Per-flush entries | 25–33M pre-prune → 2.5–3.3M post-prune (~10× reduction) |
| Total partial flushes | 239 |
| Source-processing wall-clock | ~7h on 8 vCPU / 30GB RAM (with OOM retries) |
| Merge phase (streaming, both passes) | **52 min** on same box |
| Peak RAM in merge phase | ~12-15GB (RFC-010 streaming merge) |

**Output (4.0GB total):**

| File | Rows | Size | B/row |
|------|------|------|-------|
| `ngram_counts.parquet` | **317,397,990** | 3,412 MB | 10.7 |
| `global_counts.parquet` | 45,844,785 (post min_global_export=3 filter) | 572 MB | 12.5 |
| `ngram_vocabulary.parquet` | 23,360,687 admitted | 304 MB | 13.0 |
| `bucket_totals.parquet` | 1,162 | 0.01 MB | 8.1 |
| `ingest_log.parquet` | 1 | 0.003 MB | — |

**`ngram_counts` rows by n:**

| n | Rows | % of total | Avg per (bucket, n) |
|---|------|-----------|---------------------|
| 1 | 25,957,894 | 8.2% | 111,407 |
| 2 | 100,568,605 | **31.7%** | 431,625 |
| 3 | 112,620,933 | **35.5%** | 485,435 |
| 4 | 53,331,062 | 16.8% | 229,875 |
| 5 | 24,919,496 | 7.9% | 107,412 |

**Vocabulary admitted by n:**

| n | Candidates (post-flush globals) | Admitted | Admission rate |
|---|---------------------------------|----------|----------------|
| 1 | 1,088,632 | 1,088,632 | 100% (`min_global=0`) |
| 2 | 6,681,252 | 2,139,671 | **32%** |
| 3 | 14,176,586 | 4,943,278 | **35%** |
| 4 | 13,565,509 | 4,856,300 | 36% |
| 5 | 10,332,806 | 10,332,806 | 100% (threshold = export floor) |

The "100% admission" for n=5 is an artifact: `min_global_export = min(non-zero min_global across n) = 3` (set by n=5), so the global_counts file already pre-filters everything to count≥3, identical to the n=5 admission threshold. n=2..4 have higher thresholds, hence their lower rates.

**Top global counts (eyeballing):** n=1 `max_global = 107,449,960` (the most-frequent unigram appears ~107M times), n=2 max 9.3M, n=3 max 1.86M, n=4 max 256k, n=5 max 80k.

## 2. Variables and Their Effects

### 2.1 `max_n`

Each n contributes an independent slice. Updated with actuals:

| n | Post-flush candidates (HN, monthly) | Admitted at the §1.2 thresholds |
|---|-------------------------------------|---------------------------------|
| 1 | 1.09M | 1.09M (always) |
| 2 | 6.68M | 2.14M (≥15) |
| 3 | 14.18M | 4.94M (≥8) |
| 4 | 13.57M | 4.86M (≥5) |
| 5 | 10.33M | 10.33M (≥3) |

The shape is *not* monotonic with n — n=3 has the most candidates and the most output rows, with n=4 already shrinking and n=5 dropping further. This is because more aggressive `min_flush` thresholds at higher n, plus the rapid Zipfian thinning of high-n vocabulary, beat the combinatorial explosion of unique sequences.

**Heuristic, revised**: in the n=1..3 → n=1..5 transition, output rows roughly **doubled** (rough estimate scaled from prod-default n=1..3 ~= 150M to actual 317M for n=1..5). Earlier "4× cost" estimate was too pessimistic — pruning makes the marginal cost of n=4/5 more like 1.3-1.5×.

### 2.2 `bucket_granularity`

Number of distinct buckets dominates per-bucket count cardinality:

| Granularity | Buckets (HN history) | Output multiplier vs monthly |
|-------------|----------------------|------------------------------|
| daily | ~7,200 | ~30× |
| monthly | ~240 | 1× (this build: 232-233 non-empty) |
| yearly | ~20 | ~0.08× |

Storage scales roughly linearly with bucket count for a given vocabulary. Monthly remains the sweet spot. Yearly throws away most of the temporal signal HN-ngram-viewer needs; daily inflates storage 30× with limited query-time benefit (the API can aggregate up but never drill below stored granularity).

### 2.3 `min_global`

The single biggest size lever. Filters whole ngrams by corpus-wide count.

**Calibrated from §1.2 observations** (HN, monthly, post-flush):

| n | `min_global` | Admitted | Admission rate vs candidates |
|---|--------------|----------|------------------------------|
| 2 | 15 | 2.14M | 32% |
| 3 | 8 | 4.94M | 35% |
| 4 | 5 | 4.86M | 36% |
| 5 | 3 | 10.33M | 100% (pre-filtered) |

Earlier prediction said "doubling threshold roughly halves admitted set" with a Zipf model. The actual data is closer to **flat 32-36% admission rate across n=2..4** at our chosen thresholds — meaning the post-flush candidate population is already mostly above-threshold by structure, and `min_global` is only knocking off the bottom third. Going lower (e.g. `min_global_2gram=5`) would admit much more, but most of that "more" is already in `global_counts` (45.8M total retained across all n's at floor=3) — the question is whether to surface it in `ngram_counts` or just keep it in the global table.

**Practical rule from this calibration**: at the §1.2 thresholds, **vocabulary≈30% of candidates** for n=2..4. To estimate output rows for a different threshold without rerunning, multiply that admission rate by the candidate count and the avg-buckets-per-admitted-ngram from §1.2 (n=2: 431k, n=3: 485k, n=4: 230k).

### 2.4 `min_bucket`

Filters per-bucket counts. Most aggressive for sparse n. A 5-gram that appears once in a month gets killed by `min_bucket=2`. Setting this low (1–2) is what enables niche phrases to surface in obscure months.

Effect on row count is approximately linear in the count distribution's tail. For HN at the §1.2 thresholds, dropping `min_bucket` from 5 → 1 typically increases retained per-bucket rows by 3–5× for trigrams+, much less for unigrams (which mostly already appear ≥5 times anywhere they appear at all). This was not directly measured in this build — only `min_bucket=1 / 2 / 2 / 2 / 1` was used.

### 2.5 `min_flush`

Cuts data *during* the partial-file write phase, before merge. Highest-leverage at runtime: filters singletons that would otherwise inflate disk + merge RAM but never pass `min_global` anyway. Observed: the 10× per-flush reduction (25-33M → 2.5-3.3M).

**Caveat (still worth flagging)**: `min_flush` is per-batch, not per-month. Lowering `max_entries` makes batches smaller, splitting a month's data across batches, weakening `min_flush` as a "true global singleton" filter. Mitigated by RFC-013 (planned) — inline globals during source processing, eliminating Pass 1 in the merge phase.

### 2.6 `max_entries` (the trap)

Caps the in-memory consumer hashmap. Lower = less RAM but more flushes and more aggressive flush filtering of what would be legitimate long-tail ngrams. Upper bound = peak consumer RAM ≈ `max_entries × ~250B` for n=1..3, `~370B` for n=1..5.

| `max_entries` | Peak consumer RAM | Suitable RAM total |
|---------------|-------------------|--------------------|
| 20M | ~5GB | 8GB+ |
| 25M | ~9GB (observed at §1.2) | 16GB+ |
| 80M | ~30GB (caused OOM at 30GB box) | 64GB+ |
| 200M | ~75GB | 128GB+ |

**These are consumer-side only.** Producer-side memory adds 1–4GB depending on `producer_count` and per-file size. The merge phase, post-RFC-010, has its own bounded profile (~12-18GB independent of corpus size).

## 3. Output Size Estimation (Calibrated)

Updated formula:

```
ngram_counts_rows ≈ sum over n of (admitted_ngrams[n] × avg_buckets_per_admitted[n])
```

Where for HN at the §1.2 thresholds:
* `admitted_ngrams[n]`: 1.09M / 2.14M / 4.94M / 4.86M / 10.33M for n=1..5
* `avg_buckets_per_admitted[n]`: 24 / 47 / 23 / 11 / 2.4 (= rows ÷ admitted)
* Total: 25.96M + 100.57M + 112.62M + 53.33M + 24.92M = 317.40M ✓

### Calibration table (updated with actuals)

| Profile | Output rows | Output parquet | B/row | After CH (estimated) |
|---------|-------------|----------------|-------|----------------------|
| Dev (n=1..3, daily, agg pruning) | 20.5M | ~600 MB | ~30 | ~1.5 GB |
| Full (n=1..5, monthly, low pruning) | **317M** | **3.4 GB** (`ngram_counts` only) | **10.7** | **~3-6 GB** (TBD on import) |

**Surprise findings vs my earlier estimates:**

* **Output rows** came in at 317M — higher than my 150-250M estimate. Reason: I underestimated n=2/3 admitted-vocab, both of which contribute 100M+ rows each due to high `avg_buckets_per_admitted` (each common bigram appears in nearly every monthly bucket).
* **Parquet size** came in at 3.4GB for `ngram_counts` (4.0GB total) — *lower* than my 5-8GB estimate. Parquet's column dictionary encoding plus ZSTD is more efficient on these strings than I credited. **B/row is ~10-13** in parquet vs ~250-370 in the in-memory consumer hashmap — a 25-35× compression ratio.
* The earlier claim that "ClickHouse storage is 1.5-2× raw Parquet" is **untested** for this dataset and should not be trusted blindly. Will measure once the prod import lands.

## 4. Recommended Profiles by Box Size

| Box | Profile to run | Notes |
|-----|----------------|-------|
| 8GB RAM | `ingest.test.toml` | Smoke test only, monthly, n=3, aggressive thresholds |
| 16GB RAM | `ingest.dev.toml` (with `--max-entries 15M`) | Full corpus, daily, agg pruning |
| 32GB RAM | `ingest.full.toml` (with `--max-entries 25M`) | Monthly, n=5, low pruning. **Requires RFC-010 streaming merge** (now landed). Verified end-to-end on this box class. |
| 64GB RAM | `ingest.full.toml` (default 200M `max_entries`) | Same profile, ~2× faster source-processing, simpler memory profile |
| 128GB+ RAM | could go daily + n=5 | Output would be ~7,000 / 233 = 30× the row count of monthly = ~10 billion rows. Not recommended; storage cost dominates query benefit |

## 5. Prod Disk Budget

Based on §1.2 actuals:

| Resource | Need |
|----------|------|
| Output Parquet (during transfer + import) | **4.0 GB** (peak; deletable post-import) |
| ClickHouse on-disk after import | **3-6 GB** (estimate; depends on compression vs Parquet) |
| ClickHouse staging during atomic swap | **+5-10 GB** transient |
| **Total peak disk for n-gram data** | **~15-20 GB** |
| **Steady-state disk** | **~5 GB** |

The current Hetzner prod box has ~150GB free. Comfortable.

## 6. Future Work

**RFC-013: inline globals during source processing**. Eliminates merge Pass 1 entirely; recovers true unpruned globals. Strictly faster and better signal quality. ~1 day of work.

**External-sort merge** (RFC-010 §4.5): makes merge RAM-independent of corpus size. Required if corpus grows 10× (e.g., HN posts + comments, or other forums combined).

**Producer-side streaming**: the producer's `process_comments_parallel` builds a per-file rayon-merged hashmap that grows with file size. For the largest months (2024+) this is several GB per producer. A streaming variant that emits to the channel more aggressively would reduce producer RAM, at the cost of more channel traffic.

**String interning**: every `NgramKey` copies its `ngram` String. For high-frequency ngrams that appear in many buckets, this is the biggest source of waste in the consumer hashmap. A `StringInterner` over admitted vocabulary would cut consumer RAM 30–50% — but at the cost of an extra pass to determine the admission set first. Tractable, ~1 day of work.
