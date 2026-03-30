# RFC-008 (Agent-Oriented)

## Local Development Workflow & Compose File Strategy

---

## 0. Scope

Define:

* how to run the full dev stack (API + frontend + dependencies) with a single command
* the split between `docker-compose.yml` (dev) and `docker-compose.prod.yml` (production)
* production deployment model: build and run on the VPS with a single command
* environment setup for native Rust builds against Dockerized dependencies
* automatic OpenAPI spec regeneration and client SDK updates on API type changes
* which crates participate in the dev loop and which do not

Assumes:

* developer has Rust toolchain, Bun, and Docker installed locally
* production deployment target is a single VPS (see RFC-007)
* `process-compose` is installed for dev orchestration

---

# 1. Core Principle

## Spec (mandatory)

**Two distinct modes with separate Compose files.**

| Mode | File | What runs in Docker | What runs natively |
|------|------|--------------------|--------------------|
| Development | `docker-compose.yml` | ClickHouse only | API (cargo), frontend (bun), SDK gen |
| Production | `docker-compose.prod.yml` | Everything: Caddy, API, ClickHouse | Nothing |

Development optimizes for **iteration speed** — native Rust incremental compilation, hot-reloading frontend, automatic SDK regeneration.

Production optimizes for **simplicity** — one command builds and deploys the entire stack on a VPS.

---

## Rationale

Rust's incremental compiler is fast — sub-second rebuilds for small changes in debug mode. Docker image rebuilds discard this cache entirely, turning every change into a full release build (minutes). Native `cargo` preserves the incremental compilation cache across runs.

For production, the opposite tradeoff applies: a single `docker compose` command that builds and runs everything eliminates the need for a separate build pipeline, CI system, or image registry. On a $12/mo VPS, this is the right level of complexity.

---

# 2. docker-compose.yml (Development)

## Spec (mandatory)

`docker-compose.yml` runs **only dependencies** — services that the developer doesn't need to iterate on.

```yaml
services:
  clickhouse:
    image: clickhouse/clickhouse-server:24.3-alpine
    ports:
      - "8123:8123"
    volumes:
      - clickhouse_data:/var/lib/clickhouse
      - ./server/etc/clickhouse/config.xml:/etc/clickhouse-server/config.d/custom.xml:ro
      - ./server/etc/clickhouse/users.xml:/etc/clickhouse-server/users.d/custom.xml:ro
      - ./server/etc/clickhouse/init:/docker-entrypoint-initdb.d:ro
    environment:
      - CLICKHOUSE_DB=hn_ngram
    healthcheck:
      test: ["CMD", "clickhouse-client", "--query", "SELECT 1"]
      interval: 5s
      timeout: 5s
      retries: 10

volumes:
  clickhouse_data:
```

Usage:

```bash
docker compose up -d    # starts ClickHouse only
```

The API container is **not present** in this file. It lives exclusively in `docker-compose.prod.yml`.

---

## Rationale

* no profiles, no overrides, no conditional logic — the dev file is just dependencies
* `docker compose up -d` does the right thing with no flags
* the file is small and obvious

---

# 3. docker-compose.prod.yml (Production)

## Spec (mandatory)

`docker-compose.prod.yml` defines the **complete production stack**. It builds all images from source and runs them together. This is the only file used on the VPS.

```yaml
services:
  caddy:
    build:
      context: .
      dockerfile: server/etc/caddy/Dockerfile
    ports:
      - "80:80"
      - "443:443"
    depends_on:
      api:
        condition: service_started

  api:
    build:
      context: ./server
      dockerfile: Dockerfile
    expose:
      - "3000"
    environment:
      - CLICKHOUSE_HOST=clickhouse
    env_file:
      - path: .env
        required: false
    depends_on:
      clickhouse:
        condition: service_healthy

  clickhouse:
    image: clickhouse/clickhouse-server:24.3-alpine
    volumes:
      - clickhouse_data:/var/lib/clickhouse
      - ./server/etc/clickhouse/config.xml:/etc/clickhouse-server/config.d/custom.xml:ro
      - ./server/etc/clickhouse/users.xml:/etc/clickhouse-server/users.d/custom.xml:ro
      - ./server/etc/clickhouse/init:/docker-entrypoint-initdb.d:ro
    environment:
      - CLICKHOUSE_DB=hn_ngram
    healthcheck:
      test: ["CMD", "clickhouse-client", "--query", "SELECT 1"]
      interval: 5s
      timeout: 5s
      retries: 10

volumes:
  clickhouse_data:
```

### Deployment

On the VPS, deployment is a single command:

```bash
docker compose -f docker-compose.prod.yml up --build -d
```

This:

1. Builds the Rust API from `server/Dockerfile` (multi-stage, release mode)
2. Builds the frontend and Caddy image from `server/etc/caddy/Dockerfile` (Bun build → static assets → Caddy)
3. Pulls the ClickHouse image (if not cached)
4. Starts all three services with correct dependency ordering

To update after a code change on the VPS:

```bash
git pull
docker compose -f docker-compose.prod.yml up --build -d
```

No image registry. No CI pipeline. No separate build step. The VPS builds from source.

---

## Rationale

**Why build on the VPS instead of pushing pre-built images?**

* eliminates the need for a container registry ($0 extra cost)
* eliminates CI/CD pipeline complexity
* `git pull && docker compose up --build` is the simplest possible deployment model
* a 4 GB RAM VPS can build the Rust binary — it's a small crate with few dependencies
* appropriate for a single-developer, single-server project at this scale

**Why not `ports: "3000:3000"` on the API?**

The API uses `expose` (internal only), not `ports` (host-bound). In production, Caddy is the only public ingress. ClickHouse and the API are on the internal Docker network only, per RFC-007 §3.

**Why a separate prod file instead of profiles?**

* the two files have genuinely different service sets (Caddy exists only in prod)
* no risk of accidentally running production config locally
* each file is self-contained and readable without understanding profile logic
* `docker compose -f docker-compose.prod.yml` is explicit about intent

---

## Flexibility

* Agent may add a container registry and pre-built images if VPS build times become a problem (e.g., if the Rust build exceeds available RAM). In that case, `docker compose pull && docker compose up -d` replaces the `--build` flow.
* Agent may add build-time secrets or args if needed for features like TLS certificate configuration.

---

# 4. Environment Variables

## Spec (mandatory)

| Variable | Dev value | Prod value | Set by |
|----------|-----------|------------|--------|
| `CLICKHOUSE_HOST` | `localhost` | `clickhouse` | `process-compose.yml` (dev) / `docker-compose.prod.yml` (prod) |
| `HN_NGRAM_DEV` | `1` | unset | `process-compose.yml` (dev only) |

In dev, ClickHouse is accessed via `localhost` (port 8123 is forwarded to the host). In production, services communicate over the internal Docker network using service names.

Recommended: add a `.env.development` file (committed) with local dev defaults:

```env
CLICKHOUSE_HOST=localhost
HN_NGRAM_DEV=1
```

`process-compose` will load these for all processes (see §7).

---

## Rationale

The split is clean: dev environment variables live in `process-compose.yml` or `.env.development`. Production environment variables live in `docker-compose.prod.yml` or `.env` (gitignored, for secrets).

---

## Flexibility

Agent may use any mechanism to set dev environment variables: `.env` file, `direnv`, shell aliases, or inline env vars.

---

# 5. Crate Scope in the Dev Loop

## Spec (mandatory)

Only the `api` crate runs as a long-lived process in the dev loop.

| Crate | In dev loop? | Reason |
|-------|-------------|--------|
| `api` | **Yes** — `cargo watch` rebuilds and restarts on change | Long-running HTTP server |
| `tokenizer` | **Indirectly** — recompiled when `api` rebuilds (it's a dependency) | Library crate |
| `hn-clickhouse` | **Indirectly** — recompiled when `api` rebuilds (it's a dependency) | Library crate |
| `ingest` | **No** — separate CLI, run manually | Batch tool, not a service; pulls heavy deps (parquet, arrow, reqwest, rayon) that the API doesn't need |

If actively developing the `ingest` crate, run a separate ad-hoc watcher:

```bash
cargo watch -w crates/ingest -x 'check -p ingest'
```

This is not part of the standard dev orchestration.

---

## Rationale

The `api` crate depends on `tokenizer` and `hn-clickhouse`, so changes to any of those three crates trigger an API rebuild automatically. The `ingest` crate is a batch CLI with a separate dependency tree — including it in the watch would slow down incremental builds by pulling in parquet/arrow compilation and would serve no purpose since ingest isn't running as a service during development.

---

# 6. Automatic OpenAPI + SDK Regeneration

## Spec (mandatory)

API type changes must automatically propagate to the frontend SDK without manual steps.

### Pipeline

```
API Rust types change
  → cargo-watch rebuilds and restarts the API server
  → API server writes openapi.json to disk on startup (dev mode only)
  → file watcher detects openapi.json change
  → kubb regenerates TypeScript SDK in client/src/gen/
  → rsbuild HMR picks up the new types
```

### Implementation

**Step 1: API writes spec on startup in dev mode.**

In `server/crates/api/src/main.rs`, when `HN_NGRAM_DEV` is set, write the OpenAPI spec to `server/openapi.json` at server startup before binding the listener:

```rust
if std::env::var("HN_NGRAM_DEV").is_ok() {
    let spec = ApiDoc::openapi().to_pretty_json().unwrap();
    std::fs::write("../openapi.json", &spec).ok();
    tracing::info!("wrote openapi.json (dev mode)");
}
```

This runs inside the already-built API binary, so there is no second `cargo` invocation and no target directory lock contention.

**Step 2: A file-watching process runs kubb when `openapi.json` changes.**

In `process-compose.yml`, a dedicated `sdk` process watches `server/openapi.json` and runs `bun run generate` in the client directory when it changes.

Preferred tool: `watchexec` — single binary, cross-platform, familiar to Rust developers.

```bash
watchexec -w server/openapi.json -- bash -c 'cd client && bun run generate'
```

---

## Rationale

**Why not a second `cargo watch` for `generate_openapi`?**

Two `cargo` processes building the same workspace simultaneously will contend for the target directory lock. The first blocks the second, creating unpredictable delays. By having the API server itself write the spec on startup, the spec is regenerated as a side effect of the normal build-and-run cycle with zero additional compilation.

**Why not regenerate on every API source change?**

Most API source changes (bug fixes, query logic, logging) don't affect the OpenAPI spec. Only struct/endpoint signature changes do. Writing the spec on startup means it only updates when the API actually rebuilds, and the file watcher (watchexec) is content-aware — if the spec hasn't changed, kubb won't re-run.

**Why a separate `sdk` process instead of chaining into cargo-watch?**

Separation of concerns. The `cargo watch` process manages Rust compilation. The `sdk` process manages TypeScript generation. They have different triggers, different runtimes, and different failure modes. Chaining them into one command makes error diagnosis harder.

---

## Flexibility

* Agent may use the `generate_openapi` binary instead of in-process spec writing if the lock contention issue is resolved (e.g., via separate target directories). The in-process approach is preferred because it is simpler.
* Agent may use any file-watching tool for the SDK regeneration step. `watchexec` is preferred but not mandatory. Alternatives: `inotifywait` (Linux), `fswatch` (macOS).
* The `HN_NGRAM_DEV` env var name is flexible — agent may use `DEV=1`, `ENVIRONMENT=development`, or any other convention consistent with the project.

---

# 7. Dev Orchestration with process-compose

## Spec (mandatory)

Use `process-compose` to start all dev processes with a single command.

### Installation

```bash
# macOS
brew install process-compose

# Linux (direct binary)
curl -L https://github.com/F1bonacc1/process-compose/releases/latest/download/process-compose_linux-amd64.tar.gz | tar xz
sudo mv process-compose /usr/local/bin/
```

### Configuration

`process-compose.yml` at project root:

```yaml
version: "0.5"

environment:
  - "CLICKHOUSE_HOST=localhost"
  - "HN_NGRAM_DEV=1"

processes:
  clickhouse:
    command: docker compose up clickhouse
    readiness_probe:
      exec:
        command: docker compose exec clickhouse clickhouse-client --query "SELECT 1"
      initial_delay_seconds: 3
      period_seconds: 5
    shutdown:
      command: docker compose stop clickhouse

  api:
    command: cargo watch -x 'run -p api'
    working_dir: ./server
    depends_on:
      clickhouse:
        condition: process_healthy

  sdk:
    command: watchexec -w server/openapi.json -- bash -c 'cd client && bun run generate'
    depends_on:
      api:
        condition: process_started

  client:
    command: bun run dev
    working_dir: ./client
    depends_on:
      sdk:
        condition: process_started
```

### Usage

```bash
# Start everything with TUI (view logs, restart processes, etc.)
process-compose up

# Start headless (multiplexed output, no TUI)
process-compose up -t
```

### Startup order

```
clickhouse (wait for healthy)
  → api (cargo watch — rebuilds on Rust changes, writes openapi.json on startup)
    → sdk (watchexec — regenerates TS SDK when openapi.json changes)
    → client (bun run dev — serves frontend with HMR)
```

---

## Rationale

* **Single command** — `process-compose up` replaces 3-4 terminal windows
* **Dependency ordering** — API waits for ClickHouse health check, SDK waiter waits for API
* **Built-in TUI** — view per-process logs, restart individual services, without tmux
* **No IDE dependency** — works from any terminal, compatible with neovim workflows
* **Declarative** — `process-compose.yml` is committed and versioned, unlike tmux scripts or VS Code tasks

---

## Flexibility

* Agent may omit the `sdk` process if the team prefers manual SDK regeneration
* Agent may adjust `readiness_probe` timings based on local machine performance
* Developer may run individual processes manually instead of using process-compose — the tool is additive, not required

---

# 8. Debug vs Release Builds

## Spec (mandatory)

Local development must use **debug builds** (the default `cargo build` / `cargo run` behavior).

Do NOT use `--release` for local dev iteration. Reserve release builds for:

* production Docker images (handled by `docker-compose.prod.yml`)
* performance benchmarking
* final testing before deployment

---

## Rationale

Debug builds compile significantly faster than release builds due to:

* no LTO
* no optimizations
* incremental compilation is more effective

The API's performance in debug mode is more than sufficient for local testing.

---

# 9. Docker Build Optimizations (CI/Production)

## Spec (recommended)

The existing `server/Dockerfile` dependency-caching strategy is adequate. If VPS build times become a concern, consider:

### BuildKit cache mounts

```dockerfile
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --package api
```

### cargo-chef

Replace the dummy-source trick with `cargo-chef`, which provides more robust dependency caching:

```dockerfile
FROM rust:1.77-alpine AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --package api
COPY . .
RUN cargo build --release --package api
```

---

## Rationale

* BuildKit cache mounts persist the cargo registry and target directory across builds on the VPS
* `cargo-chef` computes a dependency fingerprint rather than relying on dummy source files, which is less fragile
* These matter more now that the VPS builds from source on every deploy

---

## Flexibility

These are optional improvements. The current Dockerfile works. Apply if VPS build times are a bottleneck.

---

# 10. Required Tool Dependencies

## Spec (mandatory)

### Local development

| Tool | Purpose | Install |
|------|---------|---------|
| Rust toolchain | Build API natively | `rustup` |
| `cargo-watch` | Auto-rebuild on file changes | `cargo install cargo-watch` |
| Docker + Compose | Run ClickHouse | System package manager |
| Bun | Frontend dev server + SDK generation | `curl -fsSL https://bun.sh/install \| bash` |
| `process-compose` | Dev orchestration | `brew install process-compose` or direct binary |
| `watchexec` | File-watching for SDK regen | `cargo install watchexec-cli` or system package |

### Production VPS

| Tool | Purpose |
|------|---------|
| Docker + Compose | Build and run everything |
| Git | Pull source for builds |

No Rust toolchain, Bun, or other dev tools needed on the VPS — everything builds inside Docker containers.

---

## Flexibility

* `watchexec` may be replaced with `inotifywait` (Linux) or `fswatch` (macOS)
* `process-compose` is strongly recommended but not strictly required — developers can run processes manually in separate terminals

---

# 11. Production Deployment Workflow

## Spec (mandatory)

Deployment to the VPS follows this workflow:

```bash
# On the VPS
git pull
docker compose -f docker-compose.prod.yml up --build -d
```

That's it. No image registry, no CI pipeline, no separate build artifact.

### What this builds

* **Caddy image** (`server/etc/caddy/Dockerfile`): installs Bun, builds frontend static assets, copies them into a Caddy image with the Caddyfile
* **API image** (`server/Dockerfile`): multi-stage Rust build, produces a minimal Alpine image with the release binary
* **ClickHouse**: pulled from Docker Hub, not built

### Rollback

```bash
# If a deploy goes wrong
git checkout <previous-commit>
docker compose -f docker-compose.prod.yml up --build -d
```

### First-time setup on VPS

```bash
git clone <repo> /app
cd /app
cp .env.example .env        # add any secrets
docker compose -f docker-compose.prod.yml up --build -d
```

---

## Rationale

This is the simplest deployment model that works for a single-server, single-developer project. It has one moving part (git + docker compose) and zero infrastructure dependencies beyond the VPS itself.

The VPS has enough resources to build (4 GB RAM per RFC-007). Rust release builds are memory-intensive but the API crate is small. If builds fail due to memory, the first remediation is adding swap, not adding infrastructure.

---

## Flexibility

* Agent may add a container registry and switch to `docker compose pull` if VPS builds become untenable
* Agent may add a simple deploy script that wraps git pull + compose up if the two-command workflow needs guardrails (health checks, rollback on failure)

---

# 12. Acceptance Criteria

### Development

* `process-compose up` starts all dev services with a single command
* `docker compose up -d` starts only ClickHouse
* `cargo run -p api` connects to ClickHouse on localhost:8123
* code changes to `api`, `tokenizer`, or `hn-clickhouse` crates trigger automatic API rebuild within seconds
* API type changes automatically regenerate `server/openapi.json` and `client/src/gen/` without manual steps

### Production

* `docker compose -f docker-compose.prod.yml up --build -d` builds and runs the full stack
* Caddy serves the frontend on port 80/443 and proxies `/api/*` to the API
* API connects to ClickHouse over the internal Docker network
* ClickHouse is not exposed on the host (no `ports` directive in prod)
* ClickHouse data persists across restarts via named volume

### Both

* the two Compose files share no implicit dependencies (either works standalone)
* existing `server/Dockerfile` and `server/etc/caddy/Dockerfile` are unchanged

---

# 13. Non-goals

* a CI/CD pipeline or image registry
* a dev-specific Dockerfile
* running ClickHouse natively (Docker is fine for the database)
* hot module replacement or other frontend-style HMR for Rust
* remote development containers (VS Code devcontainers, GitHub Codespaces)
* including the `ingest` crate in the dev loop
* IDE-specific configuration (VS Code tasks, IntelliJ run configs)
