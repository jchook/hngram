-- HN N-gram schema (per RFC-003)

CREATE DATABASE IF NOT EXISTS hn_ngram;

-- N-gram counts table
-- Stores pre-aggregated daily counts for each n-gram
CREATE TABLE IF NOT EXISTS hn_ngram.ngram_counts
(
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    bucket Date,
    count UInt32
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tokenizer_version, n, ngram, bucket)
SETTINGS index_granularity = 8192;

-- Bucket totals table
-- Stores total n-gram counts per bucket for normalization denominators
CREATE TABLE IF NOT EXISTS hn_ngram.bucket_totals
(
    tokenizer_version LowCardinality(String),
    n UInt8,
    bucket Date,
    total_count UInt64
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tokenizer_version, n, bucket);

-- N-gram vocabulary table for admission tracking
-- Uses ReplacingMergeTree to handle re-ingestion without duplicates
-- (ClickHouse has no INSERT ... ON CONFLICT, so we rely on eventual dedup)
CREATE TABLE IF NOT EXISTS hn_ngram.ngram_vocabulary
(
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    global_count UInt64,
    admitted_at DateTime
)
ENGINE = ReplacingMergeTree(admitted_at)
ORDER BY (tokenizer_version, n, ngram);
