// k6 ramp test for hngram /api/ngram.
//
// Models the real frontend pattern: each VU picks 1–5 phrases and fires them
// in parallel via http.batch (the API returns a single phrase per request),
// then thinks 2–5s and repeats.
//
// Phrase pool comes from loadtest/phrases.tsv (generate via fetch_phrases.sh).
// Each request also picks a random YYYY-MM-DD date window — together this
// produces enough URL variety to exceed ClickHouse's mark/uncompressed caches
// and the OS page cache, so the test measures something closer to mixed
// real-world load rather than warm-cache throughput.
//
// Run:
//   k6 run loadtest/ngram.js
//   BASE_URL=https://hngram.com k6 run loadtest/ngram.js
//
// The Caddy rate limit (120/min per IP on /api/*) must be bypassed via
// loopback or LOADTEST_TOKEN — see loadtest/README.md.

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate } from 'k6/metrics';
import { SharedArray } from 'k6/data';
import { randomItem, randomIntBetween } from 'https://jslib.k6.io/k6-utils/1.4.0/index.js';

const BASE = __ENV.BASE_URL || 'https://hngram.com';
const TOKEN = __ENV.LOADTEST_TOKEN || '';

const DEFAULT_HEADERS = TOKEN
  ? { 'X-Loadtest-Token': TOKEN, 'X-Loadtest': 'k6' }
  : { 'X-Loadtest': 'k6' };

// Phrase pool — second column of phrases.tsv (count<TAB>phrase).
// SharedArray loads once and is read-only across VUs.
const phrases = new SharedArray('phrases', function () {
  return open('./phrases.tsv')
    .split('\n')
    .map((line) => line.split('\t')[1])
    .filter((p) => p && p.length > 0);
});

const GRANULARITIES = ['day', 'week', 'month', 'year'];

// Bounds of the corpus. End is the first of the current month, since data
// after that may be partial.
const DATA_START_MS = Date.UTC(2011, 0, 1);
const DATA_END_MS   = Date.UTC(2026, 4, 1);
const DAY_MS        = 86400000;

const rate429 = new Rate('rate_limited');

export const options = {
  stages: [
    { duration: '30s', target: 10 },
    { duration: '1m',  target: 50 },
    { duration: '2m',  target: 100 },
    { duration: '2m',  target: 200 },
    { duration: '1m',  target: 0 },
  ],
  thresholds: {
    http_req_failed:   ['rate<0.02'],
    http_req_duration: ['p(95)<2000', 'p(99)<5000'],
    rate_limited:      ['rate<0.001'],
  },
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

function fmtDate(ms) {
  return new Date(ms).toISOString().slice(0, 10);
}

// Random date window: length 1mo–10y, placed randomly within the corpus.
function randomDateRange() {
  const minLen = 30 * DAY_MS;
  const maxLen = 10 * 365 * DAY_MS;
  const len    = minLen + Math.random() * (maxLen - minLen);
  const startMax = DATA_END_MS - len;
  const start    = DATA_START_MS + Math.random() * (startMax - DATA_START_MS);
  return [fmtDate(start), fmtDate(start + len)];
}

export function setup() {
  if (phrases.length === 0) {
    throw new Error(
      'Phrase pool is empty. Run `bash loadtest/fetch_phrases.sh` on the prod host first ' +
      'to populate loadtest/phrases.tsv.'
    );
  }
  console.log(`Loaded ${phrases.length} phrases from phrases.tsv`);

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

  const reqs = [];
  for (let i = 0; i < phraseCount; i++) {
    const phrase = randomItem(phrases);
    const [start, end] = randomDateRange();
    const granularity = randomItem(GRANULARITIES);
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
      'status 200':       (r) => r.status === 200,
      'has body':         (r) => r.body && r.body.length > 0,
      'not rate-limited': (r) => r.status !== 429,
    });
  }

  sleep(randomIntBetween(2, 5));
}
