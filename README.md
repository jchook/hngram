![version](https://img.shields.io/badge/version-0.0.1-blue)
![status](https://img.shields.io/badge/status-alpha-orange)
![rust](https://img.shields.io/badge/built%20with-rust-dea584)

# HN N-gram Viewer

Explore word and phrase trends in Hacker News comments over time, inspired by Google's Ngram Viewer.

Type a word or phrase, get a chart showing how often it appears relative to all comments, from 2006 to present. Compare multiple phrases to see how the community's language shifts.

## How it works

HN comments are sourced from a [public HuggingFace dataset](https://huggingface.co/datasets/open-index/hacker-news), tokenized, and stored as pre-aggregated counts in ClickHouse. The API serves normalized frequencies which the frontend plots over time.

Very rare phrases are pruned from the dataset. This is a tool for exploring trends, not for checking whether something has ever been said on Hacker News.

## Architecture

- **Ingestion** (Rust) -- downloads Parquet data, tokenizes comments, counts n-grams, inserts into ClickHouse
- **API** (Rust/Axum) -- serves n-gram frequency queries
- **Frontend** (React/TypeScript) -- search UI with time series charts
- **ClickHouse** -- stores all the counts

## Development

See `CLAUDE.md` for setup instructions and codebase orientation, and `docs/design-decisions.md` for architectural rationale.

## License

MIT
