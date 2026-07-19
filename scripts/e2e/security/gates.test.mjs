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

test('TS5-internal-gate-fail-closed · /internal is 401 without the internal-auth header', { skip: gate }, async () => {
  const r = await req('GET', '/internal/v1/status');
  bodies.push(r.text);
  ev.record({ cell: 'TS5-internal-gate-fail-closed', atoms: ['F-5.1', 'F-4.2', 'S7.4'], direction: 'adversarial',
    grade: 'E2', stimulus: `GET ${BASE}/internal/v1/status (no X-Corelink-Internal-Auth)`,
    assertion: 'the privileged /internal surface is unreachable without the internal-auth header — 401, fail-closed',
    pass: r.status === 401, artifact: { status: r.status, body: r.text.slice(0, 200) } });
  assert.equal(r.status, 401);
});

test('TS5-v1-namespace-auth-front · an unknown /v1 route is 401 (no unauthenticated route oracle)', { skip: gate }, async () => {
  const r = await req('GET', '/v1/this-route-does-not-exist');
  bodies.push(r.text);
  // 401 (not 404) proves auth fronts the whole /v1 namespace: an unauthenticated caller
  // cannot distinguish a real route from a fake one — no enumeration oracle.
  ev.record({ cell: 'TS5-v1-namespace-auth-front', atoms: ['F-3.2', 'F-5.1', 'S7.4'], direction: 'adversarial',
    grade: 'E2', stimulus: `GET ${BASE}/v1/this-route-does-not-exist (no PAT)`,
    assertion: 'auth fronts the entire /v1 namespace — unknown route still 401, no route-existence oracle for the unauthenticated',
    pass: r.status === 401, artifact: { status: r.status, body: r.text.slice(0, 200) } });
  assert.equal(r.status, 401);
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

test('TS5-cross-tenant-no-oracle · a foreign lease id is 404 under my PAT (no existence oracle)', { skip: gatePat }, async () => {
  const r = await req('GET', '/v1/leases/11111111-1111-4111-8111-111111111111', { pat: PAT });
  bodies.push(r.text);
  // 404 (not 403/200) — the fabric does not confirm whether a lease exists in another tenant.
  ev.record({ cell: 'TS5-cross-tenant-no-oracle', atoms: ['F-4.2', 'S7.4'], direction: 'adversarial',
    grade: 'E2', stimulus: `GET ${BASE}/v1/leases/{foreign-uuid} (f0005 PAT)`,
    assertion: 'reading a lease id outside my tenant is 404 — no cross-tenant existence oracle, isolation holds',
    pass: r.status === 404, artifact: { status: r.status, body: r.text.slice(0, 200) } });
  assert.equal(r.status, 404);
});

test('TS5-gate-nonleak-sweep · no gate-rejection body leaks secret-shaped material', { skip: gate }, () => {
  const leaks = bodies.filter((b) => bodyLeaksSecret(b));
  ev.record({ cell: 'TS5-gate-nonleak-sweep', atoms: ['S7.2', 'F-4.2'], direction: 'adversarial',
    grade: 'E2', stimulus: 'sweep every captured gate-rejection body',
    assertion: 'no rejection body contains a bearer/PAT/private-key/api-key shaped token',
    pass: leaks.length === 0, artifact: { bodiesSwept: bodies.length, leaks: leaks.length } });
  assert.equal(leaks.length, 0);
});

after(() => {
  if (!LIVE) return;
  const s = ev.flush();
  console.log(`\n[evidence] ${s.passed}/${s.cells} cells passed → docs/validation/evidence/${ev.runId}/`);
});
