![version](https://img.shields.io/badge/version-0.0.1-blue)
![status](https://img.shields.io/badge/status-alpha-orange)
![rust](https://img.shields.io/badge/built%20with-rust-dea584)

# HN N-gram Viewer

**Live demo: [hngram.com](https://hngram.com)**

Explore word and phrase trends in Hacker News comments over time, inspired by Google's Ngram Viewer.

Type a word or phrase, get a chart showing how often it appears relative to all comments, from 2006 to today. Compare multiple phrases side-by-side to see how the community's language has shifted — when "GPT" eclipsed "machine learning," when "Rust" took off, when "the cloud" peaked.

## Why I built this

I wanted to know when certain ideas entered or left the HN zeitgeist, and Google Ngrams stops at 2019 and doesn't cover the kind of jargon people actually argue about here. So I built one for HN.

It's a fun way to settle "is this hyped more than it used to be?" debates, and the resulting curves are sometimes surprisingly clean.

## How it works

HN comments come from a [public HuggingFace dataset](https://huggingface.co/datasets/open-index/hacker-news), get tokenized, and are stored as pre-aggregated daily n-gram counts in ClickHouse. The API serves normalized frequencies (count / total tokens that day) which the frontend plots over time.

A few design choices worth calling out:

- **No raw text is stored** — only counts per (n-gram, day). The whole HN corpus compresses to something a single VPS can serve.
- **Append-only data** — counts only grow. No reprocessing, no dedup, no migrations. Watermark-based incremental ingest.
- **Aggressive pruning** — very rare phrases are dropped at admission time. This is a tool for trends, not for proving something was ever said on HN.
- **Single-phrase API** — the frontend fires one request per phrase in parallel so each is independently cacheable by CDN/browser.

See [`docs/design-decisions.md`](docs/design-decisions.md) for the full rationale and [`docs/RFC-*.md`](docs/) for the detailed specs.

## Stack

- **Ingest** (Rust) — downloads Parquet, tokenizes, counts n-grams, writes to ClickHouse
- **API** (Rust + axum + utoipa) — generates its own OpenAPI spec, which generates the TS SDK
- **Frontend** (React + TypeScript + Mantine + ECharts, built with Rsbuild + Bun)
- **ClickHouse** — stores the counts
- **Caddy** — reverse proxy + automatic TLS in prod

## Running it locally

ClickHouse runs in Docker; everything else runs natively for fast iteration.

```bash
docker compose up -d              # ClickHouse
cd server && just up              # API
cd client && just up              # Frontend
```

Or `process-compose up` from the repo root to run the API and client together.

For ingest setup and the full development workflow, see [`CLAUDE.md`](CLAUDE.md).

## Feedback

If you find weird gaps, surprising trends, or bugs, open an issue — or just reply to the Show HN thread. Especially curious to hear what phrases people search for first.

## License

MIT
