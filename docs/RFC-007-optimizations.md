# RFC-007: Server-Side Work Reduction and Caching Optimizations

## Status

**Proposed**

## Checklist

- [x] 1. HTTP response caching (Caddy layer)
- [ ] 2. ~~Separate `/totals` endpoint with aggressive caching~~ — Superseded by per-phrase caching strategy
- [ ] 3. ~~Client-side granularity aggregation (always return daily data)~~ — Superseded by per-phrase caching strategy
- [ ] 4. ~~Client-side relative frequency computation~~ — Superseded by per-phrase caching strategy
- [ ] 5. Pre-computed monthly materialized view (alternative to #3)
- [ ] 6. Client-side vocabulary status inference
- [x] 7. Per-phrase caching strategy (replaces items 2-4)

---

## 0. Scope

Define optimizations that reduce server-side work per query by:

* leveraging HTTP caching at the reverse proxy layer
* per-phrase API design for independent caching of each phrase × granularity × date range
* maximizing cache hit rates across users and searches

Assumes:

* RFC-003 (ClickHouse schema/query model)
* RFC-005 (API + response format)
* RFC-006 (frontend architecture, already handles zero-fill and smoothing client-side)
* RFC-007 (infrastructure, Caddy reverse proxy)

---

## 0.1 Design Principle

The primary optimization strategy is **per-phrase caching**: the API accepts a single phrase per request, enabling independent HTTP caching per phrase × granularity × date range. The client makes parallel requests and handles zero-fill and smoothing.

---

## 0.2 Priority

| # | Change | Server savings | Client cost | Complexity | Status |
|---|--------|---------------|-------------|------------|--------|
| 1 | HTTP caching | High (eliminates repeated queries) | None | Low | Active |
| 2 | ~~Separate totals endpoint~~ | — | — | — | Superseded |
| 3 | ~~Client-side granularity~~ | — | — | — | Superseded |
| 4 | ~~Client-side frequency math~~ | — | — | — | Superseded |
| 5 | Materialized monthly view | Medium (if monthly queries are hot) | None | Low | Deferred |
| 6 | Client vocabulary inference | Low | Infer from response | Low | Deferred |
| 7 | Per-phrase caching | High (independent cache per phrase) | Parallel requests | Low | Active |

Items 2-4 were superseded by item 7 (per-phrase caching strategy). Item 5 is independent. Item 6 is independent.

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

Enable response caching for `/query` endpoint. Repeated queries for the same phrase × granularity × date range are served from cache with zero ClickHouse work. Per-phrase API design maximizes cache hit rates — popular phrases are cached independently and reused across different user searches.

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

## Status: Superseded

Superseded — per-phrase caching provides better cache hit rates than separating totals. With the per-phrase API design (one request per phrase), the JOIN with `bucket_totals` is trivial (single phrase lookup) and does not warrant a separate endpoint. See §7 for the replacement strategy.

---

# 3. Client-Side Granularity Aggregation

## Status: Superseded

Superseded — granularity stays server-side to reduce payload size (30x for monthly vs daily). Per-phrase caching compensates for the additional cache keys. With the per-phrase API design, each phrase × granularity × date range is independently cached, so the cache key multiplication from granularity is offset by the cache key reduction from single-phrase requests. See §7 for the replacement strategy.

---

# 4. Client-Side Relative Frequency Computation

## Status: Superseded

Superseded — server computes frequency. Per-phrase API design means the JOIN is trivial (single phrase lookup). The server JOINs `ngram_counts` with `bucket_totals` and returns relative frequency (`f64`) directly. See §7 for the replacement strategy.

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

# 7. Per-Phrase Caching Strategy (Replaces Items 2-4)

## Spec (mandatory — current API design)

The API accepts a **single phrase** per request. Clients make parallel requests (one per phrase, up to 10). This is the primary caching optimization, replacing items 2-4.

```text
GET /query?phrase=rust&start=2015-01-01&end=2025-01-01&granularity=month
  → sparse relative frequencies for one phrase
  → server JOINs ngram_counts with bucket_totals, applies GROUP BY for granularity
  → independently cached per phrase × granularity × date range
```

**Server does:** key lookup + JOIN with bucket_totals in ClickHouse, GROUP BY for granularity, serialize response, set cache headers.

**Client does:** parallel requests (one per phrase), zero-fill, smoothing, ECharts formatting.

### Why this replaces items 2-4

| Original optimization | Why superseded |
|----------------------|----------------|
| Separate `/totals` endpoint (#2) | Single-phrase JOIN is trivial; no need to avoid it |
| Client-side granularity (#3) | Server aggregation reduces payload 30x (monthly vs daily); per-phrase caching compensates for extra cache keys |
| Client-side frequency (#4) | Server computes frequency; JOIN cost is negligible for single phrase |

### Cache hit rate analysis

With multi-phrase API (`?phrases=rust,go,python`):
* cache key = `(rust,go,python, month, 2015-2025)` — very specific, low reuse
* adding `java` to the search = entirely new cache key, no reuse

With per-phrase API (`?phrase=rust`):
* cache key = `(rust, month, 2015-2025)` — high reuse across different searches
* adding `java` = one new request, three cached hits
* popular phrases like `rust`, `python`, `ai` are cached across all users

### What the final API looks like

| Concern | Design |
|---------|--------|
| Phrases per request | Single phrase |
| Relative frequency | Server computes via JOIN |
| Granularity aggregation | Server GROUP BY |
| `granularity` API param | Sent to server (day/week/month/year, default month) |
| Response `Point.v` | `f64` (relative frequency) |
| `/totals` endpoint | Removed (not needed) |
| Rate limit | 120 req/min (accounts for 10 parallel requests per search) |

### What does NOT change

* tokenization remains server-side (security + consistency)
* phrase validation and normalization remain server-side
* sparse response model remains (client zero-fills)
* smoothing remains client-side
* URL state model remains (`g` param is sent to API, `s` param is frontend-only)

---

## Acceptance Criteria

Optimizations are valid if:

* query correctness is preserved (same final chart output)
* perceived latency for common queries decreases (parallel requests + cache hits)
* cache hit rate is higher than multi-phrase API design
* client-side transforms remain imperceptible (<50ms for typical data sizes)
* rate limit accommodates parallel per-phrase requests
