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

-- Global n-gram counts for vocabulary admission decisions.
-- Tracks total corpus-wide count per (n, ngram) across all ingestion runs.
-- ReplacingMergeTree keeps the highest count on dedup.
CREATE TABLE IF NOT EXISTS hn_ngram.global_counts
(
    tokenizer_version LowCardinality(String),
    n UInt8,
    ngram String,
    count UInt64
)
ENGINE = ReplacingMergeTree(count)
ORDER BY (tokenizer_version, n, ngram);

-- Ingestion log table
-- Append-only audit trail of ingestion runs. Tracks watermark for incremental processing.
-- UUIDv7 is time-sortable, so ORDER BY id DESC LIMIT 1 gives the latest entry.
CREATE TABLE IF NOT EXISTS hn_ngram.ingestion_log
(
    id UUID DEFAULT generateUUIDv7(),
    tokenizer_version LowCardinality(String),
    command LowCardinality(String),
    last_ingested_ts Int64,
    comments_processed UInt64,
    ngram_counts_inserted UInt64,
    bucket_totals_inserted UInt64,
    vocabulary_inserted UInt64,
    start_month String,
    end_month String,
    duration_ms UInt64,
    created_at DateTime64(3, 'UTC') DEFAULT now64()
)
ENGINE = MergeTree()
ORDER BY id;
