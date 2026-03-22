# RFC-005 (Agent-Oriented)

## Query API + Type System + OpenAPI/SDK Generation

## Status

**In Progress**
- Stub implementation: `server/crates/api/src/main.rs`
- OpenAPI served at: `/api-doc/openapi.json`
- Swagger UI at: `/swagger-ui`

---

## 0. Scope

Define:

* HTTP API for querying n-gram data
* request/response type system (Rust-first)
* OpenAPI generation (backend)
* TypeScript SDK generation (frontend)
* guarantees for cross-language type safety

---

## 1. Core Design Principle

## Spec (mandatory)

**Rust types are the single source of truth**

* all request/response schemas must be defined in Rust
* OpenAPI spec must be generated from Rust
* TypeScript types must be generated from OpenAPI
* frontend must not define duplicate API types manually

---

## Rationale

* prevents type drift between frontend and backend
* ensures correctness as API evolves
* enables automated SDK generation

---

## Flexibility

* agent may change OpenAPI generator library **only if**:

  * it remains code-first
  * Rust types remain the source of truth

---

## Non-goals

* shared Rust/TS type definitions
* manual TS type duplication

---

# 2. Technology Stack

## Spec (mandatory)

Backend:

* `axum` (HTTP framework)
* `utoipa` (OpenAPI generation)
* `utoipa-axum` (integration)

Frontend:

* `Kubb` (OpenAPI → TS SDK generator)

Rate Limiting:

* `Caddy` reverse proxy (preferred, keeps API code simple)

---

## Rationale

* `utoipa` supports deriving OpenAPI schemas directly from Rust types
* Kubb generates typed clients, hooks, and validators from OpenAPI
* Caddy handles rate limiting at infrastructure layer, avoiding Rust middleware complexity

---

## Flexibility

* `aide` or `poem-openapi` allowed as alternatives **only if**:

  * OpenAPI is still generated from Rust types
  * Kubb remains compatible

---

# 3. API Endpoint

## Spec (mandatory)

### Route

```text
GET /query
```

### Query Parameters

Defined via Rust struct (must derive `IntoParams` + `Deserialize`):

```rust
#[derive(Deserialize, IntoParams)]
struct QueryParams {
    /// Comma-separated phrases to query (max 10)
    phrases: String,
    /// Start date (YYYY-MM-DD). Default: HN_DEFAULT_START_DATE
    start: Option<String>,
    /// End date (YYYY-MM-DD). Default: today
    end: Option<String>,
}
```

Note: `phrases` is a comma-separated string (not array) for clean shareable URLs.

Note: `granularity` was removed from the API (RFC-007-optimizations §3). The API always returns daily data. Granularity aggregation is performed client-side, like smoothing.

---

## Constraints

* `phrases` splits to max 10 items
* each phrase must tokenize to 1–3 tokens
* `start <= end` (if both provided)
* dates must be valid ISO format (YYYY-MM-DD)

---

## Default Values

| Parameter | Default | Source |
|-----------|---------|--------|
| `start` | `2011-01-01` | `hn_clickhouse::HN_DEFAULT_START_DATE` |
| `end` | today | computed at request time |

The default start date (2011) is when HN reached ~1M comments/year, providing meaningful data density.

---

## Rationale

* GET allows caching and shareable URLs
* comma-separated phrases work cleanly in URLs: `?phrases=rust,go,python`
* optional params with sensible defaults reduce boilerplate

---

## Flexibility

* may switch to POST if URL length becomes problematic (unlikely with 10 phrase limit)
* validation rules may be tightened

---

# 4. Response Schema

## Spec (mandatory)

```rust
#[derive(Serialize, ToSchema)]
struct QueryResponse {
    series: Vec<Series>,
    meta: QueryMeta,
}

#[derive(Serialize, ToSchema)]
struct QueryMeta {
    tokenizer_version: String,
    start: String,
    end: String,
    granularity: String,
}

#[derive(Serialize, ToSchema)]
struct Series {
    phrase: String,
    normalized: String,  // tokenized form used for lookup
    status: SeriesStatus,
    points: Vec<Point>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum SeriesStatus {
    Indexed,      // phrase is in vocabulary, data returned
    NotIndexed,   // phrase is valid but not in vocabulary (too rare)
    Invalid,      // phrase failed validation (e.g., > 3 tokens, empty)
}

#[derive(Serialize, ToSchema)]
struct Point {
    t: String,     // bucket timestamp (YYYY-MM-DD)
    v: u32,        // raw occurrence count (RFC-007-optimizations §2)
}
```

---

## Requirements

* output sorted by time ascending
* one series per input phrase (in input order)
* for `Indexed` series: return **sparse** points (only buckets with non-zero counts). Frontend handles zero-fill.
* for `NotIndexed` series: points array is empty
* for `Invalid` series: points array is empty
* `meta` includes actual parameters used (after defaults applied)
* `Point.v` is a raw count (`u32`), NOT a relative frequency — client computes frequency using `/totals` endpoint

---

## Rationale

* stable structure for frontend charting
* sparse responses reduce payload size and server work
* raw counts eliminate the JOIN with `bucket_totals` on every query (RFC-007-optimizations §2)
* frontend zero-fills using `meta.start`, `meta.end`, and `meta.granularity` (cheap client-side with dayjs)
* frontend computes relative frequency using cached `/totals` data
* `normalized` field shows what was actually looked up (aids debugging)
* `meta` enables reproducible queries and debugging

---

## Flexibility

* additional metadata fields may be added
* field names use short forms (`t`, `v`) to reduce payload size

---

# 5. Query Normalization

## Spec (mandatory)

For each input phrase:

1. apply RFC-001 tokenizer
2. count tokens
3. if token count = 0 → status = `Invalid`
4. if token count > 3 → status = `Invalid`
5. join tokens with single space
6. use resulting string as lookup key

---

## Example

```text
Input: "Node.js"
→ tokenize → ["node.js"]
→ join → "node.js"
→ lookup in ClickHouse
```

```text
Input: "this is a very long phrase"
→ tokenize → ["this", "is", "a", "very", "long", "phrase"]
→ count = 6 > 3
→ status = Invalid
```

---

## Rationale

* ensures exact alignment between query and stored ngrams

---

## Constraints

* must use same tokenizer version as ingestion (from `hn_clickhouse::TOKENIZER_VERSION`)

---

# 6. Sparse Response Model

## Spec (mandatory)

For `Indexed` series, backend returns **only buckets with non-zero values** (sparse). The frontend is responsible for zero-filling gaps using `meta.start`, `meta.end`, and `meta.granularity`.

Points must still be sorted by time ascending.

---

## Bucket alignment (for reference — applied by frontend during zero-fill)

| Granularity | Alignment function |
|-------------|-------------------|
| Day | identity |
| Week | start of week (Monday) |
| Month | start of month |
| Year | start of year |

---

## Rationale

* reduces network payload (many n-grams are sparse across time)
* reduces server work (no bucket iteration/fill logic)
* frontend has all the information needed to zero-fill (`meta` fields + dayjs)

---

# 6.1 Totals Endpoint (RFC-007-optimizations §2)

## Spec (mandatory)

### Route

```text
GET /totals
```

### Query Parameters

```rust
#[derive(Deserialize, IntoParams)]
struct TotalsParams {
    /// Start date (YYYY-MM-DD). Default: 2011-01-01
    start: Option<String>,
    /// End date (YYYY-MM-DD). Default: today
    end: Option<String>,
}
```

### Response Schema

```rust
#[derive(Serialize, ToSchema)]
struct TotalsResponse {
    totals: Vec<TotalSeries>,  // one per n-gram order (1, 2, 3)
    meta: TotalsMeta,
}

#[derive(Serialize, ToSchema)]
struct TotalSeries {
    n: u8,                      // n-gram order (1, 2, or 3)
    points: Vec<TotalPoint>,    // sparse daily totals
}

#[derive(Serialize, ToSchema)]
struct TotalPoint {
    t: String,  // YYYY-MM-DD
    v: u64,     // total n-gram count for this day and order
}
```

### Caching

Response includes `Cache-Control: public, max-age=86400` — totals change only on ingestion.

---

## Rationale

* eliminates the JOIN between `ngram_counts` and `bucket_totals` on every `/query` request
* totals are small (~5,400 days × 3 n-orders) and change only on ingestion
* client fetches totals once, caches locally, and computes relative frequency: `count / total`
* main `/query` becomes a pure indexed key lookup in ClickHouse

---

# 7. Granularity Handling (RFC-007-optimizations §3)

## Spec

* base storage: daily
* API always returns **daily** data — no `granularity` parameter
* granularity aggregation (week/month/year) is performed **client-side**
* `granularity` is a frontend-only display parameter (like smoothing), persisted in URL as `g` but NOT sent to API

---

## Rationale

* eliminates server-side GROUP BY
* one cache key per (phrases, date range) regardless of granularity
* switching granularity is instant — no network round-trip
* daily data for 15 years is ~5,400 points per series (~100KB per series, ~200-300KB gzipped for 10 series)

---

# 8. Smoothing

## Spec (mandatory)

* smoothing is **frontend-only**
* API returns raw (unsmoothed) series data
* no `smoothing` parameter in API

---

## Rationale

* keeps API surface minimal
* smoothing slider changes become instant (no round-trip)
* reduces API complexity and cache variations
* raw data is more useful for future features

---

## Frontend Implementation

```typescript
function applySmoothing(points: Point[], window: number): Point[] {
  if (window <= 1) return points;
  return points.map((point, i) => {
    const start = Math.max(0, i - Math.floor(window / 2));
    const end = Math.min(points.length, i + Math.ceil(window / 2));
    const slice = points.slice(start, end);
    const avg = slice.reduce((sum, p) => sum + p.v, 0) / slice.length;
    return { t: point.t, v: avg };
  });
}
```

---

# 9. Error Handling

## Spec (mandatory)

API must return structured errors:

```rust
#[derive(Serialize, ToSchema)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
struct ErrorDetail {
    code: String,
    message: String,
}
```

---

## Error Codes

| Code | HTTP Status | Condition |
|------|-------------|-----------|
| `MISSING_PHRASES` | 400 | `phrases` param empty or missing |
| `TOO_MANY_PHRASES` | 400 | more than 10 phrases |
| `INVALID_PHRASE` | 400 | phrase tokenizes to 0 or >3 tokens |
| `INVALID_DATE_FORMAT` | 400 | date not in YYYY-MM-DD format |
| `INVALID_DATE_RANGE` | 400 | start > end |
| `INVALID_GRANULARITY` | 400 | unknown granularity value |
| `INTERNAL_ERROR` | 500 | ClickHouse or other internal failure |

---

## Example

```json
{
  "error": {
    "code": "TOO_MANY_PHRASES",
    "message": "Maximum 10 phrases allowed, got 15"
  }
}
```

---

# 10. OpenAPI Generation

## Spec (mandatory)

* all request/response types must derive `ToSchema`
* all endpoints must use `#[utoipa::path(...)]`
* OpenAPI spec served at runtime: `/api-doc/openapi.json`
* Swagger UI served at: `/swagger-ui`
* Static spec exported to `server/openapi.json` via: `cargo run -p api --bin generate_openapi`
* `server/openapi.json` is the input for Kubb SDK generation

### Export workflow

```bash
cd server && cargo run -p api --bin generate_openapi > openapi.json
cd client && bun run generate
```

The `generate_openapi` binary imports the `ApiDoc` struct from `api::lib` and serializes it to JSON. This ensures the static file always matches the Rust types exactly.

---

## Rationale

* enables automatic SDK generation
* Swagger UI aids development and debugging
* static export enables offline SDK generation without running the API server

---

## Constraints

* no undocumented endpoints
* no untyped request/response bodies
* `server/openapi.json` must be regenerated after any API type change

---

# 11. TypeScript SDK Generation

## Spec (mandatory)

* frontend must generate SDK using Kubb
* OpenAPI spec is the only input
* generated code goes to `client/src/gen/` (gitignored)

---

## Output must include:

* typed client functions
* request/response types
* React Query hooks (optional)

---

## Constraints

* no manual API types in frontend
* generated code must not be edited manually

---

# 12. HTTP Caching (RFC-007-optimizations §1)

## Spec (mandatory)

N-gram data is effectively immutable between ingestion runs. API responses must include `Cache-Control` headers.

### Response headers

```text
Cache-Control: public, max-age=3600
```

Set directly by the Rust API on `/query` responses (and `/totals` when added).

### Static assets

Caddy sets `Cache-Control: public, max-age=31536000, immutable` on JS/CSS/image/font files.

### Cache invalidation

Cache expires naturally via `max-age`. On ingestion, new data appears after TTL expiry. No explicit purge mechanism in v1.

---

## Rationale

* data changes only on ingestion (daily at most)
* repeated queries for popular phrases are served from intermediary caches
* highest-ROI optimization with near-zero complexity

---

## Flexibility

* `max-age` may be increased for historical date ranges that can never change
* may add `ETag` based on last ingestion timestamp in future

---

# 13. Rate Limiting

## Spec (mandatory)

Rate limiting handled by Caddy reverse proxy (not Rust middleware).

### Caddyfile configuration

```caddyfile
api.example.com {
    rate_limit {
        zone api {
            key {remote_host}
            events 60
            window 1m
        }
    }

    reverse_proxy api:3000
}
```

### Limits

* **per-IP**: 60 requests/minute
* **burst**: handled by zone configuration

---

## Rationale

* keeps API code simple
* Caddy handles this efficiently at edge
* easy to tune without code changes

---

## Flexibility

* limits may be tuned based on observed traffic
* may add per-route limits if needed

---

# 14. Versioning

## Spec

* API uses `tokenizer_version` from `hn_clickhouse::TOKENIZER_VERSION`
* version included in response `meta` for transparency
* frontend does not supply version

---

## Rationale

* ensures query consistency with stored data
* meta field aids debugging version mismatches

---

# 15. Constants

## Spec (mandatory)

Shared constants must be defined in `hn-clickhouse` crate:

```rust
/// Default start date for queries (when HN had meaningful volume)
pub const HN_DEFAULT_START_DATE: (i32, u8, u8) = (2011, 1, 1);

/// Maximum phrases per query
pub const MAX_PHRASES: usize = 10;

/// Maximum n-gram order
pub const MAX_NGRAM_ORDER: u8 = 3;
```

API crate imports these constants - no hardcoded values.

---

## Rationale

* single source of truth
* easy to adjust limits
* prevents drift between validation and documentation

---

# 16. Prohibited Designs

Agent must NOT:

* define API types separately in TS
* bypass OpenAPI generation
* use stringly-typed JSON responses
* allow queries without normalization
* expose raw DB schema directly
* hardcode limits/defaults in multiple places
* add smoothing to API

---

# 17. Acceptance Criteria

System is valid if:

* frontend SDK compiles without manual edits
* all API types originate from Rust
* queries return correct normalized results
* sparse responses contain only non-zero buckets
* no type mismatch between backend and frontend
* OpenAPI spec fully describes API
* error responses follow defined schema

---

## Implementation Checklist

- [ ] `hn-clickhouse`: Add `HN_DEFAULT_START_DATE`, `MAX_PHRASES`, `MAX_NGRAM_ORDER` constants
- [ ] `api`: Import constants from `hn-clickhouse`
- [ ] `api`: Implement proper error response types
- [ ] `api`: Return sparse points for indexed series (frontend handles zero-fill)
- [ ] `api`: Add `meta` to response
- [ ] `api`: Add `normalized` field to series
- [ ] `api`: Validate all constraints (phrase count, date range, etc.)
- [ ] `api`: Remove smoothing parameter
- [ ] Export OpenAPI spec to `server/openapi.json`
- [ ] Caddy config with rate limiting

---

## Final Note for Agent

If proposing changes:

* must preserve "Rust → OpenAPI → TS" pipeline
* must not introduce duplicated type definitions
* must maintain strict alignment with tokenizer and n-gram rules
* must use constants from `hn-clickhouse`, not hardcoded values
