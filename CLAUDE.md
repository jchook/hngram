# HN N-gram Viewer

Explore word and phrase trends in Hacker News comments over time (like Google Ngram Viewer).

See `docs/design-decisions.md` for architectural rationale behind key choices.

## Codebase Layout

```
server/                     # Rust workspace
├── crates/
│   ├── api/                # HTTP API server (axum + utoipa)
│   ├── clickhouse/         # ClickHouse client (hn-clickhouse crate)
│   ├── ingestion/          # Data pipeline CLI
│   └── tokenizer/          # Tokenization + n-gram counting
├── etc/
│   ├── caddy/              # Reverse proxy (prod)
│   └── clickhouse/         # DB config + schema
└── openapi.json            # API spec (generates client SDK)

client/                     # Bun + React + TypeScript + Rsbuild
├── src/gen/                # Generated SDK (gitignored)
└── kubb.config.ts          # SDK generation config

docs/                       # RFCs, design decisions
```

## Development

Docker only runs ClickHouse. The API and client run locally via Rust/Bun.

```bash
docker compose up -d              # Start ClickHouse
cd server && just up              # API server (cargo run -p api --bin api)
cd client && just up              # Frontend dev server (bun run dev)
```

Or use `process-compose up` from the project root to run both API and client together.

### Justfile recipes

**server/justfile:** `up` (run API), `test` (cargo test), `openapi` (regenerate spec)
**client/justfile:** `up` (dev server), `gen` (regenerate SDK from OpenAPI spec)

### OpenAPI → SDK pipeline

The OpenAPI spec is generated from Rust types, not hand-written. After any API type change:

```bash
cd server && just openapi
cd client && just gen
```

The `generate_openapi` binary lives at `server/crates/api/src/bin/generate_openapi.rs` and imports `ApiDoc` from the api crate's lib.

### Ingestion pipeline

Three subcommands: download raw data, process it, and (for bootstrap) import into ClickHouse.

```bash
cd server
# Bootstrap (on local workstation, no ClickHouse needed):
cargo run -p ingestion -- download                                        # Fetch Parquet from HuggingFace
cargo run -p ingestion -- process --output parquet                        # Full corpus → output/*.parquet
# Transfer output/ to prod, then:
cargo run -p ingestion -- import                                          # Load parquet → staging → atomic swap

# Incremental (on prod VPS, direct to ClickHouse):
cargo run -p ingestion -- download --start 2026-03 --end 2026-03          # Fetch latest month
cargo run -p ingestion -- process --start 2026-03 --end 2026-03           # Process new comments
# Use --start/--end to scope to a subset of months
```

`process --output parquet` runs the full corpus from scratch (two-pass: build vocabulary, then filter and write). `process --output clickhouse` (default) is incremental — reads watermark from `ingestion_log` table, processes only new comments, inserts directly.

All state on prod lives in ClickHouse (no local manifest files). Data is append-only — vocabulary and counts only grow. Eventual consistency via ReplacingMergeTree is acceptable everywhere.

### Environment

All Rust entry points load `.env` via `dotenvy`. Key variables:

| Variable | Default | Notes |
|----------|---------|-------|
| `CLICKHOUSE_HOST` | `localhost` | Use `clickhouse` in prod (docker network) |
| `CLICKHOUSE_PORT` | `8123` | |
| `CLICKHOUSE_DATABASE` | `hn_ngram` | |
| `API_PORT` | `3000` | |
| `RUST_LOG` | `info` | |
| `PRUNE_MIN_{N}GRAM_GLOBAL` | 20 (2gram), 10 (3gram) | Set high (e.g., 500) for fast dev testing |
| `PRUNE_MIN_{N}GRAM_BUCKET` | 3 (2gram), 5 (3gram) | |

### Deployment

```bash
# Dev: ClickHouse in Docker, everything else local
docker compose up -d

# Prod: Caddy + API + ClickHouse, all in Docker (standalone file, not merged with dev)
docker compose -f docker-compose.prod.yml up -d
```

Prod is a standalone compose file with its own ClickHouse config (`config.prod.xml` for memory/pool limits, `users.prod.xml` for auth). Secrets are mounted from the host via Docker Compose secrets (see `docs/RFC-011-production-security.md`).

## Gotchas

- **Endpoint naming affects SDK hook names.** Kubb derives hook names from the OpenAPI `operationId`, which utoipa derives from the Rust function name. We renamed `/query` → `/ngram` and `fn query()` → `fn ngram()` because `useQuery` conflicted with TanStack Query's `useQuery`. If adding endpoints, avoid names that collide with library exports.

- **Use `time` crate everywhere, never `chrono`.** The `clickhouse` crate's serde helpers use `time::Date`. Using `chrono` creates conversion friction.

- **Bind `time::Date` as formatted strings in queries.** The default serde for `time::Date` serializes as a tuple, which ClickHouse rejects. Use `"YYYY-MM-DD"` strings when calling `.bind()`. See `HnClickHouse::date_str()`.

- **Kubb client adapter contract.** The generated hooks expect `client/src/lib/client.ts` to: (1) default-export an async function accepting `RequestConfig` and returning `{data: T}`, (2) export types `RequestConfig`, `ResponseErrorConfig`, and `Client`. The function must accept 3 type params `<TData, TError, TBody>` even though only `TData` is used.

- **@mantine/dates version must match @mantine/core.** They are co-versioned (both 7.x or both 8.x). A mismatch causes runtime errors.

- **API returns single phrase per request.** The frontend makes parallel requests (one per phrase) for independent caching. Don't revert to a multi-phrase API.

- **`server/openapi.json` is a generated artifact.** Don't edit it manually.

- **Pruning thresholds are n-keyed.** Thresholds are stored per n-gram order (not hardcoded for bigram/trigram), supporting future 4-gram+ without code changes.

## Key Concepts

- **Tokenizer versioned** — all data tagged with version; increment on ANY rule change
- **Daily base granularity** — stored daily, aggregated to week/month/year at query time
- **Pre-aggregated counts** — no raw text stored, only n-gram counts per day
- **Append-only data** — vocabulary and counts only grow, never dropped or reprocessed
- **Pruning thresholds** — configurable per n-gram order via env vars

## Crate Responsibilities

| Crate | Purpose |
|-------|---------|
| `tokenizer` | Tokenization rules (RFC-001), n-gram counting/pruning (RFC-002) |
| `hn-clickhouse` | Schema types, insert/query functions (RFC-003) |
| `ingestion` | HuggingFace → tokenize → ClickHouse pipeline (RFC-004) |
| `api` | HTTP endpoints, OpenAPI spec (RFC-005) |

## Tokenizer Version

Stored as `LowCardinality(String)` in ClickHouse. Currently `"1"` (from `TOKENIZER_VERSION: u8`).

**Increment on ANY tokenization rule change** — changing rules invalidates all existing data.

Defined in: `server/crates/tokenizer/src/lib.rs`

## Where to Look

| What | Where |
|------|-------|
| Tokenization rules | `server/crates/tokenizer/src/lib.rs` |
| N-gram counting/pruning | `server/crates/tokenizer/src/counter.rs` |
| ClickHouse types/queries | `server/crates/clickhouse/src/lib.rs` |
| DB schema | `server/etc/clickhouse/init/001-schema.sql` |
| API types + handlers | `server/crates/api/src/lib.rs` |
| API server startup | `server/crates/api/src/main.rs` |
| OpenAPI generator | `server/crates/api/src/bin/generate_openapi.rs` |
| Ingestion pipeline | `server/crates/ingestion/src/` |
| Frontend app | `client/src/App.tsx` |
| URL state management | `client/src/features/query/useQueryState.ts` |
| Data transforms | `client/src/features/chart/transforms.ts` |
| SDK client adapter | `client/src/lib/client.ts` |
| Generated SDK (read-only) | `client/src/gen/` |
| Design decisions | `docs/design-decisions.md` |
| Detailed specs | `docs/RFC-*.md` |
| Optimization decisions | `docs/RFC-007-optimizations.md` |
