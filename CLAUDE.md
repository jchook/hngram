# HN N-gram Viewer

Explore word and phrase trends in Hacker News comments over time (like Google Ngram Viewer).

## Codebase Layout

```
server/                     # Rust workspace
├── crates/
│   ├── api/                # HTTP API server
│   ├── ingestion/          # Data pipeline CLI
│   └── tokenizer/          # Shared tokenization lib
├── etc/
│   ├── caddy/              # Reverse proxy (prod)
│   └── clickhouse/         # DB config + schema
└── openapi.json            # API spec → generates client SDK

client/                     # Bun + React + TypeScript + Rsbuild
├── src/gen/                # Generated SDK (gitignored)
└── kubb.config.ts          # SDK generation config

docs/                       # RFCs with detailed specs
```

## Development

```bash
docker compose up -d              # Start API + ClickHouse
cd client && bun run dev          # Frontend dev server
cd client && bun run generate     # Regenerate SDK from openapi.json
cd server && cargo run -p ingestion   # Run data pipeline
```

## Architecture Notes

- **Tokenizer is versioned** — all data tagged with tokenizer version for reproducibility
- **Daily base granularity** — stored daily, aggregated to week/month/year at query time
- **Pre-aggregated counts** — no raw text stored, only n-gram counts per day
- **SDK generated from OpenAPI** — edit `server/openapi.json`, then `bun run generate`

## Where to Look

- API routes: `server/crates/api/src/`
- DB schema: `server/etc/clickhouse/init/`
- Tokenization rules: `server/crates/tokenizer/src/`
- Frontend components: `client/src/`
- Detailed specs: `docs/RFC-*.md`
