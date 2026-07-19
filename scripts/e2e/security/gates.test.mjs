// TS-5 · security-adversarial — an attacker with a real user's reach probes every gate.
//
// Ground truth captured live 2026-07-19 before authoring (evidence, not theory). All cells
// are no-spawn (they die at a gate). Behavioral assertions: fail-closed, no route-existence
// oracle, no cross-tenant oracle, no secret leak.
//
// Run: E2E_LIVE=1 node --test scripts/e2e/security/gates.test.mjs
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { req, loadPat, LIVE, bodyLeaksSecret, BASE } from '../lib/fabric.mjs';
import { Evidence } from '../lib/evidence.mjs';

const PAT = loadPat();
const ev = new Evidence('TS-5·security-gates');
const gate = LIVE ? false : 'set E2E_LIVE=1 to run live probes';
const gatePat = LIVE ? (PAT ? false : 'no f0005 PAT available') : gate;
const bodies = [];

test('TS5-internal-gate-fail-closed · /internal rejects header-less callers with 401', { skip: gate }, async () => {
  const r = await req('GET', '/internal/v1/status');
  bodies.push(r.text);
  // Audit F5: NO positive control here (the internal-auth key is not available to this
  // harness), so this proves only that a header-less caller is rejected 401 — NOT that the
  // route is reachable-and-correct with the key. Claim scoped accordingly (no "gated" overclaim).
  ev.record({ cell: 'TS5-internal-gate-fail-closed', atoms: ['F-5.1', 'F-4.2', 'S7.4'], direction: 'adversarial',
    grade: 'E2', stimulus: `GET ${BASE}/internal/v1/status (no X-Corelink-Internal-Auth)`,
    assertion: 'a header-less caller to /internal is rejected 401 (fail-closed). GAP: no positive-control cell (with-key → 200) — the internal-auth key is not in this harness',
    pass: r.status === 401, artifact: { status: r.status, body: r.text.slice(0, 200), positiveControl: 'absent (key unavailable)' } });
  assert.equal(r.status, 401);
});

test('TS5-v1-namespace-auth-front · unknown and REAL /v1 routes give a byte-identical 401 (no oracle)', { skip: gate }, async () => {
  const fake = await req('GET', '/v1/this-route-does-not-exist');
  const real = await req('GET', '/v1/usage'); // a known-real route, unauthenticated
  bodies.push(fake.text, real.text);
  // Audit F12: indistinguishability requires comparing the fake route to a KNOWN-REAL route
  // unauthenticated. Both must be 401 with byte-identical bodies — only then is there truly
  // no route-existence oracle for the unauthenticated caller.
  const identical = fake.status === 401 && real.status === 401 && fake.text === real.text;
  ev.record({ cell: 'TS5-v1-namespace-auth-front', atoms: ['F-3.2', 'F-5.1', 'S7.4'], direction: 'adversarial',
    grade: 'E2', stimulus: `GET ${BASE}/v1/{unknown-route, usage} (no PAT)`,
    assertion: 'a fake route and a real route both return 401 with a byte-identical body — no route-existence oracle for the unauthenticated',
    pass: identical, artifact: { fakeStatus: fake.status, realStatus: real.status, bodiesIdentical: fake.text === real.text, body: fake.text.slice(0, 120) } });
  assert.equal(fake.status, 401);
  assert.equal(real.status, 401);
  assert.equal(fake.text, real.text, 'fake and real unauthenticated /v1 bodies must be identical');
});

test('TS5-cred-cred-bad-ticket · redeem with a garbage ticket is 401 invalid ticket', { skip: gate }, async () => {
  const r = await req('POST', '/v1/leases/00000000-0000-4000-8000-000000000000/cas-cred', { body: { ticket: 'garbage-not-a-real-ticket' } });
  bodies.push(r.text);
  const pass = r.status === 401 && /invalid ticket/i.test(r.text);
  ev.record({ cell: 'TS5-cred-cred-bad-ticket', atoms: ['F-5.9', 'S7.11'], direction: 'adversarial',
    grade: 'E2', stimulus: `POST ${BASE}/v1/leases/{id}/cas-cred (garbage ticket)`,
    assertion: 'the C2c redemption endpoint validates the ticket — a forged ticket is rejected 401 "invalid ticket", no CAS-PAT minted',
    pass, artifact: { status: r.status, body: r.text.slice(0, 200) } });
  assert.equal(r.status, 401);
  assert.match(r.text, /invalid ticket/i);
});

test('TS5-unknown-lease-404 · an unknown lease id is 404 under my PAT', { skip: gatePat }, async () => {
  const r = await req('GET', '/v1/leases/11111111-1111-4111-8111-111111111111', { pat: PAT });
  bodies.push(r.text);
  // Audit F2: this UUID exists in NO tenant, so 404 is trivially true — it does NOT prove
  // cross-tenant isolation (a fabric with zero isolation returns the same 404). The real
  // no-oracle proof needs a lease that ACTUALLY EXISTS under another tenant, read with this
  // PAT, asserting 404 (not 200/403) — that requires a spawned foreign lease and is deferred
  // to the box-spawn batch. Claim scoped honestly to "unknown id → 404".
  ev.record({ cell: 'TS5-unknown-lease-404', atoms: ['F-4.2', 'S7.4'], direction: 'adversarial',
    grade: 'E2', stimulus: `GET ${BASE}/v1/leases/{nonexistent-uuid} (f0005 PAT)`,
    assertion: 'an unknown lease id returns 404. GAP: true cross-tenant no-oracle needs a live FOREIGN lease (spawn batch) — not proven by a random uuid',
    pass: r.status === 404, artifact: { status: r.status, body: r.text.slice(0, 200), crossTenantOracle: 'not-yet-proven (needs live foreign lease)' } });
  assert.equal(r.status, 404);
});

test('TS5-gate-nonleak-sweep · no gate-rejection body leaks secret-shaped material (>= 5 bodies)', { skip: gate }, () => {
  const leaks = bodies.filter((b) => bodyLeaksSecret(b));
  // Audit F8: non-vacuous floor + the regex now includes the real corelink_ PAT shape.
  ev.record({ cell: 'TS5-gate-nonleak-sweep', atoms: ['S7.2', 'F-4.2'], direction: 'adversarial',
    grade: 'E2', stimulus: 'sweep every captured gate-rejection body (corelink_/pat_/bearer/key regex)',
    assertion: 'no rejection body contains a corelink_-PAT / bearer / private-key / api-key shaped token; >= 5 bodies swept (non-vacuous)',
    pass: leaks.length === 0 && bodies.length >= 5, artifact: { bodiesSwept: bodies.length, leaks: leaks.length } });
  assert.ok(bodies.length >= 5, 'sweep must cover >= 5 captured bodies, got ' + bodies.length);
  assert.equal(leaks.length, 0);
});

after(() => {
  if (!LIVE) return;
  const s = ev.flush();
  console.log(`\n[evidence] ${s.passed}/${s.cells} cells passed → docs/validation/evidence/${ev.runId}/`);
});
