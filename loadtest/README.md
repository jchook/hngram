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

## Phrase pool

The script reads phrases from `loadtest/phrases.tsv` (gitignored — regenerate
when you want a fresh sample). `fetch_phrases.sh` queries prod ClickHouse for
the top phrases by recent activity, with **per-n pool sizes that follow a 1/n
(Zipf-style) distribution** — so the file itself reflects realistic query
shape and k6 just samples uniformly:

```bash
# On the prod host, from the repo root:
bash loadtest/fetch_phrases.sh             # 1000 phrases, last 90 days (default)
bash loadtest/fetch_phrases.sh 10000       # 10k phrases
bash loadtest/fetch_phrases.sh 1000 30     # 1000 phrases from the last 30 days
```

Each line of `phrases.tsv` is `count<TAB>n<TAB>phrase`. With `LIMIT=10000`
the resulting pool sizes are roughly:

| n      | Pool size  | Share of pool |
|--------|-----------:|--------------:|
| 1-gram | 4380       | 44%           |
| 2-gram | 2190       | 22%           |
| 3-gram | 1460       | 15%           |
| 4-gram | 1095       | 11%           |
| 5-gram | 875        |  9%           |

Within each n stratum, phrases are sorted by recent count desc and the top
N are kept — so selection is implicitly biased toward popular phrases.
Combined with a per-request random date window (1mo–10y, placed randomly
within 2011-01-01 to 2026-05-01), this produces enough URL variety to push
past ClickHouse's mark/uncompressed caches and the OS page cache while
still reflecting realistic user query patterns.

If you run k6 from your laptop (against `https://hngram.com`), `scp` the file
down first:

```bash
scp prod:/srv/hngram/loadtest/phrases.tsv loadtest/phrases.tsv
```

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

The script targets realistic active-user concurrency: 1 VU sleeps 10–30s
between iterations (avg 20s), which is roughly the cadence of a user
loading the chart, glancing at it, and tweaking a parameter. So 1 VU ≈
1 real active user.

Total run time is ~5 minutes if everything passes; thresholds with
`abortOnFail` kill the test on the first sustained breach (after a 60s
settle grace).

| Stage   | Duration | VUs        | Purpose                              |
|---------|----------|------------|--------------------------------------|
| Settle  | 60s      | 50 → 50    | Baseline; threshold eval starts here |
| Step 1  | 60s      | 50 → 150   | Near expected cold-cache knee        |
| Step 2  | 60s      | 150 → 300  |                                      |
| Step 3  | 60s      | 300 → 500  | Around expected warm-cache knee      |
| Step 4  | 60s      | 500 → 700  | Headroom past the knee               |
| Drain   | 15s      | → 0        | Brief drain                          |

`startVUs: 50` skips the slow ramp from zero. Each VU loops: pick 1–5
phrases uniformly from `phrases.tsv`, each with its own random date range
and granularity → fire them in parallel via `http.batch` → think 10–30s.

> **Why not shorter sleep?** Aggressive 0.5–1.5s think time amplifies one
> VU into ~20 real users of offered load. That's useful for finding raw
> backend ceiling but reports VU counts that don't translate to "concurrent
> users". Stick with realistic sleep when you want a number you can quote.

## Thresholds (abort the run on breach)

All three thresholds use `abortOnFail: true` — k6 stops the run the moment
they're sustained-breached, instead of continuing to hammer the system past
the knee. `delayAbortEval: 60s` means evaluation doesn't start until the
settle stage is over, so brief warmup spikes don't trip an abort.

- `http_req_failed` rate < 5%
- `http_req_duration` p95 < 5000 ms — 5s tolerates the cold-cache per-query
  cost (~4s avg observed) without flagging healthy operation as broken
- `rate_limited` < 0.1% — any `429` indicates the bypass isn't active, not a
  real capacity signal

If the test aborts on `rate_limited`, fix the bypass before drawing
conclusions about capacity. If it aborts on `http_req_duration` or
`http_req_failed`, that's the signal — the VU count at abort time is your
knee estimate.

## Tuning

Edit `options.stages` in `ngram.js`:

- **Push higher**: extend the `200 → 300+` stage if 200 VUs barely budges p95.
- **Soak**: replace the ramp with a flat hold at ~70% of the observed knee
  for 10–30 min to surface leaks, GC pauses, or ClickHouse merge issues.
- **Realistic Zipf bias**: replace `randomItem(phrases)` with weighted
  selection from the top of `phrases.tsv` (the file is sorted by recent
  count desc within each n). Uniform sampling is more cache-hostile;
  Zipf-weighted is closer to real traffic.
- **Larger pool**: re-run `fetch_phrases.sh 10000` for a 10k phrase pool —
  even more variety for finding cold-cache limits.

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
