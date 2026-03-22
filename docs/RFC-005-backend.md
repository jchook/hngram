# RFC-005 (Agent-Oriented)

## Query API + Type System + OpenAPI/SDK Generation

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

---

## Rationale

* `utoipa` supports deriving OpenAPI schemas directly from Rust types
* Kubb generates typed clients, hooks, and validators from OpenAPI
* combination enables near “define once” workflow

---

## Flexibility

* `aide` or `poem-openapi` allowed as alternatives **only if**:

  * OpenAPI is still generated from Rust types
  * Kubb remains compatible

---

# 3. API Endpoint

## Spec (mandatory)

### Route

```text id="u8b9t2"
GET /query
```

---

### Query Parameters

Defined via Rust struct (must derive `IntoParams` + `Deserialize`):

```rust
struct QueryRequest {
    phrases: Vec<String>,
    start: String,        // ISO date (YYYY-MM-DD)
    end: String,          // ISO date (YYYY-MM-DD)
    granularity: Granularity,
    smoothing: Option<u32>,
}
```

---

### Enum

```rust
enum Granularity {
    Day,
    Week,
    Month,
    Year,
}
```

---

## Constraints

* `phrases.len() <= 10`
* each phrase must tokenize to 1–3 tokens
* `start <= end`
* max date range may be limited (implementation-defined)

---

## Rationale

* GET allows caching and shareable URLs
* simple query model matches UI needs

---

## Flexibility

* may switch to POST if URL length becomes problematic
* validation rules may be tightened

---

# 4. Response Schema

## Spec (mandatory)

```rust
struct QueryResponse {
    series: Vec<Series>,
}

struct Series {
    phrase: String,
    status: SeriesStatus,
    points: Vec<Point>,
}

enum SeriesStatus {
    Indexed,      // phrase is in vocabulary, data returned
    NotIndexed,   // phrase is NOT in vocabulary (too rare historically)
    Invalid,      // phrase failed validation (e.g., > 3 tokens)
}

struct Point {
    t: String,     // bucket timestamp (ISO)
    value: f64,    // relative frequency
}
```

---

## Requirements

* output sorted by time ascending
* one series per input phrase
* for `Indexed` series: missing buckets must be filled (value = 0.0)
* for `NotIndexed` series: points array is empty
* for `Invalid` series: points array is empty

---

## Rationale

* stable structure for frontend charting
* avoids frontend needing to normalize missing data
* status field enables frontend to distinguish "legitimately zero" from "not tracked"

---

## Flexibility

* timestamp format may be changed to integer if needed
* additional metadata fields may be added

---

# 5. Query Normalization

## Spec (mandatory)

For each input phrase:

1. apply RFC-001 tokenizer
2. count tokens
3. reject if token count ∉ {1,2,3}
4. join tokens with single space
5. use resulting string as lookup key

---

## Example

```text id="7wrn6k"
Input: "Node.js"
→ ["node.js"]
→ "node.js"
```

---

## Rationale

* ensures exact alignment between query and stored ngrams

---

## Constraints

* must use same tokenizer version as ingestion

---

## Flexibility

* none (must remain identical to ingestion logic)

---

# 6. Missing Data Handling

## Spec (mandatory)

If no rows exist for `(bucket, ngram)`:

* return value = 0.0

---

## Rationale

* ensures continuous time series
* simplifies frontend logic

---

## Flexibility

* agent may perform zero-fill in backend or frontend

---

# 7. Granularity Handling

## Spec

* base storage: daily
* derived via SQL aggregation

Mapping:

```text id="aq8y3c"
Day   → raw bucket
Week  → toStartOfWeek
Month → toStartOfMonth
Year  → toStartOfYear
```

---

## Rationale

* avoids storing redundant aggregates

---

## Flexibility

* may precompute aggregates if performance requires

---

# 8. Smoothing

## Spec (recommended)

* smoothing should be applied **frontend-side**
* API returns raw (unsmoothed) series data
* frontend applies simple moving average in memoized transform

---

## Rationale

* keeps API surface minimal
* smoothing slider changes become instant (no round-trip)
* reduces API complexity and cache variations
* raw data is more useful for future features

---

## Implementation

Frontend applies smoothing via:

```typescript
function applySmoothing(points: Point[], window: number): Point[] {
  // simple moving average over `window` buckets
}
```

Memoize this transform based on `(points, window)`.

---

## Flexibility

* backend-side smoothing is allowed if there is a compelling reason
* if implemented backend-side, `smoothing` becomes part of query key (affects caching)

---

# 9. OpenAPI Generation

## Spec (mandatory)

* all request/response types must derive:

  * `ToSchema`

* all endpoints must use:

  * `#[utoipa::path(...)]`

* OpenAPI spec must be exposed at:

```text id="j4o4js"
/openapi.json
```

---

## Rationale

* enables automatic SDK generation
* keeps schema centralized

---

## Constraints

* no undocumented endpoints
* no untyped request/response bodies

---

## Flexibility

* spec may also be exported as static file

---

# 10. TypeScript SDK Generation

## Spec (mandatory)

* frontend must generate SDK using Kubb
* OpenAPI spec is the only input

---

## Output must include:

* typed client functions
* request/response types
* optional React hooks

---

## Rationale

* ensures frontend/backend type consistency

---

## Constraints

* no manual API types in frontend
* generated code must not be edited manually

---

## Flexibility

* generation strategy (hooks vs plain client) may vary

---

# 11. Error Handling

## Spec

API must return structured errors:

```json id="hpfvge"
{
  "error": {
    "code": "INVALID_QUERY",
    "message": "Phrase must tokenize to 1-3 tokens"
  }
}
```

---

## Rationale

* predictable error handling for frontend

---

## Flexibility

* error codes may expand

---

# 12. Rate Limiting

## Spec (mandatory)

API must implement basic rate limiting to prevent abuse.

### Recommended limits

* **per-IP**: 60 requests/minute
* **burst**: 10 requests/second

### Implementation options

1. **Caddy rate_limit directive** (preferred for simplicity)

```caddyfile
rate_limit {
  zone api {
    key {remote_host}
    events 60
    window 1m
  }
}
```

2. **Rust middleware** (tower-governor or similar)

```rust
use tower_governor::{GovernorLayer, GovernorConfigBuilder};

let config = GovernorConfigBuilder::default()
    .per_second(10)
    .burst_size(60)
    .finish()
    .unwrap();
```

---

## Rationale

* public API without rate limiting is vulnerable to abuse
* even accidental client bugs can cause excessive load
* small VPS has limited resources

---

## Flexibility

* exact limits may be tuned based on observed traffic
* may add per-route limits if needed

---

# 13. Versioning

## Spec

* API must include:

  * `tokenizer_version` internally
* frontend does not supply version

---

## Rationale

* ensures query consistency with stored data

---

## Flexibility

* future support for multiple versions allowed

---

# 14. Prohibited Designs

Agent must NOT:

* define API types separately in TS
* bypass OpenAPI generation
* use stringly-typed JSON responses
* allow queries without normalization
* expose raw DB schema directly

---

# 15. Acceptance Criteria

System is valid if:

* frontend SDK compiles without manual edits
* all API types originate from Rust
* queries return correct normalized results
* no type mismatch between backend and frontend
* OpenAPI spec fully describes API

---

## Final Note for Agent

If proposing changes:

* must preserve “Rust → OpenAPI → TS” pipeline
* must not introduce duplicated type definitions
* must maintain strict alignment with tokenizer and n-gram rules

