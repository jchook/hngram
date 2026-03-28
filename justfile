set positional-arguments
set dotenv-load

secrets_dir := env("SECRETS_DIR", "/srv/hngram/secrets")

prod *args="up --watch":
  docker compose -f docker-compose.prod.yml "$@"

# Generate ClickHouse password and write to secrets directory
init-secrets:
  SECRETS_DIR={{secrets_dir}} ./scripts/init-secrets.sh
