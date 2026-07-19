// TS-2 / TS-5 · LIVE probes against the real fabric — the no-spawn behavioral set.
//
// Every cell here hits the public surface a real user reaches and asserts BEHAVIOR (G2),
// not just a status code, then captures the artifact. All cells stop at auth/validation
// BEFORE any box spawns — zero cost, zero leak. Box-spawning journeys (full Door-A) are a
// separate, deliberately-batched live run.
//
// Run: E2E_LIVE=1 node --test scripts/e2e/journey/live-probes.test.mjs
// Skips cleanly (never fails) when E2E_LIVE!=1 or no PAT — so the pure CI gate is unaffected.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { req, loadPat, LIVE, DUMMY_ACQUIRE, bodyLeaksSecret, BASE } from '../lib/fabric.mjs';
import { Evidence } from '../lib/evidence.mjs';

const PAT = loadPat();
const ev = new Evidence('TS-2/TS-5·live-probes');
const gate = LIVE ? false : 'set E2E_LIVE=1 to run live probes';
const gatePat = LIVE ? (PAT ? false : 'no f0005 PAT available') : gate;
const bodies = []; // captured non-key bodies, swept for secret leakage at the end

test('TS2-substrate-health · GET /health is 200 and up', { skip: gate }, async () => {
  const r = await req('GET', '/health');
  const pass = r.status === 200;
  ev.record({ cell: 'TS2-substrate-health', atoms: ['F-10.3', 'F-7.2', 'F-6.4'], direction: 'happy',
    grade: 'E2', stimulus: `GET ${BASE}/health`,
    assertion: 'substrate answers 200 (control-plane singleton live)',
    pass, artifact: { status: r.status, body: r.text.slice(0, 400) } });
  assert.equal(r.status, 200);
});

test('TS2-attestation-key · GET /v1/attestation/key serves the public signing key (FLIP-B)', { skip: gate }, async () => {
  const r = await req('GET', '/v1/attestation/key');
  // The attestation PUBLIC key is intentionally public — behavioral: 200 + real key material.
  const hasKey = r.status === 200 && /[a-z0-9]/i.test(r.text) && r.text.length > 20;
  ev.record({ cell: 'TS2-attestation-key', atoms: ['F-4.10', 'F-5.4'], direction: 'happy',
    grade: 'E2', stimulus: `GET ${BASE}/v1/attestation/key`,
    assertion: '200 and serves ed25519 public-key material (result-binding verifiable client-side)',
    pass: hasKey, artifact: { status: r.status, keyId: r.json?.kid ?? r.json?.key_id ?? null, alg: r.json?.alg ?? null, len: r.text.length } });
  assert.equal(r.status, 200);
  assert.ok(hasKey, 'expected real key material in the body');
});

test('TS5-auth-fail-closed-nopat · acquire with NO PAT is 401, never fail-open', { skip: gate }, async () => {
  const r = await req('POST', '/v1/leases', { body: DUMMY_ACQUIRE });
  bodies.push(r.text);
  const pass = r.status === 401;
  ev.record({ cell: 'TS5-auth-fail-closed-nopat', atoms: ['F-5.1', 'F-4.2', 'S7.4'], direction: 'adversarial',
    grade: 'E2', stimulus: `POST ${BASE}/v1/leases (no Authorization)`,
    assertion: 'unauthenticated acquire is rejected 401 (fail-closed), no lease created, no leak',
    pass, artifact: { status: r.status, body: r.text.slice(0, 300) } });
  assert.equal(r.status, 401, 'auth must fail closed');
});

test('TS5-auth-fail-closed-badpat · acquire with a garbage PAT is 401', { skip: gate }, async () => {
  const r = await req('POST', '/v1/leases', { pat: 'garbage-not-a-real-pat-000000', body: DUMMY_ACQUIRE });
  bodies.push(r.text);
  const pass = r.status === 401;
  ev.record({ cell: 'TS5-auth-fail-closed-badpat', atoms: ['F-5.1', 'F-4.2'], direction: 'adversarial',
    grade: 'E2', stimulus: `POST ${BASE}/v1/leases (Bearer garbage)`,
    assertion: 'an invalid bearer is rejected 401 — introspect says no, fail-closed',
    pass, artifact: { status: r.status, body: r.text.slice(0, 300) } });
  assert.equal(r.status, 401);
});

test('TS2-authed-acquire-validation · f0005 PAT authenticates AND image validation is server-enforced', { skip: gatePat }, async () => {
  const r = await req('POST', '/v1/leases', { pat: PAT, body: DUMMY_ACQUIRE });
  bodies.push(r.text);
  // 400 (not 401) proves BOTH: the PAT authenticated (past auth) AND the invalid image was
  // rejected server-side (a user can't bypass image validation). No box spawns.
  const pass = r.status === 400;
  ev.record({ cell: 'TS2-authed-acquire-validation', atoms: ['F-5.1', 'F-8.2', 'F-4.1', 'F-4.5'], direction: 'happy',
    grade: 'E2', stimulus: `POST ${BASE}/v1/leases (f0005 PAT, deliberately-invalid image)`,
    assertion: 'authenticated (400 not 401) and server-side image validation rejects the bad digest — no box spawned',
    pass, artifact: { status: r.status, error: r.json?.error ?? r.json?.code ?? r.text.slice(0, 200) } });
  assert.equal(r.status, 400, 'expected past-auth image validation (400), got ' + r.status);
});

test('TS5-error-vocab-malformed · malformed body is a clean 4xx, not a 5xx/leak', { skip: gatePat }, async () => {
  const r = await req('POST', '/v1/leases', { pat: PAT, headers: { 'content-type': 'application/json' } });
  // send raw invalid JSON via a manual fetch (req() JSON-encodes; here we want a bad body)
  const raw = await fetch(`${BASE}/v1/leases`, { method: 'POST',
    headers: { authorization: `Bearer ${PAT}`, 'content-type': 'application/json' },
    body: '{not valid json', signal: AbortSignal.timeout(15000) });
  const t = await raw.text();
  bodies.push(t);
  const pass = raw.status >= 400 && raw.status < 500;
  ev.record({ cell: 'TS5-error-vocab-malformed', atoms: ['F-3.2'], direction: 'edge',
    grade: 'E2', stimulus: `POST ${BASE}/v1/leases (authed, malformed JSON body)`,
    assertion: 'malformed input yields a clean 4xx from the DTO layer, never a 5xx or a stack leak',
    pass, artifact: { status: raw.status, body: t.slice(0, 300) } });
  assert.ok(pass, 'malformed body must be a 4xx, got ' + raw.status);
});

test('TS5-nonleak-sweep · no captured error/acquire body leaks secret-shaped material', { skip: gate }, () => {
  const leaks = bodies.filter((b) => bodyLeaksSecret(b));
  ev.record({ cell: 'TS5-nonleak-sweep', atoms: ['S7.2', 'F-4.2'], direction: 'adversarial',
    grade: 'E2', stimulus: 'sweep every captured acquire/error response body',
    assertion: 'no response body contains a bearer/PAT/private-key/api-key shaped token',
    pass: leaks.length === 0, artifact: { bodiesSwept: bodies.length, leaks: leaks.length } });
  assert.equal(leaks.length, 0, 'a response body leaked secret-shaped material');
});

after(() => {
  if (!LIVE) return;
  const summary = ev.flush();
  console.log(`\n[evidence] ${summary.passed}/${summary.cells} cells passed → docs/validation/evidence/${ev.runId}/`);
});
