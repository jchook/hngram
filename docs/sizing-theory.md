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

### 1.2 Full profile (in-flight, partial)

```toml
max_n              = 5
bucket_granularity = monthly
min_global         = 0 / 15 / 8 / 5 / 3
min_bucket         = 1 / 2  / 2 / 2 / 1
min_flush          = 1 / 2  / 2 / 2 / 2
max_entries        = 25_000_000   (override; toml says 200M)
```

| Metric | Value |
|--------|-------|
| Source corpus | 12GB raw (same) |
| Partial files on disk | **24GB** in 1912 shard files |
| Per-flush entries | 25–33M pre-prune, 2.5–3.3M post-prune (~10× reduction) |
| Total partials | 239 |
| Estimated post-merge unique entries | 280–400M |
| Peak RAM in merge phase | **70+GB** (caused OOM at 30GB+32GB swap) |
| Output `ngram_counts` | not yet written |

## 2. Variables and Their Effects

### 2.1 `max_n`

Each n is a roughly independent bag of data. Total grows roughly as the sum across n, weighted by Zipf-like distribution:

| n | Unique ngrams (HN, no pruning) | After global threshold |
|---|--------------------------------|------------------------|
| 1 | ~2M unigrams | always admitted |
| 2 | ~50–100M bigrams | ~5–15M admitted (≥15) |
| 3 | ~500M+ trigrams | ~5–15M admitted (≥8) |
| 4 | ~1B 4-grams | ~3–8M admitted (≥5) |
| 5 | ~1.5B 5-grams | ~2–5M admitted (≥3) |

**Heuristic**: each additional n roughly **doubles** total work (CPU + disk + RAM peak) when pruning thresholds are kept low. Going from n=3 → n=5 ≈ 4× cost.

### 2.2 `bucket_granularity`

Number of distinct buckets dominates the per-bucket count cardinality:

| Granularity | Buckets (HN history) | Output multiplier vs monthly |
|-------------|----------------------|------------------------------|
| daily | ~7,200 | ~30× |
| monthly | ~240 | 1× |
| yearly | ~20 | ~0.08× |

Storage scales roughly linearly with bucket count for a given vocabulary. Monthly is the sweet spot: yearly throws away most of the temporal signal HN ngram viewer needs, daily inflates storage 30× with limited query-time benefit (the API can't drill below the stored granularity, but it always *can* aggregate up).

### 2.3 `min_global`

The single biggest size lever. Filters whole ngrams by corpus-wide count.

For Zipfian text corpora, vocabulary admitted ≈ `K / threshold^α` for some α near 1.0. Concretely:

| `min_global_2gram` | Admitted bigrams |
|--------------------|------------------|
| 500 (dev) | ~250K |
| 100 | ~1M |
| 20 (prod) | ~5M |
| 15 (full) | ~7M |
| 5 | ~25M |

Doubling threshold roughly halves the admitted set. That's a *log-linear* relationship between RAM and "long-tail capture."

### 2.4 `min_bucket`

Filters per-bucket counts. Most aggressive for sparse n (4/5-grams). A 5-gram that appears once in a month gets killed by `min_bucket=2`. Setting this low (1–2) is what enables phrases like *"Pete Hegseth defense secretary"* to appear in obscure months.

Effect on row count is roughly linear: dropping `min_bucket` from 5 → 1 typically increases retained per-bucket rows by 3–5× for trigrams+, much less for unigrams (which mostly already appear ≥5 times anywhere they appear at all).

### 2.5 `min_flush`

Cuts data *during* the partial-file write phase, before merge. Highest-leverage at runtime: filters singletons that would otherwise inflate disk + merge RAM but never pass `min_global` anyway.

**Caveat**: `min_flush` is per-batch, not per-month. Lowering `max_entries` makes batches smaller, splitting a month's data across batches, weakening `min_flush` as a "true global singleton" filter. This is why `ingest.full.toml` recommends `max_entries=200M` — large enough that each batch is ≈1 month, making `min_flush=2` behave as "must appear ≥2 times per month."

### 2.6 `max_entries` (the trap)

Caps the in-memory consumer hashmap. Lower = less RAM but more flushes and more aggressive flush filtering of what would be legitimate long-tail ngrams. Upper bound = peak consumer RAM ≈ `max_entries × ~250B` for n=1..3, `~370B` for n=1..5.

| `max_entries` | Peak consumer RAM | Suitable RAM total |
|---------------|-------------------|--------------------|
| 20M | ~5GB | 8GB+ |
| 25M | ~9GB | 16GB+ |
| 80M | ~30GB | 64GB+ |
| 200M | ~75GB | 128GB+ |

**These are consumer-side only.** Producer-side memory adds 1–4GB depending on `producer_count` and per-file size. The merge phase has its own (worse) memory profile — see RFC-010.

## 3. Output Size Estimation

Rough formula for `ngram_counts` row count given a profile:

```
rows ≈ sum over n of (admitted_ngrams[n] × avg_buckets_per_ngram[n])
```

Where:
* `admitted_ngrams[n]` is ~K[n] / min_global[n]^α, K[n] from §2.1
* `avg_buckets_per_ngram[n]` for HN ranges from "almost all buckets" (common unigrams) to ~3–10 (rare ngrams)

**Calibration from observed builds:**

| Setting | Output rows | Output parquet | After CH compression |
|---------|-------------|----------------|----------------------|
| Dev (n=1..3, daily, agg pruning) | 20.5M | ~600MB | ~1.5GB |
| Prod-default (n=1..3, daily, low pruning) | ~80–120M (est.) | ~3GB | ~6GB |
| Full (n=1..5, monthly, low pruning) | ~150–250M (est.) | ~5–8GB | ~3–6GB |

ClickHouse compression typically *increases* on-disk size relative to raw Parquet for this kind of low-cardinality categorical data — the column store with LowCardinality + ZSTD is roughly 1.5–2× the raw Parquet. Counterintuitive but consistent.

## 4. Recommended Profiles by Box Size

| Box | Profile to run | Notes |
|-----|----------------|-------|
| 8GB RAM | `ingest.test.toml` | Smoke test only, monthly, n=3, aggressive thresholds |
| 16GB RAM | `ingest.dev.toml` (with --max-entries 15M) | Full corpus, daily, agg pruning |
| 32GB RAM | `ingest.full.toml` (with --max-entries 25M) | Monthly, n=5, low pruning. **Requires RFC-010 streaming merge.** |
| 64GB RAM | `ingest.full.toml` (default 200M max_entries) | Same profile, no streaming-merge requirement, ~2× faster |
| 128GB+ RAM | could go daily + n=5 | Probably overkill; storage cost dominates query benefit |

## 5. Future Work

**External-sort merge** (RFC-010 §4.4): would make the merge phase RAM-independent of corpus size. Required if we ever expand to a 10× corpus (e.g., HN posts + comments combined, or other forums).

**Producer-side streaming**: the producer's `process_comments_parallel` builds a per-file rayon-merged hashmap that grows with file size. For the largest months (2024+) this is several GB per producer. A streaming variant that emits to the channel more aggressively would reduce producer RAM, at the cost of more channel traffic.

**String interning**: every NgramKey copies its `ngram` String. For high-frequency ngrams that appear in many buckets, this is the biggest source of waste in the consumer hashmap. A `StringInterner` over admitted vocabulary would cut consumer RAM 30–50% — but at the cost of an extra pass to determine the admission set first. Tractable, ~1 day of work.
