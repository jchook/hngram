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
    /// Single phrase to query
    phrase: String,
    /// Start date (YYYY-MM-DD). Default: HN_DEFAULT_START_DATE
    start: Option<String>,
    /// End date (YYYY-MM-DD). Default: today
    end: Option<String>,
    /// Time bucket granularity. Default: month
    granularity: Option<Granularity>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum Granularity {
    Day,
    Week,
    Month,
    Year,
}
```

Note: The API accepts a **single phrase** per request. Clients make one request per phrase in parallel (up to `MAX_PHRASES` concurrent requests). This enables independent per-phrase caching — adding or removing a phrase does not invalidate cached results for other phrases.

---

## Constraints

* `phrase` must tokenize to 1–3 tokens
* `start <= end` (if both provided)
* dates must be valid ISO format (YYYY-MM-DD)

---

## Default Values

| Parameter | Default | Source |
|-----------|---------|--------|
| `start` | `2011-01-01` | `hn_clickhouse::HN_DEFAULT_START_DATE` |
| `end` | today | computed at request time |
| `granularity` | `month` | hardcoded default |

The default start date (2011) is when HN reached ~1M comments/year, providing meaningful data density.

---

## Rationale

* GET allows caching and shareable URLs
* single phrase per request enables independent caching per phrase × granularity × date range
* optional params with sensible defaults reduce boilerplate

---

## Flexibility

* validation rules may be tightened

---

# 4. Response Schema

## Spec (mandatory)

```rust
#[derive(Serialize, ToSchema)]
struct QueryResponse {
    phrase: String,
    normalized: String,  // tokenized form used for lookup
    status: SeriesStatus,
    points: Vec<Point>,
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
#[serde(rename_all = "snake_case")]
enum SeriesStatus {
    Indexed,      // phrase is in vocabulary, data returned
    NotIndexed,   // phrase is valid but not in vocabulary (too rare)
    Invalid,      // phrase failed validation (e.g., > 3 tokens, empty)
}

#[derive(Serialize, ToSchema)]
struct Point {
    t: String,     // bucket timestamp (YYYY-MM-DD)
    v: f64,        // relative frequency (count / total for that bucket)
}
```

---

## Requirements

* output sorted by time ascending
* one phrase per request, one response per request (flat object, not wrapped in array)
* for `Indexed` status: return **sparse** points (only buckets with non-zero counts). Frontend handles zero-fill.
* for `NotIndexed` status: points array is empty
* for `Invalid` status: points array is empty
* `meta` includes actual parameters used (after defaults applied)
* `Point.v` is a relative frequency (`f64`) — server computes via JOIN with `bucket_totals`

---

## Rationale

* stable structure for frontend charting
* sparse responses reduce payload size and server work
* server computes relative frequency via JOIN — trivial for single-phrase lookups
* frontend zero-fills using `meta.start`, `meta.end`, and `meta.granularity` (cheap client-side with dayjs)
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

# 7. Granularity Handling

## Spec (mandatory)

* base storage: daily
* API accepts `granularity` parameter (`day`, `week`, `month`, `year`; default `month`)
* server performs aggregation via ClickHouse SQL

### SQL mapping

| Granularity | SQL |
|-------------|-----|
| Day | raw daily data, no aggregation |
| Week | `toStartOfWeek(bucket, 1) AS bucket ... GROUP BY bucket, ngram` |
| Month | `toStartOfMonth(bucket) AS bucket ... GROUP BY bucket, ngram` |
| Year | `toStartOfYear(bucket) AS bucket ... GROUP BY bucket, ngram` |

Server JOINs with `bucket_totals` (aggregated at the same granularity) and returns relative frequency.

---

## Rationale

* server-side aggregation reduces payload size significantly (30x for monthly vs daily)
* per-phrase caching compensates for the additional cache keys introduced by granularity
* each phrase × granularity × date range is independently cached

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
| `MISSING_PHRASE` | 400 | `phrase` param empty or missing |
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

Set directly by the Rust API on `/query` responses.

### Static assets

Caddy sets `Cache-Control: public, max-age=31536000, immutable` on JS/CSS/image/font files.

### Cache invalidation

Cache expires naturally via `max-age`. On ingestion, new data appears after TTL expiry. No explicit purge mechanism in v1.

---

## Rationale

* data changes only on ingestion (daily at most)
* repeated queries for popular phrases are served from intermediary caches
* per-phrase API design maximizes cache hit rates — each phrase × granularity × date range is independently cached, so adding a new phrase to a search reuses cached results for existing phrases
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
            events 120
            window 1m
        }
    }

    reverse_proxy api:3000
}
```

### Limits

* **per-IP**: 120 requests/minute
* **burst**: handled by zone configuration

Note: The higher limit (120 vs 60) accounts for the per-phrase API design where each search fires up to 10 parallel requests (one per phrase).

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
