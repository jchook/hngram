#!/usr/bin/env bash
# Fetch top phrases by recent activity from prod ClickHouse.
#
# Run this on the prod host, from the repo root (where docker-compose.prod.yml lives).
# Writes loadtest/phrases.tsv with `count<TAB>phrase` lines, sorted by recent
# count desc per n, stratified as ~1/3 unigrams, 1/3 bigrams, 1/3 trigrams.
#
# Usage:
#   bash loadtest/fetch_phrases.sh           # 1000 phrases, last 90 days
#   bash loadtest/fetch_phrases.sh 10000     # 10k phrases, last 90 days
#   bash loadtest/fetch_phrases.sh 1000 30   # 1000 phrases, last 30 days
#
# The default ClickHouse user (no password) is restricted to loopback in
# users.prod.xml, and clickhouse-client inside the container connects via
# loopback — so no password is needed.

set -euo pipefail

LIMIT="${1:-1000}"
DAYS="${2:-90}"
PER_N=$(( LIMIT / 3 ))

cd "$(dirname "$0")/.."

if [[ ! -f docker-compose.prod.yml ]]; then
  echo "error: must be run from the repo root (no docker-compose.prod.yml here)" >&2
  exit 1
fi

QUERY="SELECT sum(count) AS recent_count, ngram
FROM hn_ngram.ngram_counts
WHERE bucket >= today() - INTERVAL ${DAYS} DAY
  AND tokenizer_version = '1'
GROUP BY n, ngram
ORDER BY n ASC, recent_count DESC
LIMIT ${PER_N} BY n
FORMAT TSV"

echo "Querying top ${PER_N} phrases per n (1,2,3) from last ${DAYS} days..."
docker compose -f docker-compose.prod.yml exec -T clickhouse \
  clickhouse-client --query "$QUERY" \
  > loadtest/phrases.tsv

count=$(wc -l < loadtest/phrases.tsv)
echo "Wrote ${count} phrases to loadtest/phrases.tsv"
echo
echo "Top 5:"
head -5 loadtest/phrases.tsv
