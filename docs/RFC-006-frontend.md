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
* Mantine
* Kubb-generated SDK from OpenAPI
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

* React Router or equivalent lightweight router
* Zustand only if local state becomes awkward

Not allowed initially:

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

Allowed later:

* `/about`
* `/methodology`

Not required for v1.

---

## 5. Routing and URL State

## Spec (mandatory)

All query state must be representable in the URL.

Use query params for:

* `q` = comma-separated phrases
* `start` = ISO date
* `end` = ISO date
* `g` = granularity (`day|week|month|year`)
* `s` = smoothing integer

Example:

```text
/?q=rust,go,startups&start=2015-01-01&end=2025-01-01&g=month&s=3
```

---

## Requirements

* loading a URL must reconstruct the UI state
* editing controls must update URL
* back/forward browser navigation must work
* invalid URL params must degrade safely to defaults or validation errors

---

## Rationale

URL shareability is core to this product.

---

## Flexibility

Agent may debounce URL updates slightly to reduce noise, but state must remain shareable and stable.

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

Frontend must use only Kubb-generated API types/client/hooks for backend communication.

No hand-written duplicate request/response types.

---

## Requirements

* generated code must be treated as read-only
* app-specific wrappers are allowed only in thin adapter functions
* query keys must be stable and derived from normalized request params

---

## Rationale

Prevents frontend/backend drift and keeps API layer consistent with RFC-005.

---

## Flexibility

Agent may choose one of:

* generated hooks directly
* generated client + custom TanStack Query hooks

Preferred approach for simplicity:

* generated client + one thin app-level query hook

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

Defaults:

* sensible preset range if URL is empty
* recommended default:

  * `start = 2007-01-01`
  * `end = today`

---

### Rationale

Simple and explicit. Avoid hidden date logic.

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

Use a small integer control.

Allowed values:

* `0` to `12`

Default:

* `3`

Interpretation:

* simple moving average window as defined by backend/frontend implementation from RFC-005

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

Must include:

* phrases
* start
* end
* granularity
* smoothing if backend-applied

If smoothing is frontend-applied, smoothing must not affect API query key.

---

## Rationale

Ensures correct cache behavior and avoids unnecessary requests.

---

## Flexibility

Agent may choose whether smoothing is backend-side or frontend-side per RFC-005.

Preferred for simplicity:

* fetch unsmoothed series
* apply smoothing in frontend memoized transform

This reduces API complexity and makes slider changes instant.

---

## 11. Charting Model

## Spec (mandatory)

Use a single line chart.

One line per phrase.

Chart must support:

* multiple series
* hover tooltip
* legend
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

## 11.4 Series Color

### Spec

Use chart library defaults or a simple stable palette.

No user-configurable color system in v1.

---

## 11.5 Missing Buckets

### Spec

Chart input must already be zero-filled into continuous series before rendering.

---

## Rationale

Avoid broken lines and frontend ambiguity.

---

## 11.6 Smoothing Display

### Spec

If smoothing > 0:

* display smoothed values in chart
* indicate smoothing in UI near chart or controls

Optional:

* retain raw values in tooltip if easy

---

## 12. Data Transformation Layer

## Spec (mandatory)

Create a small transformation layer between API response and chart props.

Responsibilities:

* zero-fill buckets if not already done
* apply smoothing if frontend-side
* map API data into ECharts series format
* format tooltip values

This logic must not be embedded directly in page JSX.

---

## Rationale

Keeps page component simple and prevents chart-coupled business logic from spreading.

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

* explicit “no data” or “no indexed matches” message
* not a blank chart

---

## Rationale

These states are common and must be obvious.

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
* URL fully represents query state
* browser navigation restores state correctly
* chart renders one line per phrase
* loading/error/empty states are handled
* frontend uses generated API types/client
* implementation remains small and understandable

---

## 22. Final Guidance for Agent

When making implementation choices:

Prefer:

* fewer files
* fewer abstractions
* explicit data flow
* generated API integration
* one chart, one page, one query model

Avoid:

* framework cleverness
* generalized state machinery
* custom chart infrastructure
* overengineering for future hypothetical features

The frontend should feel like a thin, reliable shell over a typed API and a single time-series chart.

