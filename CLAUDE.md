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
cd client && bun run generate         # Regenerate SDK from openapi.json
cd server && cargo run -p ingestion   # Run data pipeline
cd server && cargo test               # Run all tests
```

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
| N-gram counting | `server/crates/tokenizer/src/counter.rs` |
| ClickHouse types/queries | `server/crates/clickhouse/src/lib.rs` |
| DB schema | `server/etc/clickhouse/init/001-schema.sql` |
| API routes | `server/crates/api/src/main.rs` |
| Frontend | `client/src/` |
| Detailed specs | `docs/RFC-*.md` |
