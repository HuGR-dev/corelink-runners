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

test('TS2-substrate-health · GET /health — the HTTP front answers 200', { skip: gate }, async () => {
  const r = await req('GET', '/health');
  const pass = r.status === 200;
  // Audit F10: /health may be answered at the proxy Worker edge, so this proves the FRONT is
  // up, NOT that the fabricd container is live. Container-liveness is proven by the authed
  // cells below (introspect + ledger both traverse the container).
  ev.record({ cell: 'TS2-substrate-health', atoms: ['F-7.2'], direction: 'happy',
    grade: 'E2', stimulus: `GET ${BASE}/health`,
    assertion: 'the HTTP front answers 200 (edge-or-container; container-liveness is proven separately by the authed introspect+ledger cells)',
    pass, artifact: { status: r.status, body: r.text.slice(0, 400) } });
  assert.equal(r.status, 200);
});

test('TS2-attestation-key · GET /v1/attestation/key serves a valid ed25519 pubkey (FLIP-B)', { skip: gate }, async () => {
  const r = await req('GET', '/v1/attestation/key');
  // Audit F4: assert the ACTUAL shape, not "200 + any chars". keys[].pubkey_b64 must base64-
  // decode to exactly 32 bytes (an ed25519 public key), with a stable key_id.
  const k = r.json?.keys?.[0];
  let keyBytes = -1;
  try { keyBytes = k?.pubkey_b64 ? Buffer.from(k.pubkey_b64, 'base64').length : -1; } catch { keyBytes = -1; }
  const pass = r.status === 200 && !!k?.key_id && keyBytes === 32;
  ev.record({ cell: 'TS2-attestation-key', atoms: ['F-4.10', 'F-5.4'], direction: 'happy',
    grade: 'E2', stimulus: `GET ${BASE}/v1/attestation/key`,
    assertion: 'keys[0].pubkey_b64 decodes to exactly 32 bytes (a real ed25519 public key) under a stable key_id — client-side result-binding is verifiable against this key',
    pass, artifact: { status: r.status, keyId: k?.key_id ?? null, pubkeyBytes: keyBytes } });
  assert.equal(r.status, 200);
  assert.ok(!!k?.key_id, 'expected keys[0].key_id');
  assert.equal(keyBytes, 32, 'pubkey_b64 must decode to a 32-byte ed25519 key');
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

test('TS2-authed-acquire-validation · f0005 PAT authenticates (400 not 401), rejected at server-side validation', { skip: gatePat }, async () => {
  const r = await req('POST', '/v1/leases', { pat: PAT, body: DUMMY_ACQUIRE });
  bodies.push(r.text);
  // 400 (not 401) proves BOTH: the PAT authenticated (past auth) AND the invalid image was
  // rejected server-side (a user can't bypass image validation). No box spawns.
  const pass = r.status === 400;
  // Audit F9: a 400 "invalid" is the positive control for the auth cells (the SAME route +
  // a valid PAT is 400, not 401 → the introspect layer runs and discriminates). We do NOT
  // claim which field failed — only that an authenticated request is rejected at server-side
  // validation BEFORE any box spawns.
  ev.record({ cell: 'TS2-authed-acquire-validation', atoms: ['F-5.1', 'F-8.2', 'F-4.1'], direction: 'happy',
    grade: 'E2', stimulus: `POST ${BASE}/v1/leases (f0005 PAT, deliberately-invalid body)`,
    assertion: 'authenticated (400 not 401 — proves the PAT introspects live) and rejected at server-side validation before any spawn',
    pass, artifact: { status: r.status, error: r.json?.error ?? r.json?.code ?? r.text.slice(0, 200) } });
  assert.equal(r.status, 400, 'expected past-auth server-side validation (400), got ' + r.status);
});

test('TS5-error-vocab-malformed · malformed body is a clean 4xx (no 5xx); parser-echo tracked', { skip: gatePat }, async () => {
  const r = await req('POST', '/v1/leases', { pat: PAT, headers: { 'content-type': 'application/json' } });
  // send raw invalid JSON via a manual fetch (req() JSON-encodes; here we want a bad body)
  const raw = await fetch(`${BASE}/v1/leases`, { method: 'POST',
    headers: { authorization: `Bearer ${PAT}`, 'content-type': 'application/json' },
    body: '{not valid json', signal: AbortSignal.timeout(15000) });
  const t = await raw.text();
  bodies.push(t);
  const pass = raw.status >= 400 && raw.status < 500;
  // Audit F11: scope the claim to "no 5xx". OBSERVED info-disclosure — the body echoes the
  // JSON parser internals ("Failed to parse ... line/column"), leaking the framework layer.
  // Not secret-shaped, so the leak-sweep won't flag it; recorded here as a tracked product
  // finding (harden the DTO error to an opaque code), not silently passed.
  const echoesParser = /failed to parse|line \d+ column \d+|serde|expected/i.test(t);
  ev.record({ cell: 'TS5-error-vocab-malformed', atoms: ['F-3.2'], direction: 'edge',
    grade: 'E2', stimulus: `POST ${BASE}/v1/leases (authed, malformed JSON body)`,
    assertion: 'malformed input yields a clean 4xx (no 5xx). NOTE: the error body echoes parser internals — a tracked info-disclosure finding, not a clean pass',
    pass, artifact: { status: raw.status, body: t.slice(0, 300), infoDisclosure_parserEcho: echoesParser } });
  assert.ok(pass, 'malformed body must be a 4xx, got ' + raw.status);
});

test('TS5-nonleak-sweep · no captured body leaks secret-shaped material (>= 4 bodies)', { skip: gate }, () => {
  const leaks = bodies.filter((b) => bodyLeaksSecret(b));
  // Audit F8: assert a floor on bodies swept so the sweep can't pass VACUOUSLY, and the regex
  // now includes the real corelink_ PAT shape. NOTE: these are auth/error bodies (least likely
  // to carry a secret); credential-bearing SUCCESS bodies (mint/redeem 200) are swept in the
  // box-spawn batch, not here.
  ev.record({ cell: 'TS5-nonleak-sweep', atoms: ['S7.2', 'F-4.2'], direction: 'adversarial',
    grade: 'E2', stimulus: 'sweep every captured acquire/error response body (corelink_/pat_/bearer/key regex)',
    assertion: 'no response body contains a corelink_-PAT / bearer / private-key / api-key shaped token; >= 4 bodies actually swept (non-vacuous)',
    pass: leaks.length === 0 && bodies.length >= 4, artifact: { bodiesSwept: bodies.length, leaks: leaks.length } });
  assert.ok(bodies.length >= 4, 'sweep must cover >= 4 captured bodies, got ' + bodies.length);
  assert.equal(leaks.length, 0, 'a response body leaked secret-shaped material');
});

after(() => {
  if (!LIVE) return;
  const summary = ev.flush();
  console.log(`\n[evidence] ${summary.passed}/${summary.cells} cells passed → docs/validation/evidence/${ev.runId}/`);
});
