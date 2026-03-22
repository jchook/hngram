# HN N-gram Viewer — TODO

## Critical (blocking for launch)

- [ ] **Ingestion pipeline** — `server/crates/ingestion/src/main.rs` is a stub
  - [ ] Fetch Parquet files from HuggingFace
  - [ ] Parse/filter comments (type=comment, not deleted/dead, text not null)
  - [ ] Tokenize and generate n-grams (RFC-001, RFC-002)
  - [ ] Pass 1: compute global counts, build vocabulary
  - [ ] Pass 2: aggregate daily counts with pruning
  - [ ] Batch insert into ClickHouse
  - [ ] Idempotency / checkpoint tracking

- [ ] **Frontend implementation** — `client/src/App.tsx` is a placeholder
  - [ ] Generate SDK: `cd client && bun run generate`
  - [ ] URL state hook (`useQueryState` — `URLSearchParams` + `pushState`/`popstate`)
  - [ ] Query form wired to generated React Query hooks (`/query` + `/totals`)
  - [ ] Data transformation layer (`features/chart/transform.ts`):
    - [ ] Zero-fill sparse daily counts
    - [ ] Granularity aggregation (sum counts/totals by period)
    - [ ] Relative frequency computation (count / total)
    - [ ] Smoothing (centered moving average)
  - [ ] ECharts time series chart component
  - [ ] Loading / error / empty / not-indexed states
  - [ ] Default example query on first load

- [ ] **Vocabulary status logic** — API returns `NotIndexed` when ClickHouse returns no rows
  - [ ] Unigrams should always be `Indexed` (never pruned per RFC-002)
  - [ ] Bigrams/trigrams: check `ngram_vocabulary` table to distinguish "not indexed" from "indexed but zero in range"

## Medium (should have for launch)

- [ ] **Caddy rate limiting** — Caddyfile missing `rate_limit` block (RFC-005 §13, 60 req/min per IP)
- [ ] **Production docker-compose** — `docker-compose.prod.yml` doesn't include ClickHouse service

## Low (polish)

- [ ] **OpenAPI spec sync** — add CI check or pre-commit hook to keep `openapi.json` in sync with Rust types
- [ ] **Methodology/about modal** — optional per RFC-006
