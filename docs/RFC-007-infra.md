Yes — with your plan of doing the heavy historical build locally and only shipping the **final ClickHouse data + app images** to the server, this should fit comfortably on a **single small VPS** for v1. The cheapest realistic tier is usually **4 GB RAM**, not 1–2 GB, because ClickHouse plus the API plus the frontend proxy will be happier with more headroom. On current public pricing, that puts you around **$12/month on Linode 2 GB, $24/month on Linode 4 GB**, **$4+/month on DigitalOcean entry Droplets with higher tiers available**, and roughly **€4.51/month for Hetzner CX22 (4 GB RAM, 2 vCPU, 40 GB disk)** after Hetzner’s 2026 price adjustment. ([Akamai][1])

The main caveat is **disk**, not CPU. Your serving box does not need to ingest the raw HN corpus, but it does need enough storage for:

* ClickHouse data files
* Docker images/volumes
* logs
* backups or snapshots if you keep them locally

A 40–50 GB disk is the floor; **80 GB is safer** if you want breathing room. Hetzner’s CX22 comes with **40 GB**, CX32 with **80 GB**; Linode’s 2 GB plan includes **50 GB**, and its 4 GB plan includes **80 GB**. ([Hetzner][2])

Below is the RFC in the same agent-oriented format.

---

# RFC-007 (Agent-Oriented)

## Infrastructure, Deployment, and Cost Model

## 0. Scope

Define:

* deployment topology
* host sizing
* Docker Compose layout
* persistence model
* backup/recovery approach
* cost target and expected resource envelope

Assumes:

* historical processing/backfill is performed locally on a separate powerful machine
* production server is serving-only, plus light incremental updates if enabled later
* deployment target is a **single VPS in one VPC/private network context** using Docker Compose

---

# 1. Deployment Model

## Spec (mandatory)

Production deployment is a **single VPS** running Docker Compose.

Services:

* `reverse-proxy`
* `frontend`
* `api`
* `clickhouse`

Optional later:

* `incremental-worker`
* `backup-worker`

No Kubernetes in v1.

---

## Rationale

* app is simple
* traffic profile is low to moderate
* operational simplicity is more important than elasticity
* local machine handles historical compute

---

## Flexibility

Agent may collapse `frontend` into static assets served by `reverse-proxy` if that materially simplifies deployment.

Agent may omit a separate reverse proxy only if TLS and static asset serving are otherwise handled cleanly.

---

## Non-goals

* multi-node cluster
* autoscaling
* managed service dependencies required for correctness

---

# 2. Hosting Assumption

## Spec (mandatory)

Target budget for serving-only deployment:

* preferred monthly cost: **$5–20**
* acceptable practical target: **4 GB RAM class VPS**
* recommended starting point: **2 vCPU, 4 GB RAM, 40–80 GB SSD/NVMe**

---

## Rationale

This app serves:

* one frontend
* one lightweight API
* one ClickHouse instance with precomputed aggregates

The production server is **not** responsible for full historical ingestion.

Current low-cost VPS pricing shows that:

* Hetzner CX22 provides **2 vCPU / 4 GB / 40 GB** at about **€4.51/month**
* Hetzner CX32 provides **4 vCPU / 8 GB / 80 GB** at about **€8.09/month**
* Linode shared plans are **$12 for 2 GB / 50 GB** and **$24 for 4 GB / 80 GB**
* DigitalOcean entry compute starts at **$4/month**, with larger droplets available above that. ([Hetzner][2])

---

## Flexibility

Agent may recommend a slightly larger machine if:

* ClickHouse dataset size materially exceeds expectations
* backup retention is stored locally
* expected concurrency is higher than hobby-scale

Preferred starting recommendation:

* **Hetzner CX22 / CX32**, or equivalent
* if avoiding Hetzner, then a **4 GB RAM VPS** from another provider

---

# 3. Single-Host Topology

## Spec (mandatory)

All services run on one host via Docker Compose.

Network model:

* one public entrypoint
* private internal Docker network for service-to-service communication

Publicly exposed ports:

* `80`
* `443`

Internal-only services:

* `api`
* `clickhouse`

`clickhouse` must not be public on the internet in v1.

---

## Rationale

* reduces attack surface
* simplifies firewalling
* reverse proxy is the only public ingress

---

## Flexibility

Agent may expose ClickHouse temporarily for administrative access only if:

* IP-restricted
* authenticated
* disabled by default

---

# 4. Compose Services

## 4.1 `reverse-proxy`

## Spec

Use a simple reverse proxy / web server.

Responsibilities:

* TLS termination
* static asset serving (if frontend is built static)
* proxy `/api/*` to backend
* optionally serve `/openapi.json`

Possible choices:

* Caddy
* Nginx
* Traefik

Preferred for simplicity:

* **Caddy**

---

## Rationale

Caddy gives very low-friction TLS and simple config for small deployments.

---

## Flexibility

Agent may choose Nginx if team preference or deployment familiarity makes it easier.

---

## 4.2 `frontend`

## Spec

Frontend is a built React app.

Preferred deployment:

* build in CI or locally
* ship static assets
* serve via reverse proxy

Alternative:

* dedicated lightweight container that serves built assets

No Node dev server in production.

---

## Rationale

Static hosting is simpler and smaller.

---

## 4.3 `api`

## Spec

Rust API container.

Responsibilities:

* validate and normalize requests
* query ClickHouse
* return JSON responses
* expose OpenAPI spec

No background historical ingestion in this service.

---

## Rationale

Keeps API stateless except for DB access.

---

## 4.4 `clickhouse`

## Spec

Single ClickHouse container with persistent volume.

Responsibilities:

* store `ngram_counts`
* store `bucket_totals`
* answer read queries

No raw comment corpus stored in production DB unless explicitly added later.

---

## Rationale

Serving DB only. Historical data prep is already done offline.

---

## 4.5 Optional `incremental-worker`

## Spec

Disabled by default in initial deployment.

If enabled later:

* processes incremental data only
* updates current-day buckets
* runs on schedule or manually

---

## Rationale

This can wait until after serving stack is stable.

---

# 5. Persistent Storage

## Spec (mandatory)

Use Docker volumes or bind mounts for:

* ClickHouse data directory
* ClickHouse logs
* reverse proxy TLS state/config if applicable
* optional app logs

ClickHouse data must persist across container restarts and host reboots.

---

## Rationale

Database durability is mandatory.

---

## Flexibility

Agent may choose bind mounts over named volumes if easier for backup/inspection.

Preferred:

* bind mounts on host filesystem for easier transfer and backup

---

# 6. Initial Data Load Strategy

## Spec (mandatory)

Historical ingestion happens **off-server** on local machine.

Deployment flow:

1. run historical build locally
2. produce final ClickHouse-ready dataset
3. load into local ClickHouse
4. export/transfer production dataset or ClickHouse data
5. restore/import onto VPS
6. start serving stack

Preferred methods:

* ClickHouse native export/import
* table-level backup/restore
* rsync/tar of data directory only when ClickHouse is stopped and version-compatible

---

## Rationale

Avoids expensive and slow cloud-side backfill.

---

## Flexibility

Agent may recommend one of two concrete workflows:

### Preferred workflow

* import using ClickHouse SQL/native tools into a clean production DB

### Allowed workflow

* copy ClickHouse data directory if:

  * ClickHouse versions match exactly
  * server is stopped during copy
  * restore process is documented

---

## Non-goals

* production server doing first full rebuild
* production server downloading raw HN corpus for historical backfill

---

# 7. Resource Envelope

## Spec (mandatory)

Initial production assumptions:

* low to moderate traffic
* mostly read-heavy workload
* pre-aggregated data
* small number of phrases per request (less than or equal to 10)
* one ClickHouse instance
* one Rust API instance

Recommended minimum production target:

* **2 vCPU**
* **4 GB RAM**
* **40 GB disk minimum**
* **80 GB preferred**

---

## Rationale

This is enough for:

* ClickHouse serving precomputed series
* API query translation
* frontend/static serving

Disk is likely tighter than CPU.

Provider examples:

* Hetzner CX22: **2 vCPU, 4 GB RAM, 40 GB**
* Hetzner CX32: **4 vCPU, 8 GB RAM, 80 GB**
* Linode 2 GB: **50 GB disk**
* Linode 4 GB: **80 GB disk**. ([Hetzner][2])

---

## Flexibility

Agent may recommend:

* 8 GB RAM if dataset size or query concurrency grows
* external object storage for backups if local disk gets tight

---

# 8. Cost Model

## Spec (mandatory)

Target production cost should remain near VPS cost only.

Expected recurring cost components:

* VPS monthly cost
* optional snapshots/backups
* optional domain name
* optional object storage for backups

Core app should run on one VPS without requiring managed DB/services.

---

## Rationale

The project should stay cheap because compute-heavy work is offloaded to the local machine.

---

## Estimated Server Cost Bands

### Lowest practical

* Hetzner CX22 class: ~**€4.51/month** for 4 GB / 40 GB. ([Hetzner][2])

### Safer small-production tier

* Hetzner CX32 class: ~**€8.09/month** for 8 GB / 80 GB. ([Hetzner][2])

### Non-Hetzner comparable range

* Linode: **$12/month for 2 GB**, **$24/month for 4 GB**
* DigitalOcean: entry VPS starts at **$4/month**, but practical production tiers for this app may be above that depending on RAM/disk needs. ([Akamai][1])

---

## Recommendation

Most likely best value:

* **Hetzner CX22** if dataset is small enough and you want minimum cost
* **Hetzner CX32** if you want comfortable margin

If using US-first providers:

* choose a **4 GB RAM VPS**
* expect roughly **$12–24/month**

---

# 9. Networking and Security

## Spec (mandatory)

Host firewall must allow only:

* 22/tcp (SSH) from restricted IPs if possible
* 80/tcp
* 443/tcp

Do not expose:

* ClickHouse native/http ports publicly
* internal API port publicly

Secrets must be supplied via:

* environment files not committed to VCS
* Docker secrets if convenient

TLS required in production.

---

## Rationale

Simple public surface area reduces risk.

---

## Flexibility

Agent may add:

* fail2ban
* Tailscale/WireGuard for private admin access
* Cloudflare in front of the host

These are optional.

---

# 10. Backups and Recovery

## Spec (mandatory)

At minimum, back up:

* ClickHouse data or logical exports
* deployment config
* Compose files
* reverse proxy config
* environment templates (without secrets)

Preferred backup target:

* off-host storage

Backup frequency:

* after each production data refresh
* plus periodic scheduled backup if incremental updates are enabled

---

## Rationale

Single-host deployments have a single major failure domain.

---

## Flexibility

Agent may implement:

* rsync to another machine
* object storage uploads
* provider snapshots

Preferred simple approach:

* periodic compressed logical export or snapshot to object storage

---

# 11. Observability

## Spec (mandatory)

Keep observability lightweight.

Required:

* container logs
* health checks
* basic uptime monitoring

Recommended:

* simple metrics endpoint for API
* ClickHouse health check query
* reverse proxy access logs

No full observability stack required in v1.

---

## Rationale

Avoid running Prometheus/Grafana/Loki unless there is a demonstrated need.

---

## Flexibility

Agent may add lightweight external uptime monitoring.

---

# 12. Deployment Workflow

## Spec (mandatory)

Deployment should be simple and reproducible.

Recommended workflow:

1. build backend image
2. build frontend static assets/image
3. push/copy artifacts to VPS
4. `docker compose pull` or copy images
5. `docker compose up -d`
6. run smoke checks
7. verify `/openapi.json`, frontend load, and one query end-to-end

---

## Rationale

This is sufficient for a single-host app.

---

## Flexibility

Agent may use:

* GitHub Actions
* local build + rsync
* private registry

No full CI/CD platform required.

---

# 13. Incremental Refresh Strategy

## Spec

Initial deployment may be **static** after initial import.

If later enabling freshness:

* run incremental ingestion off-box or on-box
* import only delta aggregates
* avoid raw corpus retention on VPS

Preferred early path:

* manual or scheduled refresh from local machine

---

## Rationale

Keeps production host simple and cheap.

---

## Flexibility

Agent may later introduce:

* a small worker on the VPS
* a local-machine push job
* CI-triggered refresh workflow

Preferred until needed:

* **manual/local refresh pipeline**

---

# 14. Prohibited Complexity

## Spec (mandatory)

Do NOT implement in v1:

* Kubernetes
* service mesh
* managed queue
* managed cache
* multi-node ClickHouse cluster
* cloud-side historical reprocessing
* production raw HN corpus lake
* separate staging environment unless actually needed

---

## Rationale

These add complexity without helping the core product.

---

# 15. Acceptance Criteria

Infrastructure is valid if:

* app runs on one VPS with Docker Compose
* frontend, API, and ClickHouse work together end-to-end
* ClickHouse data persists across restarts
* public ingress is only through reverse proxy
* monthly cost stays within target band for chosen provider
* initial historical load is performed locally, not on production
* recovery path is documented and tested at least once

---

# 16. Recommended Initial Deployment

## Spec (recommended)

Start with:

* **Hetzner CX22** (~€4.51/mo, 2 vCPU, 4GB RAM, 40GB disk) OR
* **Linode 4GB** ($24/mo, 2 vCPU, 4GB RAM, 80GB disk)
* Debian 12 host
* Docker Engine + Compose plugin
* Caddy (reverse proxy + TLS)
* Rust API container
* Static frontend served by Caddy
* ClickHouse container with bind-mounted data directory
* ClickHouse configured per RFC-003 §12 (low-memory settings)
* Manual backup to off-host storage

### Alternative minimum viable deployment

If budget is extremely tight and dataset is confirmed small:

* **Linode 2GB** ($12/mo, 1 vCPU, 2GB RAM, 50GB disk)
* Requires aggressive ClickHouse memory tuning
* Acceptable for initial testing, but upgrade path should be planned

---

## Rationale

4GB RAM provides comfortable headroom for:
* ClickHouse memory requirements
* Rust API
* Caddy
* Docker overhead
* Operating system

2GB is possible but requires careful tuning and leaves no margin for growth.

Each month of HN data is 10-100MB raw. Over 20 years, this is under 20GB. N-gram aggregates will be much smaller.

---

## ClickHouse Configuration (Required)

Apply low-memory settings from RFC-003 §12:

```xml
<max_server_memory_usage_to_ram_ratio>0.6</max_server_memory_usage_to_ram_ratio>
<max_memory_usage>500000000</max_memory_usage>
<mark_cache_size>134217728</mark_cache_size>
```

Without these settings, ClickHouse will attempt to use default memory allocations designed for large servers and will cause OOM or swap thrashing on a small VPS.

---

## Final Guidance for Agent

Prefer:

* one host
* one compose file
* bind-mounted persistent data
* local historical processing
* static frontend serving
* no extra infrastructure until proven necessary

Avoid:

* cloud-native complexity
* premature HA design
* managed dependencies that exceed app needs

The correct mental model is:
**“small static-ish analytical web app with a precomputed database”**, not “distributed data platform.”

---

## References

* [Akamai/Linode Cloud Pricing](https://www.akamai.com/cloud/pricing)
* [Hetzner Cloud Server Pricing](https://www.hetzner.com/cloud)

