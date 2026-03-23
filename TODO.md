# HN N-gram Viewer — TODO

## Next: End-to-end validation

- [ ] `docker compose up -d` — start API + ClickHouse
- [ ] Download a small data slice: `cargo run -p ingestion -- download --start 2024-01 --end 2024-01`
- [ ] Build vocabulary: `cargo run -p ingestion -- vocabulary --start 2024-01 --end 2024-01`
- [ ] Backfill: `cargo run -p ingestion -- backfill --start 2024-01 --end 2024-01`
- [ ] `bun run dev` — verify chart renders with real data
- [ ] Fix whatever breaks

## Deferred

- [ ] Caddy rate limiting (120 req/min per IP)
- [ ] Methodology/about modal
- [ ] Full historical backfill
