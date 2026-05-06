set positional-arguments
set dotenv-load

secrets_dir := env("SECRETS_DIR", "/srv/hngram/secrets")

# List available commands
list:
  just --list

# One-time setup: install client deps and generate the SDK from openapi.json
setup:
  cd client && bun install && bun run gen

# Start dev environment: ClickHouse (docker) + API + client (process-compose)
dev:
  docker compose up -d
  process-compose up

# Start only the ClickHouse dev container (run `just up` in server/ and client/ separately)
dev-db:
  docker compose up -d

# Stop the ClickHouse dev container
dev-stop:
  docker compose down

# Run docker compose against the prod yml
prod *args="up --watch":
  docker compose -f docker-compose.prod.yml "$@"

# Generate ClickHouse password and write to secrets directory
init-secrets:
  SECRETS_DIR={{secrets_dir}} ./scripts/init-secrets.sh
