# RFC-006 (Agent-Oriented)

## Frontend Architecture: Hacker News N-gram Viewer

## 0. Scope

Define frontend architecture for a simple HN n-gram viewer.

Covers:

* framework and libraries
* application structure
* routing and URL state
* API integration
* query input behavior
* charting behavior
* loading, error, and empty states
* constraints to keep implementation simple

Assumes:

* RFC-001 tokenizer
* RFC-002 n-gram generation + pruning
* RFC-003 ClickHouse schema/query model
* RFC-005 API + OpenAPI + SDK generation

---

## 1. Product Goal

## Spec (mandatory)

Frontend must implement a minimal interactive viewer for normalized HN n-gram trends.

Primary user flow:

1. user enters one or more phrases
2. user selects date range and granularity
3. app queries backend
4. app displays one line series per phrase
5. user can share URL and reload state exactly

---

## Rationale

This is a narrow tool, not a general analytics platform.

Frontend should optimize for:

* speed of implementation
* reliability
* simple mental model
* low maintenance cost

Not for:

* extensible BI/dashboard behavior
* arbitrary query building
* plugin architecture

---

## Flexibility

Agent may improve ergonomics only if complexity stays low.

Avoid introducing additional abstraction layers unless they clearly reduce maintenance.

---

## 2. Stack

## Spec (mandatory)

Use:

* React
* TypeScript
* Mantine (including `@mantine/dates`)
* dayjs (date library, required by Mantine dates)
* Kubb-generated SDK from OpenAPI (types + React Query hooks)
* TanStack Query
* Apache ECharts

---

## Rationale

* React + TypeScript: standard frontend stack
* Mantine: fast UI composition with low ceremony
* Kubb: generated typed API client from backend OpenAPI
* TanStack Query: fetch/cache/loading/error management
* ECharts: robust time-series interactions with low custom charting work

---

## Flexibility

Allowed:

* Zustand only if local state becomes awkward

Not allowed:

* React Router (single view — use browser `URLSearchParams` + `history.pushState` directly)
* Redux
* XState
* custom API client handwritten alongside generated one
* multiple charting libraries

---

## 3. Architecture Overview

## Spec (mandatory)

Frontend structure must stay shallow.

Recommended top-level sections:

```text
src/
  app/
  pages/
  components/
  features/query/
  features/chart/
  lib/
  gen/
```

Where:

* `app/` = providers and app bootstrap
* `pages/` = page-level components
* `components/` = shared presentational components
* `features/query/` = query form + URL state helpers
* `features/chart/` = chart config + data shaping
* `lib/` = utility code
* `gen/` = Kubb-generated SDK/types/hooks

---

## Rationale

The app is small. Feature folders are enough. No need for elaborate layered architecture.

---

## Flexibility

Agent may collapse folders further if codebase remains understandable.

Do not split into many architectural tiers.

---

## 4. Page Model

## Spec (mandatory)

V1 has a single primary page:

```text
/
```

This page contains:

* title/header
* query controls
* chart area
* optional results summary / status
* optional footer/about text

No secondary application pages are required in v1.

---

## Rationale

Single-purpose tool. No need for multi-page product structure.

---

## Flexibility

Secondary content (about, methodology) should use modals, not separate routes.

No router needed for v1.

---

## 5. URL State

## Spec (mandatory)

All query state must be representable in the URL via query params.

No router library. Use `URLSearchParams` + `history.pushState` / `popstate` directly.

### URL params:

* `q` = comma-separated phrases
* `start` = ISO date (YYYY-MM-DD)
* `end` = ISO date (YYYY-MM-DD)
* `g` = granularity (`day|week|month|year`) — sent to API as `granularity`
* `s` = smoothing integer — frontend-only, NOT sent to API

Example:

```text
/?q=rust,go,startups&start=2015-01-01&end=2025-01-01&g=month&s=3
```

### Granularity, smoothing, and the API query key

`g` (granularity) is sent to the API and included in the TanStack Query key. `s` (smoothing) is a **frontend-only display parameter** — it is persisted in the URL for shareability but must NOT:

* be sent to the backend API
* be included in the TanStack Query key

Changing smoothing is instant (no network round-trip). Changing granularity triggers new API requests (but per-phrase caching means only uncached phrase × granularity combinations are fetched).

---

## Requirements

* loading a URL must reconstruct the UI state
* editing controls must update URL via `history.pushState`
* back/forward browser navigation must work (`popstate` listener)
* invalid URL params must degrade safely to defaults or validation errors

---

## Rationale

URL shareability is core to this product. Browser APIs are sufficient for a single-view app — no router needed.

---

## 6. State Model

## Spec (mandatory)

Use three state categories only:

### 6.1 URL state

Source of truth for:

* phrases
* date range
* granularity
* smoothing

### 6.2 server state

Managed by TanStack Query:

* query response
* loading/error
* cache

### 6.3 ephemeral UI state

Local component state only for:

* input editing before submit
* chart hover interactions
* minor presentational toggles

---

## Rationale

This keeps state ownership simple and debuggable.

---

## Flexibility

Do not introduce global client state store unless a concrete need emerges.

---

## 7. API Integration

## Spec (mandatory)

### Endpoint

`GET /query` with query parameters (per RFC-005). Not a POST with JSON body. Each request queries a **single phrase**.

### SDK

Frontend must use Kubb-generated React Query hooks and types for backend communication. No hand-written duplicate request/response types.

Kubb is already configured to generate:

* TypeScript types (from OpenAPI schemas)
* React Query hooks (for GET endpoints)
* A thin client adapter (`@/lib/client`)

Use the generated hooks directly. They provide typed request params, typed responses, and automatic query key management.

### Per-phrase parallel requests

The frontend makes **one request per phrase** (up to 10 phrases in parallel). Each phrase gets its own TanStack Query instance with key `["query", phrase, start, end, granularity]`.

Benefits:

* adding a new phrase does not re-fetch existing cached phrases
* removing a phrase does not invalidate any cache entries
* each phrase × granularity × date range is independently cached by both TanStack Query and HTTP caches

### Error handling

The client adapter (`lib/client.ts`) must parse structured error responses from RFC-005:

```json
{ "error": { "code": "INVALID_PHRASE", "message": "Phrase tokenizes to more than 3 tokens" } }
```

The adapter must extract and surface the `error.code` and `error.message` fields so the UI can map them to user-friendly messages (see §19).

### Response structure

Each request returns a **flat object** (per RFC-005):

```typescript
{
  phrase: string;         // original input
  normalized: string;     // tokenized form used for lookup
  status: "indexed" | "not_indexed" | "invalid";
  points: Array<{ t: string; v: number }>;  // v is f64 relative frequency
  meta: {
    tokenizer_version: string;
    start: string;
    end: string;
    granularity: string;
  };
}
```

The `meta` object is available for debugging but does not need prominent display in v1.

### Phrase matching

Each response corresponds to exactly one phrase. The frontend matches responses to user-entered phrases by the `phrase` field (which echoes the input).

---

## Requirements

* generated code must be treated as read-only
* query keys must be stable and derived from request params (excluding smoothing)
* each phrase results in an independent TanStack Query with key `["query", phrase, start, end, granularity]`

---

## Rationale

Kubb-generated hooks eliminate SDK drift. Per-phrase requests enable independent caching — the most common user action (adding/removing a phrase) reuses all existing cached data.

---

## 8. Query Form

## Spec (mandatory)

The query controls must include:

* phrases input
* start date input
* end date input
* granularity select
* smoothing control
* submit/apply action

---

## 8.1 Phrase Input

### Spec

Use a single text input for phrases.

Input format:

* comma-separated phrases

Examples:

* `rust`
* `rust, go`
* `machine learning, deep learning`

On submit:

* trim whitespace around phrases
* drop empty entries
* preserve user-entered display phrases for UI
* backend remains source of truth for actual normalization

Maximum phrases:

* 10

---

### Rationale

Single input is simpler than tag editors and sufficient for v1.

---

### Flexibility

Agent may implement chips/tags later, but plain text input is preferred initially.

---

## 8.2 Date Inputs

### Spec

Use two date inputs:

* start date
* end date

Defaults (matching API defaults from RFC-005):

* `start = 2011-01-01` (from `HN_DEFAULT_START_DATE` — when HN had meaningful comment volume)
* `end = today`

---

### Rationale

Simple and explicit. Defaults match the API so omitting params produces consistent behavior.

---

## 8.3 Granularity Input

### Spec

Allowed values:

* day
* week
* month
* year

Default:

* month

---

### Rationale

Month is the best general default for readability and noise reduction.

---

## 8.4 Smoothing Input

### Spec

Use a small integer control (slider or number input).

Allowed values:

* `0` to `12`

Default:

* `3`

Interpretation:

* centered simple moving average applied **frontend-only** (per RFC-005)
* changing this control does NOT trigger an API request
* persisted in URL as `s` param for shareability

---

### Rationale

Small bounded smoothing control matches user expectations and avoids complexity.

---

## 8.5 Submit Behavior

### Spec

Prefer explicit submit/apply button.

Optional:

* submit on Enter in phrase field

Do not refetch on every keystroke.

---

### Rationale

Reduces accidental requests and keeps UX predictable.

---

## 9. Validation Behavior

## Spec (mandatory)

Frontend should perform light validation before request:

* phrase count <= 10
* start and end are valid dates
* start <= end
* smoothing in range
* granularity valid enum

Frontend must not attempt to reimplement tokenizer validation.

Backend remains authority for:

* phrase normalization
* 1–3 token rule after tokenization

---

## Rationale

Frontend should catch obvious user mistakes but not duplicate backend semantics.

---

## Flexibility

Agent may display inline validation messages or top-level form errors.

Keep validation implementation simple.

---

## 10. Data Fetching Model

## Spec (mandatory)

Query execution is user-driven, not keystroke-driven.

Fetch occurs when:

* page loads with valid URL params
* user submits updated query
* URL changes via browser navigation

---

## Query key

Each phrase gets its own TanStack Query. The key must include:

* phrase (single phrase, not comma-separated list)
* start
* end
* granularity

Must NOT include:

* smoothing (frontend-only — not sent to API)

---

## Rationale

Per-phrase query keys mean adding or removing a phrase reuses all existing cached data. Smoothing is applied client-side, so excluding it from the query key means changing smoothing reuses cached data with no network request.

---

## 11. Charting Model

## Spec (mandatory)

Use a single line chart.

One line per phrase.

Chart must support:

* multiple series
* hover tooltip
* legend (using user-entered phrase text, not server-normalized form)
* responsive resize

Preferred additional support:

* data zoom / brush
* export image later if cheap

---

## Rationale

One chart is enough for the product.

---

## Flexibility

Do not add multiple chart types in v1.

---

## 11.1 Chart Library

### Spec

Use Apache ECharts.

Wrap chart setup behind a small local component boundary, e.g.:

```jsx
<TimeSeriesChart />
```

Do not scatter ECharts option-building logic across the app.

---

### Rationale

Keeps charting contained and makes library replacement possible if ever needed.

---

## 11.2 X Axis

### Spec

X axis is time.

Use backend-provided buckets.

Display formatting should depend on granularity:

* day: compact date
* week: week start date
* month: YYYY-MM
* year: YYYY

---

## 11.3 Y Axis

### Spec

Y axis is relative frequency.

Display as human-readable decimal or percent-like value.

Do not over-format to imply false precision.

Recommended:

* compact scientific/decimal formatting for small values

---

## Rationale

N-gram frequencies will often be very small.

---

## 11.4 Series Labels

### Spec

Chart legend and tooltip must display the **user-entered phrase** (trimmed), NOT the server-returned `normalized` form.

The frontend matches `series[i]` to `userInputPhrases[i]` by array index (see §7 phrase ordering contract).

The `normalized` field may optionally be shown in a secondary position (e.g., tooltip detail) for debugging, but never as the primary label.

Example:

* User enters: `Node.js, C++`
* Server normalizes to: `node.js, c++`
* Chart legend shows: `Node.js`, `C++` (user's casing)

---

## 11.5 Series Color

### Spec

Use chart library defaults or a simple stable palette.

No user-configurable color system in v1.

---

## 11.6 Missing Buckets

### Spec

The backend returns **sparse** data — only buckets with non-zero counts. The frontend must zero-fill gaps to produce continuous series before rendering.

Zero-fill is a frontend responsibility (along with smoothing) to reduce network payload and server work.

For `not_indexed` or `invalid` series, `points` is empty — do not render a line (see §13).

---

## 11.7 Smoothing Display

### Spec

If smoothing > 0:

* display smoothed values in chart
* indicate smoothing window in UI near chart or controls (e.g., "smoothing: 3")

Optional:

* show raw (unsmoothed) value in tooltip alongside smoothed value

---

## 12. Data Transformation Layer

## Spec (mandatory)

Create a small transformation layer between API response and chart props.

Responsibilities:

* zero-fill sparse backend data into continuous bucket sequences (using `meta.start`, `meta.end`, `meta.granularity`)
* apply centered moving average smoothing
* map data into ECharts series format
* format tooltip values
* match series to user-entered phrases

NOT frontend responsibilities (handled by backend):

* tokenization / normalization — backend handles via RFC-001
* relative frequency computation — server JOINs with `bucket_totals`
* granularity aggregation — server performs GROUP BY

This logic must not be embedded directly in page JSX.

### Zero-fill implementation

The frontend must generate the expected bucket sequence from `meta.start`, `meta.end`, and `meta.granularity`, then fill in values from the sparse backend data.

Missing buckets get `v: 0`.

Use dayjs for bucket alignment:

* `day` — increment by 1 day
* `week` — align to Monday, increment by 7 days
* `month` — align to 1st of month, increment by 1 month
* `year` — align to Jan 1, increment by 1 year

### Smoothing implementation

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

### Transform pipeline

```text
sparse relative frequencies (from /query, one response per phrase)
  → zero-fill using meta.start, meta.end, meta.granularity
  → apply smoothing
  → ECharts format
```

Each step is independently memoizable:

* zero-fill — recomputes only when raw data or date range changes
* smoothing — recomputes only when smoothing window changes

Changing smoothing reuses cached API data — no network request.

---

## Rationale

Server handles frequency computation and granularity aggregation. Frontend handles zero-fill (sparse data), smoothing (display-only), and chart formatting. Smoothing changes are instant with no API round-trip. All frontend transforms are simple array arithmetic, trivial in JS.

---

## Flexibility

A single file/module is sufficient. No need for service classes.

---

## 13. Loading, Error, and Empty States

## Spec (mandatory)

### Loading

Show:

* chart skeleton or placeholder
* preserve existing chart while fetching new data if possible

### Error

Show:

* clear error message
* retain form state
* allow retry by resubmitting

### Empty

Show:

* explicit "no data" or "no indexed matches" message
* not a blank chart

### Not Indexed vs Zero (Important)

Frontend must distinguish between two different "empty" cases per RFC-002 §11.1:

1. **Indexed but zero**: phrase is in vocabulary, but has zero occurrences in selected range
   - Show the series line at y=0
   - Normal behavior, no special messaging needed

2. **Not indexed**: phrase is NOT in vocabulary (historically too rare)
   - Show inline warning: "This phrase is not indexed (too rare historically)"
   - Do not show a flat zero line (misleading)
   - Use `status` field from API response to detect

Example UI:

```
┌─────────────────────────────────────────────┐
│ "rust"           ✓ indexed                  │
│ "machine learning" ✓ indexed                │
│ "xyzzy foobar"   ⚠ not indexed (too rare)   │
└─────────────────────────────────────────────┘
```

---

## Rationale

These states are common and must be obvious. Users need to understand why data is missing.

---

## Flexibility

Agent may use Mantine components such as Alert, Loader, Skeleton.

Keep presentation lightweight.

---

## 14. Default Experience

## Spec (mandatory)

On first load without URL params:

* show default query state in controls
* optionally auto-run a default example query

Recommended default example:

* `rust, python`
* last 10–15 years
* month granularity
* smoothing 3

Alternative acceptable behavior:

* show empty chart and wait for submit

---

## Rationale

A default example makes the page immediately understandable.

---

## Flexibility

Agent may disable default fetch if minimizing backend load is preferred.

---

## 15. Accessibility and Interaction

## Spec (mandatory)

Must support:

* keyboard access to form controls
* visible labels
* sufficient contrast via Mantine defaults
* meaningful loading/error text

Chart accessibility should be reasonable but does not need full screen-reader semantic richness in v1.

---

## Rationale

Small project, but baseline accessibility should still be respected.

---

## 16. Styling and Layout

## Spec (mandatory)

Keep layout simple.

Recommended structure:

* centered page container
* header/title
* control panel/card
* chart card
* optional small methodology/footer text

Use Mantine components such as:

* `AppShell` or simple `Container`
* `Stack`
* `Group`
* `Card` / `Paper`
* `TextInput`
* `Select`
* `Button`
* `Alert`

---

## Rationale

This is a tool, not a marketing site.

---

## Flexibility

Do not introduce extensive theming, animation systems, or design tokens in v1.

---

## 17. Recommended Component Breakdown

## Spec (recommended)

Minimal component set:

* `NgramViewerPage`
* `QueryControls`
* `TimeSeriesChart`
* `QueryStatus`
* `MethodologyNote` (optional)

Support modules:

* `useQueryState`
* `useNgramSeriesQuery`
* `buildChartOption`
* `applySmoothing`
* `fillMissingBuckets`

---

## Rationale

Enough modularity to keep files readable, without over-componentizing.

---

## Flexibility

Agent may merge very small components if simpler.

---

## 18. Performance Constraints

## Spec (mandatory)

Frontend must remain simple and performant for typical response sizes.

Assume typical result:

* <= 10 series
* daily/monthly/yearly points over many years

Requirements:

* no unnecessary rerenders from uncontrolled state churn
* expensive transforms memoized where helpful
* chart recreated only when data/settings change

---

## Rationale

Even a simple app can feel sluggish if chart config is rebuilt carelessly.

---

## Flexibility

No premature optimization beyond memoizing chart transforms and stable query params.

---

## 19. Error Surface Contract with Backend

## Spec (mandatory)

Frontend must handle structured backend errors from RFC-005.

At minimum, support:

* invalid query
* invalid date range
* unsupported tokenized phrase length
* server failure

Map backend error codes to user-friendly messages.

---

## Rationale

Do not leak backend/internal wording directly if it is confusing.

---

## 20. Non-Goals / Prohibited Complexity

## Spec (mandatory)

Do NOT implement in v1:

* user accounts
* saved searches
* multiple pages of analytics
* advanced filter builder
* comparisons across multiple corpora
* live-updating chart every few seconds
* handwritten API types duplicating generated ones
* global state framework beyond TanStack Query
* custom design system

---

## Rationale

These would add complexity without helping the core product.

---

## 21. Acceptance Criteria

Frontend is valid if:

* user can enter phrases and run query
* URL fully represents query state (including frontend-only smoothing)
* browser navigation restores state correctly
* chart renders one line per phrase, labeled with user-entered text (not normalized)
* sparse backend data is zero-filled into continuous series
* smoothing is applied client-side with no API round-trip
* loading/error/empty/not-indexed states are handled
* frontend uses Kubb-generated API types and React Query hooks
* implementation remains small and understandable

---

## 22. Final Guidance for Agent

When making implementation choices:

Prefer:

* clear separation of concerns — each file should have one reason to change
* explicit data flow
* generated API integration
* one chart, one page, one query model

Avoid:

* framework cleverness
* generalized state machinery
* custom chart infrastructure
* overengineering for future hypothetical features
* collapsing distinct concerns into one file just to reduce file count

The goal is **maintainability**, not minimalism. A 15-line wrapper component is fine if it represents a real boundary (e.g. chart library integration). Don't add abstractions for hypothetical growth, but don't avoid them when they clarify responsibility.

