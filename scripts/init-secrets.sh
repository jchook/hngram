#!/usr/bin/env bash
set -euo pipefail

SECRETS_DIR="${SECRETS_DIR:-/srv/hngram/secrets}"

sudo mkdir -p "$SECRETS_DIR"
openssl rand -base64 32 | sudo tee "$SECRETS_DIR/clickhouse_password.txt" > /dev/null
sudo chown 1000:1000 "$SECRETS_DIR/clickhouse_password.txt"
sudo chmod 400 "$SECRETS_DIR/clickhouse_password.txt"

echo "Secret written to $SECRETS_DIR/clickhouse_password.txt"
