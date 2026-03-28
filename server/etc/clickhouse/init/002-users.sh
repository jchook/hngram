#!/bin/bash
set -e

SECRET_FILE="/run/secrets/clickhouse_password"

# Skip in dev (no secret file mounted)
if [ ! -f "$SECRET_FILE" ]; then
    echo "No secret file at $SECRET_FILE, skipping hngram user creation"
    exit 0
fi

# Hash the password so the SQL only ever contains a hex string
HASH=$(tr -d '\n' < "$SECRET_FILE" | sha256sum | cut -d' ' -f1)

clickhouse client -n <<-EOSQL
    CREATE USER IF NOT EXISTS hngram
        IDENTIFIED WITH sha256_hash BY '${HASH}'
        DEFAULT DATABASE hn_ngram
        SETTINGS readonly = 1;
    GRANT SELECT ON hn_ngram.* TO hngram;
EOSQL
