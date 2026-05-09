#!/usr/bin/env bash
# Fetch top phrases by recent activity from prod ClickHouse.
#
# Run this on the prod host, from the repo root (where docker-compose.prod.yml lives).
# Writes loadtest/phrases.tsv with `count<TAB>n<TAB>phrase` lines, sorted by
# recent count desc within each n. The corpus has n=1..5 (MAX_NGRAM_ORDER),
# so the LIMIT argument is split evenly across all five strata.
#
# Even stratification keeps the file inspectable as "top phrases per n";
# k6 reweights to a Zipf-style 1/n distribution at selection time so unigrams
# get queried ~5x more than 5-grams (matches realistic user behavior).
#
# Usage:
#   bash loadtest/fetch_phrases.sh           # 1000 phrases, last 90 days
#   bash loadtest/fetch_phrases.sh 10000     # 10k phrases, last 90 days
#   bash loadtest/fetch_phrases.sh 1000 30   # 1000 phrases, last 30 days
#
# The default ClickHouse user (no password) is restricted to loopback in
# users.prod.xml, and clickhouse-client inside the container connects via
# loopback — so no password is needed.
#
# Format is TSVRaw (no escape sequences). ngrams contain plain ASCII (the
# tokenizer normalizes curly quotes to ' and strips control chars), so the
# raw form is unambiguous and avoids `\'` / `\\` headaches in k6.

set -euo pipefail

LIMIT="${1:-1000}"
DAYS="${2:-90}"
# 5 strata for n=1..5 — see MAX_NGRAM_ORDER in clickhouse crate.
PER_N=$(( LIMIT / 5 ))

cd "$(dirname "$0")/.."

if [[ ! -f docker-compose.prod.yml ]]; then
  echo "error: must be run from the repo root (no docker-compose.prod.yml here)" >&2
  exit 1
fi

QUERY="SELECT sum(count) AS recent_count, n, ngram
FROM hn_ngram.ngram_counts
WHERE bucket >= today() - INTERVAL ${DAYS} DAY
  AND tokenizer_version = '1'
GROUP BY n, ngram
ORDER BY n ASC, recent_count DESC
LIMIT ${PER_N} BY n
FORMAT TSVRaw"

echo "Querying top ${PER_N} phrases per n (1..5) from last ${DAYS} days..."
docker compose -f docker-compose.prod.yml exec -T clickhouse \
  clickhouse-client --query "$QUERY" \
  > loadtest/phrases.tsv

count=$(wc -l < loadtest/phrases.tsv)
echo "Wrote ${count} phrases to loadtest/phrases.tsv"
echo
echo "Top 5:"
head -5 loadtest/phrases.tsv
