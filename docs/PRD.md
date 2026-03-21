Below is a **clean, implementation-oriented PRD** you can actually build from. I’m intentionally making a few opinionated calls (especially on granularity and scope) so you don’t end up with a vague system.

---

# 📄 Product Requirements Document (PRD)

## Project: Hacker News N-gram Viewer

---

## 1. 🎯 Objective

Build a web-based tool that allows users to explore how words and phrases evolve over time in Hacker News comments, using **normalized relative frequency**, similar to the Google Ngram Viewer.

The system should:

* Allow querying 1–3 word phrases (n-grams)
* Display time-series trends
* Normalize counts to account for varying corpus size over time
* Be fast, interactive, and reproducible

---

## 2. 🧠 Core Concept

For any query phrase `g` and time bucket `t`:

[
relative_frequency(g,t) = \frac{count(g,t)}{total_ngrams(n,t)}
]

Where:

* `n` = number of tokens in `g`
* denominator = total number of n-grams of the same order in that time bucket

---

## 3. 👤 Target Users

### Primary

* Developers, founders, and HN readers
* People interested in tech trends over time

### Secondary

* Researchers analyzing discourse trends
* Curious users exploring language shifts (e.g., “AI safety” vs “alignment”)

---

## 4. 🧱 Scope

### Included (v1)

* Unigrams, bigrams, trigrams
* Hacker News comments only (not stories)
* Historical + near-real-time data
* Time-series charting with multiple query terms
* Basic smoothing
* Case-insensitive mode

### Excluded (v1)

* Full-text search of comments
* Named entity recognition
* Semantic grouping / embeddings
* Advanced NLP

---

## 5. ⚙️ System Overview

### Data Flow

```
HuggingFace Parquet (HN dataset)
        ↓
Rust Fetcher
        ↓
Rust Indexer (tokenize + n-grams)
        ↓
Aggregated counts
        ↓
ClickHouse
        ↓
Rust API
        ↓
React UI
```

---

## 6. 🪵 Data Model

### 6.1 Core Tables (ClickHouse)

#### `ngram_counts`

* `bucket` (Date)
* `n` (UInt8)
* `ngram` (String)
* `count` (UInt32)

#### `bucket_totals`

* `bucket` (Date)
* `n` (UInt8)
* `total_count` (UInt64)

#### Optional (future)

* `comment_counts`
* `token_counts`

---

## 7. 🕒 Time Granularity (IMPORTANT)

### Decision: **Base granularity = daily**

#### Why daily:

* HN activity is bursty → monthly hides too much
* Weekly is okay, but less flexible
* Daily allows aggregation upward (week/month/year)
* Storage cost is manageable with aggregation

### Query behavior:

* Always store **daily buckets**
* UI allows:

  * Daily
  * Weekly (derived)
  * Monthly (derived)
  * Yearly (derived)

👉 Aggregation rule:

* Sum counts across buckets
* Sum denominators accordingly

### Explicit constraint:

> No arbitrary bucket sizes (e.g., 9 days). Only predefined multiples:

* 1 day
* 7 days (week)
* calendar month
* calendar year

---

## 8. 🔤 Tokenization Strategy

### Goals:

* deterministic
* reproducible
* fast
* HN-aware (technical language)

### Rules (v1):

* lowercase everything
* strip HTML
* normalize Unicode
* preserve:

  * letters
  * numbers
  * apostrophes
  * `+`, `#`, `.` inside tokens
* split on everything else

### Examples:

| Input           | Output                    |
| --------------- | ------------------------- |
| `C++ is great`  | `["c++", "is", "great"]`  |
| `Node.js rocks` | `["node.js", "rocks"]`    |
| `Don't do that` | `["don't", "do", "that"]` |

### Non-goals:

* sentence detection
* named entity recognition

---

## 9. 🔢 N-gram Strategy

### Supported:

* 1-grams
* 2-grams
* 3-grams

### Generation:

* sliding window over tokens

### Storage constraints:

* keep all unigrams
* keep bigrams/trigrams above frequency threshold (configurable)

---

## 10. 🔄 Data Processing

### 10.1 Batch (historical)

* process monthly partitions in parallel
* emit:

  * per-day counts
  * per-day totals

### 10.2 Incremental (live)

* poll “today” partitions every ~5–15 minutes
* append/update current day bucket

### 10.3 Idempotency

* track processed partitions
* ensure re-runs do not double count

---

## 11. ⚡ Performance Requirements

### Query latency:

* < 200 ms for typical queries (≤ 10 phrases)

### Data volume:

* tens of millions of comments
* millions of unique n-grams

### Throughput:

* batch processing should saturate CPU cores

---

## 12. 🌐 API Design (Rust)

### Endpoint: `/query`

#### Input:

```json
{
  "phrases": ["machine learning", "deep learning"],
  "start": "2010-01-01",
  "end": "2025-01-01",
  "granularity": "month",
  "smoothing": 3
}
```

#### Output:

```json
{
  "series": [
    {
      "phrase": "machine learning",
      "points": [
        {"t": "2010-01", "value": 0.00023},
        ...
      ]
    }
  ]
}
```

---

## 13. 🖥️ UI Requirements

### Core features:

* multi-phrase input (comma-separated)
* interactive line chart
* smoothing slider
* granularity selector
* time range selector

### Nice-to-have:

* shareable URLs
* hover comparison
* log scale toggle

---

## 14. 🧪 Validation & Accuracy

### Must ensure:

* deterministic tokenization
* consistent normalization across time
* no double counting
* correct denominator alignment

### Testing:

* snapshot tests on known phrases
* compare against manual counts for small samples

---

## 15. ⚠️ Risks

### 1. Tokenization quality

* biggest impact on usefulness

### 2. Storage explosion

* too many rare n-grams

### 3. Incremental update bugs

* double counting / gaps

### 4. UX complexity

* charts can get messy quickly

---

## 16. 🚀 Future Extensions

* story titles as separate corpus
* case-sensitive mode
* per-user or per-domain filtering
* “show example comments at this time”
* trending phrases detection
* embeddings / semantic grouping

---

## 17. 🧭 Key Product Decisions (Locked)

* ✅ Daily base granularity
* ✅ Pre-aggregated n-gram index (not raw scan)
* ✅ Deterministic tokenizer (not NLP-based)
* ✅ ClickHouse as serving DB
* ✅ Rust backend
* ✅ React + TS frontend
* ❌ No heavy NLP in v1

---

## 🧠 Final Take on Granularity (your question)

Your instinct to pick a base unit is exactly right.

👉 **Daily is the correct base granularity**

Because:

* you can always aggregate up
* you cannot recover detail if you start at monthly
* storage is still manageable with pruning

And importantly:

> Google Ngram Viewer effectively works this way conceptually (fixed base + smoothing/aggregation on top), even though it presents yearly data.

---

If you want next step, we should do RFCs in this order:

1. Tokenization spec (this is the most critical)
2. N-gram generation + pruning strategy
3. ClickHouse schema + partitioning
4. Rust pipeline architecture
5. Query API contract
6. Frontend interaction model

Tokenization is where most people get this wrong, so I’d start there.

