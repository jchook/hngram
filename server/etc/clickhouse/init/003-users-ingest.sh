#!/bin/bash
set -e

SECRET_FILE="/run/secrets/clickhouse_ingest_password"

# Skip in dev (no secret file mounted)
if [ ! -f "$SECRET_FILE" ]; then
    echo "No secret file at $SECRET_FILE, skipping hngram_ingest user creation"
    exit 0
fi

HASH=$(tr -d '\n' < "$SECRET_FILE" | sha256sum | cut -d' ' -f1)

clickhouse client -n <<-EOSQL
    CREATE USER IF NOT EXISTS hngram_ingest
        IDENTIFIED WITH sha256_hash BY '${HASH}'
        DEFAULT DATABASE hn_ngram;
    GRANT SELECT, INSERT, ALTER, CREATE, DROP, TRUNCATE ON hn_ngram.* TO hngram_ingest;
EOSQL
