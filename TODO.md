# HN N-gram Viewer — TODO

## Next: End-to-end validation

- [x] `docker compose up -d` — start API + ClickHouse
- [x] Download a small data slice: `cargo run -p ingestion -- download --start 2024-01 --end 2024-01`
- [x] Build vocabulary: `cargo run -p ingestion -- vocabulary --start 2024-01 --end 2024-01`
- [x] Backfill: `cargo run -p ingestion -- backfill --start 2024-01 --end 2024-01`
- [x] `bun run dev` — verify chart renders with real data
- [x] Fix whatever breaks

## Deferred

- [ ] Caddy rate limiting (120 req/min per IP)
- [ ] Methodology/about modal
- [ ] Full historical backfill
