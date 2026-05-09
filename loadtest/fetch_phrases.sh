#!/usr/bin/env bash
# Fetch top phrases by recent activity from prod ClickHouse.
#
# Run this on the prod host, from the repo root (where docker-compose.prod.yml lives).
# Writes loadtest/phrases.tsv with `count<TAB>n<TAB>phrase` lines, sorted by
# recent count desc within each n.
#
# Pool sizes per n follow a 1/n (Zipf-style) distribution so the file itself
# matches realistic query distribution — unigrams ~5x more frequent than
# 5-grams. k6 then samples uniformly from the whole pool with no extra
# weighting.
#
# Weight ratios (×60 to keep them integers): 60, 30, 20, 15, 12 → sum 137.
# So per-n counts ≈ LIMIT × {60,30,20,15,12} / 137.
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

cd "$(dirname "$0")/.."

if [[ ! -f docker-compose.prod.yml ]]; then
  echo "error: must be run from the repo root (no docker-compose.prod.yml here)" >&2
  exit 1
fi

# Compute Zipf-weighted per-n limits.
N1=$(awk -v t="$LIMIT" 'BEGIN{printf "%.0f", t * 60 / 137}')
N2=$(awk -v t="$LIMIT" 'BEGIN{printf "%.0f", t * 30 / 137}')
N3=$(awk -v t="$LIMIT" 'BEGIN{printf "%.0f", t * 20 / 137}')
N4=$(awk -v t="$LIMIT" 'BEGIN{printf "%.0f", t * 15 / 137}')
N5=$(awk -v t="$LIMIT" 'BEGIN{printf "%.0f", t * 12 / 137}')

QUERY="SELECT recent_count, n, ngram FROM (
  SELECT
    sum(count) AS recent_count, n, ngram,
    row_number() OVER (PARTITION BY n ORDER BY sum(count) DESC) AS rn
  FROM hn_ngram.ngram_counts
  WHERE bucket >= today() - INTERVAL ${DAYS} DAY
    AND tokenizer_version = '1'
  GROUP BY n, ngram
)
WHERE (n = 1 AND rn <= ${N1})
   OR (n = 2 AND rn <= ${N2})
   OR (n = 3 AND rn <= ${N3})
   OR (n = 4 AND rn <= ${N4})
   OR (n = 5 AND rn <= ${N5})
ORDER BY n ASC, recent_count DESC
FORMAT TSVRaw"

echo "Querying Zipf-distributed pool: n=1:${N1}, 2:${N2}, 3:${N3}, 4:${N4}, 5:${N5} (last ${DAYS} days)..."
docker compose -f docker-compose.prod.yml exec -T clickhouse \
  clickhouse-client --query "$QUERY" \
  > loadtest/phrases.tsv

count=$(wc -l < loadtest/phrases.tsv)
echo "Wrote ${count} phrases to loadtest/phrases.tsv"
echo
echo "Top 5:"
head -5 loadtest/phrases.tsv
