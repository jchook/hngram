# Load testing

`ngram.js` is a [k6](https://k6.io) ramp test against `/api/ngram` that mirrors
the frontend's parallel-per-phrase request pattern. It ramps 0 → 200 VUs over
~6.5 minutes and reports p95/p99 latency, RPS, and failure rate.

## Bypassing the rate limiter

Caddy enforces 120 requests/min per IP on `/api/*`. Two bypass paths are wired
into `server/etc/caddy/Caddyfile`, so the runner doesn't hit the cap:

1. **Loopback** — requests from `127.0.0.1` / `::1` are always exempt. Useful
   when running k6 from the prod VPS itself.
2. **Token header** — requests carrying `X-Loadtest-Token: <token>` are exempt
   when the token matches Caddy's `LOADTEST_TOKEN` env var. Useful when running
   from your laptop or a separate VPS.

Neither the IP nor the token lives in the repo. The Caddyfile has a sentinel
default for `LOADTEST_TOKEN` that no real client would send, so the bypass is
closed unless an operator deliberately sets the env var.

### Tradeoffs between the two paths

| Approach              | Setup                  | Network realism | CPU contention with API/ClickHouse |
|-----------------------|------------------------|-----------------|------------------------------------|
| k6 on prod VPS        | None — loopback exempt | None (loopback) | High — k6 steals cores             |
| k6 on 2nd VPS / laptop| Set token + redeploy   | Real            | None                               |

For finding a clean capacity ceiling, prefer a separate machine. For a
quick-and-dirty smoke test (or when the prod box has obviously spare cores),
loopback is fine and requires no secrets.

## One-time setup — token bypass

Generate a token (32 random bytes is plenty):

```bash
openssl rand -hex 32
```

Put it in the prod host's environment so docker-compose picks it up. The
project's `.env` file at the repo root works — docker-compose auto-loads it:

```bash
# /srv/hngram/.env  (or wherever you run docker compose from)
LOADTEST_TOKEN=<paste-the-hex-here>
```

The Caddyfile is baked into the image, so any change to it requires a rebuild.
The env var, however, is read at container start, so rotating the token
requires only a `restart`:

```bash
# After editing the Caddyfile (rate-limit logic):
docker compose -f docker-compose.prod.yml build caddy
docker compose -f docker-compose.prod.yml up -d --force-recreate caddy

# After only changing LOADTEST_TOKEN in .env:
docker compose -f docker-compose.prod.yml up -d --force-recreate caddy
```

The k6 `setup()` function fires 130 quick `/api/health` requests and aborts
the run if any return `429`, so a misconfigured bypass fails loudly instead
of silently testing Caddy's reject path.

## Running

```bash
# From a separate machine, against prod, with the token:
LOADTEST_TOKEN=<paste-the-hex-here> k6 run loadtest/ngram.js

# From the prod VPS itself (no token needed, loopback bypass kicks in):
BASE_URL=http://localhost:80 k6 run loadtest/ngram.js

# Against a local dev instance:
BASE_URL=http://localhost:8080 k6 run loadtest/ngram.js
```

## Stages

| Stage   | Duration | VUs       | Purpose                                |
|---------|----------|-----------|----------------------------------------|
| Warmup  | 30s      | 0 → 10    | Prime caches, surface obvious errors   |
| Low     | 1m       | 10 → 50   | Baseline                               |
| Mid     | 2m       | 50 → 100  | Look for early degradation             |
| High    | 2m       | 100 → 200 | Find the knee                          |
| Drain   | 1m       | 200 → 0   | Watch recovery                         |

Each VU loops: pick 1–5 phrases (50% hot / 35% medium / 15% cold) → fire them
in parallel via `http.batch` → think 2–5s. Effective load at 200 VUs is
roughly 100–200 RPS depending on response latency.

## Thresholds (cause k6 to exit nonzero)

- `http_req_failed` rate < 2%
- `http_req_duration` p95 < 2000 ms, p99 < 5000 ms
- `rate_limited` < 0.1% — any `429` indicates the bypass isn't active, not a
  real capacity signal

If the test fails on `rate_limited` first, fix the bypass before drawing
conclusions about capacity. If it fails on `http_req_duration` or
`http_req_failed`, that's the signal you're looking for — note the VU count
at the moment of degradation.

## Tuning

Edit `options.stages` in `ngram.js`:

- **Push higher**: extend the `200 → 300+` stage if 200 VUs barely budges p95.
- **Soak**: replace the ramp with a flat hold at ~70% of the observed knee
  for 10–30 min to surface leaks, GC pauses, or ClickHouse merge issues.
- **Cache-cold focus**: bias `pickPhrase()` toward `COLD` to stress
  ClickHouse rather than the API's response cache.

## Caveats

- **Single source.** Your local upload bandwidth or NAT may cap before the
  server does. Watch local `iftop`/`nethogs` to rule that out.
- **Loopback hides network cost.** Running from the prod box skips TLS
  termination, real RTT, and DNS — which means your "capacity" number
  reflects backend processing only. Useful, but not the same as user
  experience.
- **`remote_ip` matches the TCP peer.** If the stack ever sits behind a proxy
  (Cloudflare, ELB), update the loopback bypass to use `client_ip` with
  `trusted_proxies` configured, or it will silently stop matching.
- **Pointing at prod will degrade real users once you cross the knee.** Run
  during a low-traffic window.
