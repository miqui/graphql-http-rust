// k6 test scenarios for the `example-server` GraphQL-over-HTTP toy service.
//
// Start the server first:
//   cargo run -p example-server        # listens on http://127.0.0.1:8080/graphql
//
// Run everything:
//   k6 run examples/k6/graphql-scenarios.js
//
// Run a single scenario:
//   k6 run --env SCENARIO=smoke examples/k6/graphql-scenarios.js
//   k6 run --env SCENARIO=query_load examples/k6/graphql-scenarios.js
//
// Point at a different host:
//   k6 run --env BASE_URL=http://localhost:3000 examples/k6/graphql-scenarios.js

import http from 'k6/http';
import { check, group, fail } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:8080';
const ENDPOINT = `${BASE_URL}/graphql`;
const GRAPHQL_RESPONSE_JSON = 'application/graphql-response+json';

// ---------------------------------------------------------------------------
// Custom metrics
// ---------------------------------------------------------------------------
const specConformance = new Rate('graphql_spec_conformance');
const graphqlErrors = new Counter('graphql_error_results');
const queryDuration = new Trend('graphql_query_duration', true);
const mutationDuration = new Trend('graphql_mutation_duration', true);

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------
const allScenarios = {
  // 1. Correctness pass over every documented status-code path. Runs first
  //    and fast so a broken server fails the run before load is applied.
  smoke: {
    executor: 'shared-iterations',
    exec: 'specConformanceSuite',
    vus: 1,
    iterations: 1,
    maxDuration: '30s',
    tags: { scenario_kind: 'smoke' },
  },

  // 2. Steady throughput of the happy-path query. Open model: arrival rate is
  //    held constant regardless of how slow the server gets, so latency
  //    degradation shows up in the metrics instead of being absorbed by VUs.
  query_load: {
    executor: 'constant-arrival-rate',
    exec: 'happyPathQuery',
    startTime: '35s',
    rate: 200,
    timeUnit: '1s',
    duration: '1m',
    preAllocatedVUs: 50,
    maxVUs: 200,
    tags: { scenario_kind: 'load' },
  },

  // 3. Realistic mix (queries, mutations, partial results, GET) ramping up to
  //    find the point where latency starts to bend.
  mixed_ramp: {
    executor: 'ramping-vus',
    exec: 'mixedTraffic',
    startTime: '1m40s',
    startVUs: 5,
    stages: [
      { duration: '30s', target: 50 },
      { duration: '1m', target: 50 },
      { duration: '30s', target: 150 },
      { duration: '30s', target: 0 },
    ],
    gracefulRampDown: '10s',
    tags: { scenario_kind: 'ramp' },
  },

  // 4. Short burst of malformed/rejected requests to confirm the error paths
  //    stay cheap and don't leak resources under pressure.
  error_path_spike: {
    executor: 'constant-vus',
    exec: 'errorPathTraffic',
    startTime: '4m',
    vus: 30,
    duration: '30s',
    tags: { scenario_kind: 'spike' },
  },
};

const selected = __ENV.SCENARIO;
if (selected && !allScenarios[selected]) {
  fail(`unknown SCENARIO "${selected}"; expected one of: ${Object.keys(allScenarios).join(', ')}`);
}

export const options = {
  scenarios: selected
    ? { [selected]: Object.assign({}, allScenarios[selected], { startTime: '0s' }) }
    : allScenarios,

  thresholds: {
    // Every spec assertion must hold, always.
    graphql_spec_conformance: ['rate==1.0'],

    // Latency budgets per scenario.
    'http_req_duration{scenario:query_load}': ['p(95)<150', 'p(99)<400'],
    'http_req_duration{scenario:mixed_ramp}': ['p(95)<300'],
    'http_req_duration{scenario:error_path_spike}': ['p(95)<100'],

    // Only unexpected transport/status failures count here — the negative
    // tests declare their own expected statuses via responseCallback.
    http_req_failed: ['rate<0.01'],

    graphql_query_duration: ['p(95)<150'],
    graphql_mutation_duration: ['p(95)<250'],
  },
};

// Global default for http_req_failed: treat 2xx/3xx as successful, which
// covers the non-standard 294 partial-success code. Negative tests override
// this per-request via their own `responseCallback`. This must be set in init
// context — it is not a valid `options` field.
http.setResponseCallback(http.expectedStatuses({ min: 200, max: 399 }));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function post(query, params = {}) {
  const {
    accept = GRAPHQL_RESPONSE_JSON,
    contentType = 'application/json',
    raw = null,
    ...rest
  } = params;

  const headers = {};
  if (contentType !== null) headers['Content-Type'] = contentType;
  if (accept !== null) headers['Accept'] = accept;

  const body = raw !== null ? raw : JSON.stringify({ query });
  return http.post(ENDPOINT, body, Object.assign({ headers }, rest));
}

function get(query, params = {}) {
  const { accept = GRAPHQL_RESPONSE_JSON, ...rest } = params;
  const headers = {};
  if (accept !== null) headers['Accept'] = accept;

  const url = query === null ? ENDPOINT : `${ENDPOINT}?query=${encodeURIComponent(query)}`;
  return http.get(url, Object.assign({ headers }, rest));
}

function contentTypeOf(res) {
  return (res.headers['Content-Type'] || '').toLowerCase();
}

function json(res) {
  try {
    return res.json();
  } catch (_) {
    return null;
  }
}

// Records checks against both the built-in check metric and the
// spec-conformance rate so a single threshold guards all assertions.
function specCheck(res, assertions, tags = {}) {
  const ok = check(res, assertions, tags);
  specConformance.add(ok, tags);
  return ok;
}

// Statuses we deliberately provoke, so they don't pollute http_req_failed.
const expect = (code) => http.expectedStatuses(code);

// ---------------------------------------------------------------------------
// Scenario: smoke — spec conformance across all documented paths
// ---------------------------------------------------------------------------
export function specConformanceSuite() {
  group('POST · query · 200 + data', () => {
    const res = post('{ hello }');
    specCheck(
      res,
      {
        'status is 200': (r) => r.status === 200,
        'content-type is graphql-response+json': (r) =>
          contentTypeOf(r).startsWith(GRAPHQL_RESPONSE_JSON),
        'data.hello is "world"': (r) => json(r)?.data?.hello === 'world',
        'no errors entry': (r) => json(r)?.errors === undefined,
      },
      { spec: 'sec-POST' }
    );
  });

  group('POST · partial success · 294', () => {
    const res = post('{ partial }');
    specCheck(
      res,
      {
        'status is 294': (r) => r.status === 294,
        'data entry present and null': (r) => json(r)?.data?.partial === null,
        'errors entry non-empty': (r) => (json(r)?.errors || []).length > 0,
      },
      { spec: 'sec-Partial-success' }
    );
  });

  group('POST · request error · 422', () => {
    const res = post('{ boom }', { responseCallback: expect(422) });
    specCheck(
      res,
      {
        'status is 422': (r) => r.status === 422,
        'no data entry': (r) => json(r)?.data === undefined,
        'errors entry present': (r) => (json(r)?.errors || []).length > 0,
      },
      { spec: 'sec-Status-Codes' }
    );
  });

  group('POST · malformed JSON body · 400', () => {
    const res = post(null, { raw: 'NONSENSE', responseCallback: expect(400) });
    specCheck(res, { 'status is 400': (r) => r.status === 400 }, { spec: 'sec-Status-Codes' });
  });

  group('POST · unparsable document · 400', () => {
    const res = post('{', { responseCallback: expect(400) });
    specCheck(res, { 'status is 400': (r) => r.status === 400 }, { spec: 'sec-Status-Codes' });
  });

  group('POST · missing Content-Type · 4xx', () => {
    const res = post('{ hello }', {
      contentType: null,
      responseCallback: http.expectedStatuses({ min: 400, max: 499 }),
    });
    specCheck(
      res,
      { 'status is client error': (r) => r.status >= 400 && r.status < 500 },
      { spec: 'sec-POST' }
    );
  });

  group('POST · unsupported Content-Type · 415', () => {
    const res = post('{ hello }', { contentType: 'text/plain', responseCallback: expect(415) });
    specCheck(res, { 'status is 415': (r) => r.status === 415 }, { spec: 'sec-POST' });
  });

  group('POST · unacceptable Accept · 406', () => {
    const res = post('{ hello }', {
      accept: 'application/xml, text/plain;q=0.5',
      responseCallback: expect(406),
    });
    specCheck(res, { 'status is 406': (r) => r.status === 406 }, { spec: 'sec-Body' });
  });

  group('POST · legacy client · application/json downgrade', () => {
    const res = post('{ hello }', { accept: 'application/json' });
    specCheck(
      res,
      {
        'status is 200': (r) => r.status === 200,
        'content-type downgraded to application/json': (r) =>
          contentTypeOf(r).startsWith('application/json'),
        'payload still contains data': (r) => json(r)?.data?.hello === 'world',
      },
      { spec: 'sec-Body' }
    );
  });

  group('POST · wildcard Accept · negotiates spec media type', () => {
    const res = post('{ hello }', { accept: '*/*' });
    specCheck(
      res,
      {
        'status is 200': (r) => r.status === 200,
        'content-type is graphql-response+json': (r) =>
          contentTypeOf(r).startsWith(GRAPHQL_RESPONSE_JSON),
      },
      { spec: 'sec-Accept' }
    );
  });

  group('POST · unknown properties ignored', () => {
    const res = post(null, {
      raw: JSON.stringify({ query: '{ hello }', somethingUnknown: 42, another: { x: 1 } }),
    });
    specCheck(
      res,
      {
        'status is 200': (r) => r.status === 200,
        'data.hello is "world"': (r) => json(r)?.data?.hello === 'world',
      },
      { spec: 'sec-JSON-Encoding' }
    );
  });

  group('POST · invalid variables shape · 422', () => {
    const res = post(null, {
      raw: JSON.stringify({ query: 'query Q ($i:Int!) { q(i: $i) }', variables: [7] }),
      responseCallback: expect(422),
    });
    specCheck(res, { 'status is 422': (r) => r.status === 422 }, { spec: 'sec-Status-Codes' });
  });

  group('GET · query · 200 + data', () => {
    const res = get('{ hello }');
    specCheck(
      res,
      {
        'status is 200': (r) => r.status === 200,
        'data.hello is "world"': (r) => json(r)?.data?.hello === 'world',
      },
      { spec: 'sec-GET' }
    );
  });

  group('GET · mutation rejected · 405 + Allow', () => {
    const res = get('mutation { createThing }', { responseCallback: expect(405) });
    specCheck(
      res,
      {
        'status is 405': (r) => r.status === 405,
        'Allow header present': (r) => !!r.headers['Allow'],
      },
      { spec: 'sec-GET' }
    );
  });

  group('GET · missing query param · 422', () => {
    const res = get(null, { responseCallback: expect(422) });
    specCheck(res, { 'status is 422': (r) => r.status === 422 }, { spec: 'sec-GET' });
  });

  group('GET · unsupported Accept · 406', () => {
    const res = get('{ hello }', { accept: 'text/plain', responseCallback: expect(406) });
    specCheck(res, { 'status is 406': (r) => r.status === 406 }, { spec: 'sec-Body' });
  });
}

// ---------------------------------------------------------------------------
// Scenario: query_load — steady happy-path throughput
// ---------------------------------------------------------------------------
export function happyPathQuery() {
  const res = post('{ hello }', { tags: { op: 'query_hello' } });
  queryDuration.add(res.timings.duration);
  specCheck(
    res,
    {
      'status is 200': (r) => r.status === 200,
      'data.hello is "world"': (r) => json(r)?.data?.hello === 'world',
    },
    { op: 'query_hello' }
  );
}

// ---------------------------------------------------------------------------
// Scenario: mixed_ramp — weighted mix of realistic traffic
// ---------------------------------------------------------------------------
export function mixedTraffic() {
  const roll = Math.random();

  if (roll < 0.6) {
    // 60% — POST query
    const res = post('{ hello }', { tags: { op: 'query_hello' } });
    queryDuration.add(res.timings.duration);
    specCheck(res, { 'query returns 200': (r) => r.status === 200 }, { op: 'query_hello' });
  } else if (roll < 0.8) {
    // 20% — POST mutation
    const res = post('mutation { createThing }', { tags: { op: 'mutation_create' } });
    mutationDuration.add(res.timings.duration);
    specCheck(
      res,
      {
        'mutation returns 200': (r) => r.status === 200,
        'createThing is true': (r) => json(r)?.data?.createThing === true,
      },
      { op: 'mutation_create' }
    );
  } else if (roll < 0.9) {
    // 10% — GET query (cacheable read path)
    const res = get('{ hello }', { tags: { op: 'get_hello' } });
    queryDuration.add(res.timings.duration);
    specCheck(res, { 'GET returns 200': (r) => r.status === 200 }, { op: 'get_hello' });
  } else {
    // 10% — partial success (data + errors)
    const res = post('{ partial }', { tags: { op: 'query_partial' } });
    queryDuration.add(res.timings.duration);
    graphqlErrors.add(1, { op: 'query_partial' });
    specCheck(res, { 'partial returns 294': (r) => r.status === 294 }, { op: 'query_partial' });
  }
}

// ---------------------------------------------------------------------------
// Scenario: error_path_spike — hammer the rejection paths
// ---------------------------------------------------------------------------
export function errorPathTraffic() {
  const cases = [
    () => post('{ boom }', { responseCallback: expect(422), tags: { op: 'err_request_error' } }),
    () => post(null, { raw: 'NONSENSE', responseCallback: expect(400), tags: { op: 'err_bad_json' } }),
    () => post('{', { responseCallback: expect(400), tags: { op: 'err_bad_document' } }),
    () =>
      post('{ hello }', {
        contentType: 'text/plain',
        responseCallback: expect(415),
        tags: { op: 'err_unsupported_type' },
      }),
    () =>
      get('mutation { createThing }', {
        responseCallback: expect(405),
        tags: { op: 'err_mutation_via_get' },
      }),
  ];

  const res = cases[Math.floor(Math.random() * cases.length)]();
  graphqlErrors.add(1);
  specCheck(res, {
    'error path returns 4xx': (r) => r.status >= 400 && r.status < 500,
    'error path returns an errors entry': (r) => (json(r)?.errors || []).length > 0,
  });
}

// ---------------------------------------------------------------------------
// Fail fast if the server isn't reachable before load starts.
// ---------------------------------------------------------------------------
export function setup() {
  const res = post('{ hello }', { timeout: '5s' });
  if (res.status !== 200) {
    fail(`server at ${ENDPOINT} is not healthy (status ${res.status}) — run \`cargo run -p example-server\` first`);
  }
  return { startedAt: new Date().toISOString() };
}
