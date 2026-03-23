# HN N-gram Viewer

Explore word and phrase trends in Hacker News comments over time (like Google Ngram Viewer).

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

docs/                       # RFCs with detailed specs
```

## Development

```bash
docker compose up -d                  # Start API + ClickHouse
cd client && bun run dev              # Frontend dev server
cd server && cargo test               # Run all tests
```

### OpenAPI → SDK pipeline

The OpenAPI spec is generated from Rust types, not hand-written. After any API type change:

```bash
cd server && cargo run -p api --bin generate_openapi > openapi.json
cd client && bun run generate
```

The `generate_openapi` binary lives at `server/crates/api/src/bin/generate_openapi.rs` and imports `ApiDoc` from the api crate's lib.

### Ingestion pipeline

```bash
cd server
cargo run -p ingestion -- download                    # Fetch Parquet from HuggingFace
cargo run -p ingestion -- vocabulary                  # Pass 1: build vocabulary
cargo run -p ingestion -- backfill                    # Pass 2: insert to ClickHouse
cargo run -p ingestion -- status                      # Check progress
# Use --start 2024-01 --end 2024-03 to process a subset
```

## Gotchas

- **Endpoint naming affects SDK hook names.** Kubb derives hook names from the OpenAPI `operationId`, which utoipa derives from the Rust function name. We renamed `/query` → `/ngram` and `fn query()` → `fn ngram()` because `useQuery` conflicted with TanStack Query's `useQuery`. If adding endpoints, avoid names that collide with library exports.

- **Use `time` crate everywhere, never `chrono`.** The `clickhouse` crate's serde helpers use `time::Date`. Using `chrono` creates conversion friction. This is enforced in RFC-004.

- **Kubb client adapter contract.** The generated hooks expect `client/src/lib/client.ts` to: (1) default-export an async function accepting `RequestConfig` and returning `{data: T}`, (2) export types `RequestConfig`, `ResponseErrorConfig`, and `Client`. The function must accept 3 type params `<TData, TError, TBody>` even though only `TData` is used.

- **@mantine/dates version must match @mantine/core.** They are co-versioned (both 7.x or both 8.x). A mismatch causes runtime errors.

- **API returns single phrase per request.** The frontend makes parallel requests (one per phrase) for independent caching. This was an intentional design choice over the multi-phrase approach — see RFC-007-optimizations §7 for rationale. Don't revert to a multi-phrase API.

- **`server/openapi.json` is a generated artifact.** Don't edit it manually. Regenerate with `cargo run -p api --bin generate_openapi`.

## Key Concepts

- **Tokenizer versioned** — all data tagged with version; increment on ANY rule change
- **Daily base granularity** — stored daily, aggregated to week/month/year at query time
- **Pre-aggregated counts** — no raw text stored, only n-gram counts per day
- **Pruning thresholds** — bigrams need 20+ global occurrences, trigrams need 10+

## Crate Responsibilities

| Crate | Purpose |
|-------|---------|
| `tokenizer` | Tokenization rules (RFC-001), n-gram counting/pruning (RFC-002) |
| `hn-clickhouse` | Schema types, insert/query functions (RFC-003) |
| `api` | HTTP endpoints, OpenAPI spec (RFC-005) |
| `ingestion` | HuggingFace → tokenize → ClickHouse pipeline (RFC-004) |

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
| Detailed specs | `docs/RFC-*.md` |
| Optimization decisions | `docs/RFC-007-optimizations.md` |
