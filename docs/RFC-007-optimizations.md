# RFC-007: Server-Side Work Reduction and Caching Optimizations

## Status

**Proposed**

## Checklist

- [x] 1. HTTP response caching (Caddy layer)
- [x] 2. Separate `/totals` endpoint with aggressive caching
- [x] 3. Client-side granularity aggregation (always return daily data)
- [x] 4. Client-side relative frequency computation
- [ ] 5. Pre-computed monthly materialized view (alternative to #3)
- [ ] 6. Client-side vocabulary status inference

---

## 0. Scope

Define optimizations that reduce server-side work per query by:

* leveraging HTTP caching at the reverse proxy layer
* splitting infrequently-changing data into separately cacheable endpoints
* moving cheap arithmetic (frequency computation, granularity aggregation) to the client
* eliminating JOINs and GROUP BYs from the hot query path

Assumes:

* RFC-003 (ClickHouse schema/query model)
* RFC-005 (API + response format)
* RFC-006 (frontend architecture, already handles zero-fill and smoothing client-side)
* RFC-007 (infrastructure, Caddy reverse proxy)

---

## 0.1 Design Principle

The client already performs zero-fill and smoothing (RFC-005 §8, RFC-006 §12). The optimizations here extend that philosophy: the API becomes a thin lookup layer over ClickHouse, and the client handles all data shaping.

---

## 0.2 Priority

| # | Change | Server savings | Client cost | Complexity |
|---|--------|---------------|-------------|------------|
| 1 | HTTP caching | High (eliminates repeated queries) | None | Low |
| 2 | Separate totals endpoint | Medium (removes JOIN) | Trivial division | Low |
| 3 | Client-side granularity | Medium (removes GROUP BY, better caching) | Simple aggregation | Low |
| 4 | Client-side frequency math | Medium (pairs with #2) | Trivial division | Low |
| 5 | Materialized monthly view | Medium (if keeping server aggregation) | None | Low |
| 6 | Client vocabulary inference | Low | Infer from response | Low |

Items 1-4 are recommended together. Item 5 is an alternative to #3 (pick one). Item 6 is independent.

---

# 1. HTTP Response Caching

## Spec (mandatory)

The underlying n-gram data is effectively immutable between ingestion runs. Caddy must cache API responses at the reverse proxy layer.

### Required headers (set by API or Caddy)

```text
Cache-Control: public, max-age=3600
ETag: "<last-ingestion-timestamp>"
```

### Caddy cache configuration

Enable response caching for `/query` and `/totals` endpoints. Repeated queries for the same parameters are served from cache with zero ClickHouse work.

### Cache invalidation

On ingestion completion:

* update the `ETag` / `Last-Modified` value (e.g. store last ingestion timestamp)
* Caddy cache expires naturally via `max-age`, or can be purged explicitly

---

## Rationale

* most queries will be for popular phrases that other users have already searched
* data changes only on ingestion (daily at most, likely less frequent)
* highest ROI optimization with near-zero implementation complexity

---

## Flexibility

* `max-age` may be tuned (longer for historical ranges that can never change)
* may add `Vary` header if future API changes introduce user-specific behavior

---

# 2. Separate Totals Endpoint

## Spec

### Current design (RFC-003 §7)

Every `/query` request JOINs `ngram_counts` with `bucket_totals`:

```sql
SELECT c.bucket, c.ngram, c.count / t.total_count AS rel_freq
FROM ngram_counts c
JOIN bucket_totals t ON ...
```

### Proposed design

Split into two endpoints:

```text
GET /query?phrases=rust,go&start=2015-01-01&end=2025-01-01
  Returns: raw daily counts (sparse), status per phrase
  No JOIN, no frequency computation

GET /totals?start=2015-01-01&end=2025-01-01
  Returns: daily total_count for n=1,2,3
  Heavily cached (changes only on ingestion)
```

### `/totals` response schema

```rust
#[derive(Serialize, ToSchema)]
struct TotalsResponse {
    /// Keyed by n (1, 2, 3), each containing sparse daily totals
    totals: HashMap<u8, Vec<TotalPoint>>,
    meta: TotalsMeta,
}

#[derive(Serialize, ToSchema)]
struct TotalPoint {
    t: String,      // YYYY-MM-DD
    v: u64,         // total_count for that day
}

#[derive(Serialize, ToSchema)]
struct TotalsMeta {
    tokenizer_version: String,
    start: String,
    end: String,
}
```

### `/query` response change

The `Point.v` field becomes a raw count (`u32`) instead of a relative frequency (`f64`):

```rust
struct Point {
    t: String,     // bucket timestamp (YYYY-MM-DD)
    v: u32,        // raw occurrence count (not relative frequency)
}
```

### Client computation

```typescript
const relativeFrequency = count / totalForThatDayAndN;
```

---

## Rationale

* eliminates a JOIN on every query
* totals are small (~5,400 days × 3 n-orders = ~16K rows) and change only on ingestion
* totals can be cached for hours or days with `Cache-Control: public, max-age=86400`
* main query becomes a pure indexed key lookup in ClickHouse

---

## Flexibility

* totals endpoint may return data for all n-orders, or accept an `n` filter param
* client may fetch totals once on page load and reuse across queries

---

## Constraints

* client must fetch totals for the correct date range before computing frequencies
* if totals are missing for a bucket (should not happen), client treats frequency as 0

---

# 3. Client-Side Granularity Aggregation

## Spec

### Current design (RFC-003 §8, RFC-005 §7)

Server applies ClickHouse aggregation functions for non-daily granularity:

```sql
SELECT toStartOfMonth(c.bucket) AS bucket, c.ngram,
       sum(c.count) / sum(t.total_count) AS rel_freq
FROM ...
GROUP BY bucket, c.ngram
```

### Proposed design

API always returns daily data. The `granularity` parameter is removed from the API and becomes frontend-only (like smoothing).

Client aggregates daily data to the requested granularity:

```typescript
function aggregateToGranularity(
  dailyCounts: Point[],
  dailyTotals: TotalPoint[],
  granularity: 'day' | 'week' | 'month' | 'year'
): Point[] {
  if (granularity === 'day') {
    return dailyCounts.map(c => ({
      t: c.t,
      v: c.v / getTotalForDay(dailyTotals, c.t)
    }));
  }
  // Group daily counts/totals by period, sum each, divide
  const periods = groupByPeriod(dailyCounts, granularity);
  const totalPeriods = groupByPeriod(dailyTotals, granularity);
  return periods.map(p => ({
    t: p.periodStart,
    v: p.sumCounts / totalPeriods.get(p.periodStart)
  }));
}
```

### URL state change

The `g` URL param remains (for shareability) but is no longer sent to the API. It joins `s` (smoothing) as a frontend-only display parameter.

---

## Rationale

* eliminates server-side GROUP BY
* one cache key per (phrases, date range) regardless of granularity
* switching granularity is instant with no network round-trip (same as smoothing slider)
* daily data for 15 years is ~5,400 points per series; at ~20 bytes each = ~100KB per series, ~200-300KB gzipped for 10 series — acceptable

---

## Tradeoffs

* slightly larger payloads vs. the current design where monthly data is ~180 points per series
* negligible client-side compute cost for summing arrays

---

## Flexibility

* if payload size becomes a concern, may add optional server-side pre-aggregation for year granularity only
* may compress further with binary response format in future

---

# 4. Client-Side Relative Frequency Computation

## Spec

This is the direct consequence of #2 + #3 combined.

### Transform pipeline (updated from RFC-006 §12)

```text
Current:  API response (sparse frequencies) → zero-fill → smooth → chart
Proposed: API response (sparse raw counts)
            + cached totals
            → zero-fill counts
            → aggregate to granularity (sum counts, sum totals per period)
            → compute relative frequency (count / total)
            → smooth
            → chart format
```

### Client module

```typescript
// features/chart/transform.ts

function transformSeries(
  rawCounts: Point[],          // sparse daily counts from /query
  dailyTotals: TotalPoint[],   // from /totals (cached)
  start: string,
  end: string,
  granularity: Granularity,
  smoothingWindow: number
): ChartPoint[] {
  const filled = zeroFillCounts(rawCounts, start, end);           // daily
  const aggregated = aggregateToGranularity(filled, dailyTotals, granularity);
  const frequencies = computeRelativeFrequency(aggregated);
  return applySmoothing(frequencies, smoothingWindow);
}
```

### Memoization

Each step should be independently memoizable:

* `zeroFillCounts` — recomputes only when raw data or date range changes
* `aggregateToGranularity` — recomputes only when granularity changes
* `computeRelativeFrequency` — recomputes only when aggregation changes
* `applySmoothing` — recomputes only when smoothing window changes

Changing granularity or smoothing reuses cached API data — no network request.

---

## Rationale

* all transforms are simple array arithmetic, trivial in JS
* matches the existing design philosophy: API is a "dumb pipe", client does presentation logic
* granularity and smoothing changes become equally instant

---

# 5. Pre-Computed Monthly Materialized View (Alternative to #3)

## Spec

**Use this only if #3 (client-side granularity) is rejected.**

Create a ClickHouse materialized view for monthly aggregation:

```sql
CREATE MATERIALIZED VIEW ngram_counts_monthly
ENGINE = SummingMergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tokenizer_version, n, ngram, bucket)
AS SELECT
    tokenizer_version,
    n,
    ngram,
    toStartOfMonth(bucket) AS bucket,
    sum(count) AS count
FROM ngram_counts
GROUP BY tokenizer_version, n, ngram, bucket;
```

Similarly for `bucket_totals_monthly`.

### Query routing

API routes `granularity=month` queries to the materialized view instead of applying GROUP BY on daily data.

---

## Rationale

* monthly is the default and most common granularity
* converts a GROUP BY scan into a direct indexed lookup
* ClickHouse maintains the view automatically on insert

---

## Tradeoffs

* additional storage (though modest — monthly has ~12x fewer rows than daily)
* only helps one granularity; week and year still need GROUP BY
* mutually exclusive with #3 — if client does aggregation, this is unnecessary

---

# 6. Client-Side Vocabulary Status Inference

## Spec

### Current design (RFC-005 §4)

Server checks `ngram_vocabulary` table to determine `indexed` / `not_indexed` status for each phrase.

### Proposed simplification

For bigrams and trigrams, the server can infer status directly from the main query result:

* if `ngram_counts` returns rows for the phrase → `indexed`
* if `ngram_counts` returns zero rows AND the phrase is a valid 2/3-gram → check vocabulary table

For unigrams (which have no pruning), the absence of data means the phrase was never seen — this is equivalent to `indexed` with zero occurrences.

### Alternative: bloom filter

Generate a compact bloom filter of the admitted vocabulary at ingestion time. Serve as a static file (`/vocabulary.bloom`). Client checks locally before displaying "not indexed" warnings — no round-trip needed.

* bloom filter for ~1M admitted n-grams ≈ 1-2MB at 1% false positive rate
* false positive = client thinks it's indexed but it isn't (harmless — query returns empty)
* updated only on ingestion

---

## Rationale

* vocabulary changes only on ingestion, making it highly cacheable
* bloom filter approach eliminates the vocabulary DB query entirely
* for most queries (popular phrases), vocabulary check is unnecessary overhead

---

## Flexibility

* bloom filter is optional — may start with server-side check and add bloom filter if vocabulary lookups become a bottleneck
* may skip bloom filter entirely if HTTP caching (#1) makes the question moot

---

# 7. Combined "Dumb Pipe" API Design

## Spec (recommended target state after items 1-4)

If items 1-4 are all implemented, the API simplifies to:

```text
GET /counts?phrases=rust,go&start=2015-01-01&end=2025-01-01
  → sparse daily raw counts + status per phrase
  → pure indexed ClickHouse lookup, no JOIN, no GROUP BY

GET /totals?start=2015-01-01&end=2025-01-01
  → daily totals for n=1,2,3
  → heavily cached, changes only on ingestion
```

**Server does:** key lookup in ClickHouse, serialize response, set cache headers.

**Client does:** zero-fill, frequency computation, granularity aggregation, smoothing.

### What changes from RFC-005

| Concern | RFC-005 (current) | After optimization |
|---------|-------------------|-------------------|
| Relative frequency | Server computes via JOIN | Client computes from raw counts + totals |
| Granularity aggregation | Server GROUP BY | Client aggregation |
| `granularity` API param | Sent to server | Frontend-only (like `smoothing`) |
| Response `Point.v` | `f64` (frequency) | `u32` (raw count) |
| Totals | Embedded in JOIN | Separate cached endpoint |

### What does NOT change

* tokenization remains server-side (security + consistency)
* phrase validation and normalization remain server-side
* sparse response model remains (client zero-fills)
* smoothing remains client-side
* URL state model remains (just `g` param stops being sent to API)

---

## Acceptance Criteria

Optimizations are valid if:

* query correctness is preserved (same final chart output)
* perceived latency for common queries decreases
* ClickHouse load per query decreases
* cache hit rate is measurable and meaningful
* client-side transforms remain imperceptible (<50ms for typical data sizes)
* no regression in response payload size beyond 3x for daily vs. monthly data
