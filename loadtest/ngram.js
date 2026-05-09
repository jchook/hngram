// k6 ramp test for hngram /api/ngram.
//
// Models the real frontend pattern: each VU picks 1–5 phrases and fires them
// in parallel via http.batch (the API returns a single phrase per request),
// then thinks 2–5s and repeats.
//
// Run:
//   k6 run loadtest/ngram.js
//   BASE_URL=https://hngram.com k6 run loadtest/ngram.js
//
// The Caddy rate limit (120/min per IP on /api/*) must exempt this runner's
// IP — see loadtest/README.md.

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate } from 'k6/metrics';
import { randomItem, randomIntBetween } from 'https://jslib.k6.io/k6-utils/1.4.0/index.js';

const BASE = __ENV.BASE_URL || 'https://hngram.com';
const TOKEN = __ENV.LOADTEST_TOKEN || '';

// Default headers sent on every request. The token bypasses Caddy's rate
// limiter; without it, all requests are subject to the 120/min cap and
// setup() will abort the run.
const DEFAULT_HEADERS = TOKEN
  ? { 'X-Loadtest-Token': TOKEN, 'X-Loadtest': 'k6' }
  : { 'X-Loadtest': 'k6' };

// Hot phrases — high cache-hit probability after warmup.
const HOT = ['rust', 'apple', 'google', 'javascript', 'the internet'];
// Medium-popularity multi-word phrases.
const MED = ['kubernetes', 'react native', 'machine learning', 'rust programming', 'open source'];
// Lower-popularity phrases — likelier cache miss, hit ClickHouse.
const COLD = ['nixos', 'wasm runtime', 'fediverse', 'dependent types', 'rust borrow checker'];

const GRANULARITIES = ['month', 'year', 'week'];
const RANGES = [
  ['2011-01-01', '2026-05-01'], // full
  ['2020-01-01', '2026-05-01'], // 5y
  ['2023-01-01', '2026-05-01'], // 2y
];

const rate429 = new Rate('rate_limited');

export const options = {
  stages: [
    { duration: '30s', target: 10 },   // warm up
    { duration: '1m',  target: 50 },
    { duration: '2m',  target: 100 },
    { duration: '2m',  target: 200 },
    { duration: '1m',  target: 0 },    // ramp down
  ],
  thresholds: {
    http_req_failed:   ['rate<0.02'],
    http_req_duration: ['p(95)<2000', 'p(99)<5000'],
    rate_limited:      ['rate<0.001'], // any 429 = whitelist drift
  },
  // Surface a clean summary
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

function pickPhrase() {
  const r = Math.random();
  if (r < 0.5)  return randomItem(HOT);
  if (r < 0.85) return randomItem(MED);
  return randomItem(COLD);
}

// Sanity check before the real ramp: fire 130 quick health requests
// (above the 120/min limit) and abort if any return 429.
export function setup() {
  if (!TOKEN && !BASE.includes('localhost') && !BASE.includes('127.0.0.1')) {
    console.warn(
      'LOADTEST_TOKEN is not set and BASE_URL is not loopback. ' +
      'You will hit Caddy\'s 120/min rate limit and the run will fail setup.'
    );
  }
  console.log(`Verifying rate-limit bypass against ${BASE}/api/health ...`);
  let limited = 0;
  for (let i = 0; i < 130; i++) {
    const res = http.get(`${BASE}/api/health`, { headers: DEFAULT_HEADERS });
    if (res.status === 429) limited++;
  }
  if (limited > 0) {
    throw new Error(
      `Rate-limit bypass NOT active: ${limited}/130 returned 429. ` +
      `Either run from the prod host (loopback bypass) or set LOADTEST_TOKEN ` +
      `to the value configured in Caddy's env. See loadtest/README.md.`
    );
  }
  console.log('Bypass OK. Starting ramp.');
}

export default function () {
  const phraseCount = randomIntBetween(1, 5);
  const [start, end] = randomItem(RANGES);
  const granularity = randomItem(GRANULARITIES);

  const reqs = [];
  for (let i = 0; i < phraseCount; i++) {
    const phrase = pickPhrase();
    const url =
      `${BASE}/api/ngram?phrase=${encodeURIComponent(phrase)}` +
      `&start=${start}&end=${end}&granularity=${granularity}`;
    reqs.push({
      method: 'GET',
      url,
      params: {
        tags: { name: 'ngram' },
        headers: DEFAULT_HEADERS,
      },
    });
  }

  const responses = http.batch(reqs);
  for (const res of responses) {
    rate429.add(res.status === 429);
    check(res, {
      'status 200':   (r) => r.status === 200,
      'has body':     (r) => r.body && r.body.length > 0,
      'not rate-limited': (r) => r.status !== 429,
    });
  }

  sleep(randomIntBetween(2, 5));
}
