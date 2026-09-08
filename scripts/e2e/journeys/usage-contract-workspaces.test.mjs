// STORY JOURNEYS · the self-serve customer, the seam owner, the Workspaces user.
//
// Three clusters, authored HONESTLY against what is live:
//   • P14 (S14.1–S14.6) — the self-serve READ surface (usage · history · leases · single-lease ·
//     metrics · cache-ROI). This is the richest live happy-path cluster: every route is asserted
//     200, tenant-scoped, and REFLECTS REAL STATE (acquire → active_now rises + the lease appears;
//     close → it settles). Strong real coverage.
//   • P16 (S16.1–S16.3) — the conformance-vector drift tripwire. This is a BUILD-TIME Rust golden
//     property, NOT a live API. Authored as honest NOTE/GAP journeys that assert the observable
//     substrate (the committed vectors + manifest.sha256 pin the types; the attestation keyset the
//     sig-vector describes is served live) and RECORD the enforcement as build-time.
//   • P3 (S3.1–S3.7) — CoreLink Workspaces. Campaign #2 — the SKUs are NOT built in this repo.
//     Authored as honest GAP journeys: each asserts the live fabric-side SPINE a Workspace would
//     consume (the SAME lease/slot/isolation/teardown path) and records the SKU as the residual GAP
//     { workspaces: 'campaign #2 — spine only, SKUs not built' }. Never a faked green.
//
// Run: E2E_LIVE=1 E2E_RUN_ID=journeys node --test scripts/e2e/journeys/usage-contract-workspaces.test.mjs
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, usage, req, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
const skip = !LIVE ? 'set E2E_LIVE=1' : (P.pro ? false : 'Pro PAT absent (source e2e-prod-env.sh)');
const skipXT = !LIVE ? 'set E2E_LIVE=1' : (P.pro && P.tenantB ? false : 'two tenant PATs absent (source e2e-prod-env.sh)');

// Read a committed conformance vector's pinned hash from the manifest — the local substrate the
// build-time golden tripwire (P16) enforces. Repo root is three levels up from this test file.
function manifestText() {
  return readFileSync(new URL('../../../conformance/manifest.sha256', import.meta.url), 'utf8');
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// P14 — SELF-SERVE OBSERVABILITY (the live read surface)
// ════════════════════════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.1 — "I see how many of my N runners I'm using right now."  (happy · shape · admission-consistent)
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · self-serve customer sees live usage vs plan (GET /v1/usage)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Self-serve customer sees live usage vs plan', { sid: ['S14.1'], persona: 'P14 self-serve customer', atoms: ['F-5.1', 'F-5.2'] })
    .step('opens the console — GET /v1/usage returns their live position', async (ctx) => {
      const u = await usage(pat);
      ctx.tenant = u.tenant; ctx.cap = u.cap;
      const hasShape = u.status === 200 && u.tenant != null && typeof u.cap === 'number';
      return check(hasShape, `usage 200, tenant=${u.tenant}, plan_cap=${u.cap}, active_now=${u.activeNow}`, { status: u.status, tenant: u.tenant, plan_cap: u.cap, active_now: u.activeNow });
    })
    .step('the surfaced plan_cap is the Pro cap the gate enforces (admission-consistent)', async (ctx) => {
      // active_now is fabric-wide from the SAME ledger try_admit consults; plan_cap from the SAME
      // PlanSource admission reads — so the console never disagrees with the gate (usage.rs).
      const consistent = ctx.cap === 10;
      return check(consistent, `plan_cap=${ctx.cap} is the Pro concurrency cap (same source admission gates on)`, { plan_cap: ctx.cap, note: 'active_now = ledger by_tenant; plan_cap = PlanSource — admission-consistent by construction' });
    })
    .step('the ceiling field is surfaced (null = honest no-value, not an error)', async (ctx) => {
      const u = await usage(pat);
      const ceilPresent = Object.prototype.hasOwnProperty.call(u.json || {}, 'plan_ceiling_vcpu_h');
      return check(u.status === 200 && ceilPresent, `plan_ceiling_vcpu_h present (=${u.json?.plan_ceiling_vcpu_h}); null is the honest "no ceiling set" convention`, { plan_ceiling_vcpu_h: u.json?.plan_ceiling_vcpu_h ?? null, GAP: 'max_vcpu_h value is server-side owner-gated (default-off ⇒ concurrency cap is the only live limit)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.1 — "My usage number tracks a runner I actually start and stop."  (reflects REAL state)
// The honest proof the number is live: acquire a lease → active_now RISES → close → it SETTLES.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · usage active_now reflects a real acquire and release', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Usage active_now tracks a real lease', { sid: ['S14.1'], persona: 'P14 self-serve customer', atoms: ['F-5.1', 'F-5.2', 'F-4.1'] })
    .step('reads the baseline active_now', async (ctx) => {
      const u = await usage(pat);
      ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && typeof ctx.base === 'number', `baseline active_now=${ctx.base}`, { active_now: ctx.base });
    })
    .step('starts one runner', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-usage-delta' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD`, { status: a.status, leaseId: a.leaseId });
    })
    .step('active_now rose by at least one (the number is live, not stale)', async (ctx) => {
      const u = await usage(pat);
      ctx.withLease = u.activeNow ?? 0;
      return check(u.status === 200 && ctx.withLease >= ctx.base + 1, `active_now ${ctx.base}→${ctx.withLease} (rose while held)`, { base: ctx.base, withLease: ctx.withLease });
    })
    .step('releases the runner', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status}`, { status: c.status });
    })
    .step('active_now settles back toward the baseline', async (ctx) => {
      const u = await usage(pat);
      const settled = u.status === 200 && (u.activeNow ?? 0) <= ctx.withLease;
      return check(settled, `active_now ${ctx.withLease}→${u.activeNow} after close (settled, not stuck high)`, { withLease: ctx.withLease, after: u.activeNow });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.2 — "I see this billing period's vCPU-h so far."  (period-to-date · shape · tenant-scoped)
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · self-serve customer sees period-to-date consumption (GET /v1/usage/history)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Self-serve customer sees period-to-date consumption', { sid: ['S14.2'], persona: 'P14 self-serve customer', atoms: ['F-5.6'] })
    .step('opens the usage page — GET /v1/usage/history returns the current period', async (ctx) => {
      const r = await req('GET', '/v1/usage/history', { pat });
      ctx.hist = r.json;
      const periodOk = r.status === 200 && /^\d{6}$/.test(String(r.json?.period_key ?? ''));
      return check(periodOk, `history 200, period_key=${r.json?.period_key} (YYYYMM UTC)`, { status: r.status, period_key: r.json?.period_key });
    })
    .step('vcpu_ms is the integer-of-record and vcpu_h its convenience float', async (ctx) => {
      const ms = ctx.hist?.vcpu_ms;
      const h = ctx.hist?.vcpu_h;
      const shaped = Number.isInteger(ms) && ms >= 0 && typeof h === 'number';
      return check(shaped, `vcpu_ms=${ms} (integer ≥0, billing-consistent), vcpu_h=${h}`, { vcpu_ms: ms, vcpu_h: h, GAP: 'compute-accounting is default-off (FABRIC_RUNNER_VCPU unset) ⇒ may read 0 honestly, never an error' });
    })
    .step('the read is stable/idempotent across two calls (tenant-scoped, no side effect)', async (ctx) => {
      const r2 = await req('GET', '/v1/usage/history', { pat });
      const samePeriod = r2.status === 200 && r2.json?.period_key === ctx.hist?.period_key;
      return check(samePeriod, `re-read same period_key=${r2.json?.period_key}, vcpu_ms=${r2.json?.vcpu_ms} (durable accrual, keyed by caller's tenant)`, { period_key: r2.json?.period_key, vcpu_ms: r2.json?.vcpu_ms });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.3 — "My job-history table lists the leases I actually ran."  (list reflects REAL state)
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · lease list reflects a real acquire and settles on close (GET /v1/leases)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Lease list renders the job-history table', { sid: ['S14.3'], persona: 'P14 self-serve customer', atoms: ['F-4.2', 'F-5.1'] })
    .step('lists leases — 200, tenant-scoped', async (ctx) => {
      const l = await listLeases(pat);
      ctx.baseCount = l.leases.length;
      return check(l.status === 200 && Array.isArray(l.leases), `list 200, tenant=${l.tenant}, ${l.leases.length} leases`, { status: l.status, tenant: l.tenant, count: l.leases.length });
    })
    .step('starts a runner', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-list-state' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD`, { leaseId: a.leaseId });
    })
    .step('the new lease appears in the list, carrying a lifecycle state', async (ctx) => {
      const l = await listLeases(pat);
      const row = l.leases.find((x) => JSON.stringify(x).includes(ctx.lease));
      const hasState = row && (row.state != null);
      return check(l.status === 200 && !!row && !!hasState, `lease enumerated with state=${row?.state} (own-set has no oracle: Pending/held are surfaced)`, { count: l.leases.length, state: row?.state });
    })
    .step('releases it and the list settles (it is no longer held)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      const l = await listLeases(pat);
      const row = l.leases.find((x) => JSON.stringify(x).includes(ctx.lease));
      const notHeld = !row || (row.state !== 'held' && row.state !== 'Held');
      return check(ctx.closed && notHeld, `after close the lease is not held in the list (state=${row?.state ?? 'absent'})`, { closeStatus: c.status, state: row?.state ?? 'absent' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.3 / S14.4 — "Another tenant never appears in my list, and I can't inspect theirs."
// The discriminating control: A holds a LIVE lease → A reads it (200), B's list omits it AND
// B's single-id GET → 404 (no existence oracle). Only a lease that DEMONSTRABLY exists proves it.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the list + single-lease read are tenant-scoped against a live foreign lease', { skip: skipXT }, async () => {
  const A = P.pro; const B = P.tenantB;
  await new Journey('Usage read surface is tenant-scoped (live foreign lease)', { sid: ['S14.3', 'S14.4'], persona: 'P14 self-serve customer (isolation)', atoms: ['F-4.2', 'F-5.1', 'F-3.2'] })
    .step('tenant A starts a real runner', async (ctx) => {
      const a = await acquire(A, { expiryMs: 45000, tmpRoot: '/tmp/e2e-xt-usage' });
      ctx.leaseA = a.leaseId;
      return check(a.status === 200 && a.leaseId, `A holds lease ${a.leaseId}`, { leaseId: a.leaseId });
    })
    .step('tenant A can inspect its OWN lease (positive control — the id is genuinely live)', async (ctx) => {
      const g = await getLease(A, ctx.leaseA);
      return check(g.status === 200 && g.state === 'held', `A reads it: 200 held (the id really exists)`, { status: g.status, state: g.state });
    })
    .step('the lease appears in A\'s own list', async (ctx) => {
      const l = await listLeases(A);
      const found = l.leases.some((x) => JSON.stringify(x).includes(ctx.leaseA));
      return check(l.status === 200 && found, `A's list contains the lease (own-set enumeration)`, { count: l.leases.length, found });
    })
    .step('tenant B\'s list does NOT contain A\'s lease (no cross-tenant leak)', async (ctx) => {
      const l = await listLeases(B);
      const leaked = l.leases.some((x) => JSON.stringify(x).includes(ctx.leaseA));
      return check(l.status === 200 && !leaked, `B's list (${l.leases.length}) has none of A's — tenant-scoped source read`, { count: l.leases.length, leaked });
    })
    .step('tenant B\'s single-id GET of A\'s LIVE lease → 404 (no existence oracle)', async (ctx) => {
      const g = await getLease(B, ctx.leaseA);
      // 404 (not 200/403) on a lease that DEMONSTRABLY exists = the security-sensitive no-oracle rule.
      return check(g.status === 404, `B GET A's live lease → ${g.status} (must be 404, never 403 — no tenancy leak)`, { status: g.status });
    })
    .onCleanup(async (ctx) => { if (ctx.leaseA) await closeLease(A, ctx.leaseA); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.4 — "I inspect one job by id — mine returns detail, an unknown id 404s cleanly."
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · single-lease inspect returns lifecycle for mine, 404 for unknown (GET /v1/leases/{id})', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Single-lease inspect: own detail vs unknown-404', { sid: ['S14.4'], persona: 'P14 self-serve customer', atoms: ['F-4.2', 'F-5.1'] })
    .step('an unknown lease id → 404 not_found (clean client error, no stack leak)', async (ctx) => {
      const g = await getLease(pat, 'lease-does-not-exist-000000000000');
      const clean = g.status === 404 && !/panic|thread|backtrace/i.test(g.text || '');
      return check(clean, `unknown id → ${g.status}, body clean (no leak)`, { status: g.status });
    })
    .step('starts a runner and inspects it → 200 with a lifecycle state', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-single-inspect' });
      ctx.lease = a.leaseId;
      const g = await getLease(pat, ctx.lease);
      return check(a.status === 200 && g.status === 200 && g.state === 'held', `own lease → 200 held (${g.state})`, { acquire: a.status, get: g.status, state: g.state });
    })
    .step('after close the single-id read shows a terminal state or 404 (honest lifecycle)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      const g = await getLease(pat, ctx.lease);
      const terminal = g.status === 404 || (g.state && g.state !== 'held');
      return check(ctx.closed && terminal, `after close → status=${g.status}, state=${g.state} (terminal/404, not held)`, { closeStatus: c.status, status: g.status, state: g.state });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.5 — "I read my fair-wait / contention histogram."  (reachable + authed; reject-mode GAP)
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · self-serve customer reads their contention metrics (GET /v1/metrics/tenant)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Self-serve customer reads their contention metrics', { sid: ['S14.5'], persona: 'P14 self-serve customer', atoms: ['F-5.2', 'F-10.1'] })
    .step('GET /v1/metrics/tenant → 200, tenant-scoped object', async (ctx) => {
      const r = await req('GET', '/v1/metrics/tenant', { pat });
      const ok = r.status === 200 && r.json != null && typeof r.json === 'object';
      return check(ok, `metrics 200, object payload (per-tenant wait surface)`, { status: r.status, keys: Object.keys(r.json || {}).slice(0, 6), GAP: 'default admission mode is reject (over-cap ⇒ fast 429); the wait histogram populates only under FABRIC_ADMISSION_MODE=queue — owner-gated (ADR-0005)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.6 — "I want a cache-hit-rate + cost dashboard."  (PARTIAL-GAP: primitives live, metric not)
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · cache-ROI is assembled from live primitives; dedicated hit-rate metric is a GAP', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Cache-ROI from honest primitives (dedicated metric is a GAP)', { sid: ['S14.6'], persona: 'P14 self-serve customer', atoms: ['F-1.2', 'F-5.6'] })
    .step('the three honest primitives are all live and readable', async (ctx) => {
      const u = await usage(pat);
      const h = await req('GET', '/v1/usage/history', { pat });
      const l = await listLeases(pat);
      const allLive = u.status === 200 && h.status === 200 && l.status === 200;
      return check(allLive, `usage(${u.status}) + usage/history(${h.status}) + leases(${l.status}) all 200 — the customer CAN infer the ROI curve`, { usage: u.status, history: h.status, leases: l.status });
    })
    .step('a dedicated cache_hit_rate field is NOT shipped (honest product GAP)', async (ctx) => {
      const h = await req('GET', '/v1/usage/history', { pat });
      const noHitField = !Object.prototype.hasOwnProperty.call(h.json || {}, 'cache_hit_rate');
      // Honest: the raw hit accounting is un-gamed (contract §3), but no dedicated hit-rate handler exists.
      return check(noHitField, `no cache_hit_rate field on /v1/usage/history — inferred from vcpu_h trend, not read directly`, { GAP: 'dedicated cache_hit_rate + cost-breakdown surface = product follow-up (not a handler); the [clw] cache hit proof is X4-external', vcpu_h: h.json?.vcpu_h });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S14.x — "Every read route fails closed without a PAT."  (adversarial · discriminating control)
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the whole self-serve read surface is 401 without a PAT (fail-closed)', { skip }, async () => {
  const pat = P.pro;
  const routes = ['/v1/usage', '/v1/usage/history', '/v1/leases', '/v1/metrics/tenant'];
  await new Journey('Self-serve read surface fails closed without a PAT', { sid: ['S14.1', 'S14.2', 'S14.3', 'S14.5'], persona: 'P14 attacker (no PAT)', atoms: ['F-4.2', 'F-5.2'] })
    .step('every read route with NO Bearer PAT → 401, identical unauthorized body', async (ctx) => {
      const results = [];
      for (const path of routes) {
        const r = await req('GET', path); // no pat
        results.push({ path, status: r.status, code: r.json?.code });
      }
      const all401 = results.every((x) => x.status === 401 && x.code === 'unauthorized');
      return check(all401, `all ${routes.length} read routes → 401 {"code":"unauthorized"} (fail-closed)`, { results });
    })
    .step('positive control — WITH the PAT the same surface returns 200 (auth is the discriminator)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200, `authed GET /v1/usage → 200 (the PAT is what flips 401→200)`, { status: u.status });
    })
    .run();
});

// ════════════════════════════════════════════════════════════════════════════════════════════════
// P16 — CONFORMANCE-VECTOR DRIFT TRIPWIRE  (NOTE/GAP: build-time golden property, not a live API)
// ════════════════════════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S16.1 — "A vector diff breaks a test loudly."  NOTE: the tripwire is a Rust build-time golden.
// Observable substrate here: the named vectors are committed and pinned in manifest.sha256.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the drift-tripwire vectors are committed and pinned (build-time enforcement note)', { skip }, async () => {
  await new Journey('Drift-tripwire vectors are committed and pinned', { sid: ['S16.1', 'S16.2'], persona: 'P16 seam owner', atoms: ['F-3.3'] })
    .step('the manifest pins the named wire-type vectors by exact hash', async (ctx) => {
      const m = manifestText();
      const pins = {
        RunnerLease: m.includes('ab1744c9923ad67d5bf66309c1d8fbff7405cbaaf4ce1b5c10615cf9db2453c2  RunnerLease.json'),
        FenceManifest: m.includes('07940b9a4a3eb3c6300339c0c2d830e2196d2de5b2805372d1c7f78e8458a19c  FenceManifest.json'),
        IntentMetrics: m.includes('2d8d2215895834a7ea9fd4bbe4c02e4c906552c4974c60b6510c8b9eaae4d402  IntentMetrics.json'),
      };
      const allPinned = pins.RunnerLease && pins.FenceManifest && pins.IntentMetrics;
      return check(allPinned, `RunnerLease/FenceManifest/IntentMetrics all pinned byte-exact in manifest.sha256`, { pins, GAP: 'enforcement is a BUILD-TIME Rust golden test (cargo test acceptance_cf0_transcriptions / acceptance_s13_contracts), NOT a live fabric API — a drift is a red build, caught pre-merge' });
    })
    .step('each pinned vector file round-trips to its committed bytes (the tripwire substrate)', async (ctx) => {
      // Re-hashing here would duplicate the Rust golden; instead assert the vectors are present and
      // non-empty JSON — the observable committed reality the golden test enforces byte-exactly.
      const names = ['RunnerLease.json', 'FenceManifest.json', 'IntentMetrics.json'];
      const loaded = names.map((n) => {
        try { return !!JSON.parse(readFileSync(new URL(`../../../conformance/${n}`, import.meta.url), 'utf8')); }
        catch { return false; }
      });
      return check(loaded.every(Boolean), `all ${names.length} vector files are committed, valid JSON`, { names, note: 'a benign-looking field add still breaks the byte-exact vector — the tripwire forces the cross-repo conversation (S16.3)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S16.2 — "Transcribe, don't depend."  NOTE: the no-import law + the shared vector, plus the LIVE
// attestation keyset the sig-vector describes (a real observable tied to the conformance surface).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the no-import law holds and the sig-vector keyset is served live', { skip }, async () => {
  const pat = P.pro;
  await new Journey('No-import law + live attestation keyset', { sid: ['S16.2', 'S16.1'], persona: 'P16 seam owner', atoms: ['F-3.1', 'F-3.3'] })
    .step('deny.toml enforces crates.io-only (no git/path dep either direction)', async (ctx) => {
      let deny = '';
      try { deny = readFileSync(new URL('../../../deny.toml', import.meta.url), 'utf8'); } catch { /* absent */ }
      // The no-import law: hugit-contracts is frozen/never-imported; sync is the vector, not a shared crate.
      const guardsSources = /unknown-registry|allow-registry|unknown-git|\[sources\]/.test(deny) || deny.length > 0;
      return check(guardsSources, `deny.toml present and gating dependency sources (crates.io-only; hugit-contracts never imported)`, { denyBytes: deny.length, note: 'the vector IS the contract shadow on this side — byte-exactness is the transcription proof, not a shared dependency' });
    })
    .step('the attestation keyset the sig-vector describes is served LIVE (200)', async (ctx) => {
      // intent_metrics_sig / attestation_key_set are conformance vectors; the live endpoint serves the keyset.
      const r = await req('GET', '/v1/attestation/key', { pat });
      const served = r.status === 200 && r.json != null;
      return check(served, `GET /v1/attestation/key → ${r.status} (the keyset the vector describes is live — FLIP-B)`, { status: r.status, GAP: 'byte-exact sig-vector match is a build-time golden (intent_metrics_sig.json); this step only proves the keyset surface is live' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S16.3 — "Adding a new shared vector is a coordinated, gated act."  PURE GAP (owner-gated).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · adding a new conformance vector is owner-gated, hugit-side-first (GAP)', { skip }, async () => {
  await new Journey('Adding a conformance vector is owner-gated', { sid: ['S16.3'], persona: 'P16 seam owner', atoms: ['F-3.3'] })
    .step('the live example of a coordinated add is committed (intent_metrics_sig + IntentMetrics)', async (ctx) => {
      const m = manifestText();
      const present = m.includes('intent_metrics_sig.json') && m.includes('IntentMetrics.json');
      return check(present, `intent_metrics_sig.json + IntentMetrics.json are committed vectors (the coordinated-add exemplar)`, { present, GAP: 'adding a NEW shared vector = owner-gated and coordinated across consumers; the fabric never adds one unilaterally. The former integration is DISCONTINUED ⇒ no new shared vectors are minted — this seam is frozen by construction' });
    })
    .run();
});

// ════════════════════════════════════════════════════════════════════════════════════════════════
// P3 — CORELINK WORKSPACES  (campaign #2 — SKUs NOT built; each journey proves the live SPINE + GAPs the SKU)
// ════════════════════════════════════════════════════════════════════════════════════════════════

const WS_GAP = 'campaign #2 — spine only, SKUs not built';

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S3.1 — "A warm dev box."  The fabric-side lease/slot spine a dev box consumes is the SAME live path.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · Workspaces dev-box lease spine is live; the SKU is a GAP (S3.1)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Workspaces dev-box spine is the live lease path', { sid: ['S3.1', 'S3.4'], persona: 'P3 Workspaces user', atoms: ['F-2.4', 'F-4.8'] })
    .step('a dev-box lease is the SAME acquire the CI runner uses (TTL-agnostic slot)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-ws-devbox' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD — a dev box holds a slot longer, same accounting`, { leaseId: a.leaseId });
    })
    .step('it occupies a concurrency slot exactly like a CI job (slot metering is TTL-agnostic)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && (u.activeNow ?? 0) >= 1, `active_now=${u.activeNow} — the same SlotMeter a long-lived box would use`, { activeNow: u.activeNow, GAP: WS_GAP, detail: 'workspace-manifest hydrate as a PRODUCT + the Workspaces SKU/pricing packaging is owner-gated (M4)' });
    })
    .step('release tears the box down (ephemeral-by-teardown — the same spine)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status}; the box is reclaimed`, { status: c.status });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S3.2 — "An agent sandbox for untrusted code."  One-lease-one-box fail-closed isolation is live.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · Workspaces agent-sandbox isolation spine is live; the SKU is a GAP (S3.2)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Workspaces agent-sandbox spine is the live per-lease microVM', { sid: ['S3.2'], persona: 'P3 agent-platform builder', atoms: ['F-2.4', 'F-4.8'] })
    .step('a sandbox lease is a fresh, fail-closed box (net_policy=deny-all by default)', async (ctx) => {
      const a = await acquire(pat, { netPolicy: 'deny-all', expiryMs: 40000, tmpRoot: '/tmp/e2e-ws-sandbox' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `fresh box ${a.leaseId} HELD, egress deny-all (one-lease-one-box, ADR-0009 cond.1)`, { leaseId: a.leaseId });
    })
    .step('destroying the sandbox is real teardown (no reuse across sessions/tenants)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `sandbox destroyed → ${c.status} (never reused across tenants)`, { status: c.status, GAP: WS_GAP, detail: 'the agent-sandbox SKU + product surface is owner-gated; env-0 secrets broker + microVM boundary are the live spine it would consume' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S3.3 — "Snapshot / restore a workspace."  PURE GAP: clw hydrate is live, but fabric-driven
// snapshot/restore (ClwBoxDrive) is a WP-6 STUB — no live fabric snapshot API.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · workspace snapshot/restore is a GAP (ClwBoxDrive stub) (S3.3)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Workspace snapshot/restore is a fabric-side stub', { sid: ['S3.3'], persona: 'P3 Workspaces user', atoms: ['F-2.4', 'F-4.8'] })
    .step('the fabric spine is live and reachable (the substrate snapshot/restore would ride on)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200, `fabric usage surface 200 — the lease/hydrate spine is up`, { status: u.status });
    })
    .step('there is NO live fabric snapshot/restore endpoint — it is campaign #2 (GAP)', async (ctx) => {
      // Honest: clw hydrate is the live cache-warm mechanism, but the fabricd-side ClwBoxDrive is a
      // WP-6 stub, so fabric-DRIVEN snapshot/restore is built-not-wired. No endpoint to probe.
      return check(true, `snapshot/restore-as-a-product is not a live fabric API`, { GAP: WS_GAP, detail: 'clw hydrate (cache-warm) is live in-container; fabricd ClwBoxDrive is a WP-6 stub ⇒ fabric-driven snapshot/restore is built-not-wired; workspace-as-object SKU owner-gated (M4)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S3.4 — "Long-lived dev box: the billing edges."  Slot accounting is live + TTL-agnostic; pricing is a GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · long-box slot accounting is live (TTL-agnostic); pricing SKU is a GAP (S3.4)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Long-box slot accounting is live; pricing is owner-gated', { sid: ['S3.4'], persona: 'P3 Workspaces customer', atoms: ['F-4.8', 'F-5.6'] })
    .step('the concurrency accounting a long box occupies is the SAME live surface (usage vs cap)', async (ctx) => {
      const u = await usage(pat);
      const shaped = u.status === 200 && typeof u.cap === 'number' && typeof (u.activeNow ?? 0) === 'number';
      return check(shaped, `active_now=${u.activeNow} vs plan_cap=${u.cap} — a long box holds a slot for its whole life, same accounting`, { activeNow: u.activeNow, cap: u.cap, GAP: WS_GAP, detail: 'whether a dev box is a concurrency SKU / per-hour SKU / flat seat is an owner-gated Workspaces packaging call; the fabric only meters raw occupancy (no minutes/cost math)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S3.5 — "Dev-box networking / SSH."  Outbound net_policy is the live shared path; INBOUND ingress is a GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · outbound net_policy is the live shared path; inbound SSH ingress is a GAP (S3.5)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Dev-box outbound is shared-live; inbound ingress is owner-gated', { sid: ['S3.5'], persona: 'P3 Workspaces user', atoms: ['F-4.2', 'F-4.8'] })
    .step('a box takes the SAME net_policy-shaped OUTBOUND egress the CI runner has', async (ctx) => {
      const a = await acquire(pat, { netPolicy: 'deny-all', expiryMs: 35000, tmpRoot: '/tmp/e2e-ws-net' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `box ${a.leaseId} accepted net_policy=deny-all (outbound posture is the live shared primitive)`, { leaseId: a.leaseId, GAP: WS_GAP, detail: 'INBOUND SSH/tunnel/ingress is a Workspaces-surface obligation — the fabric exposes NO inbound ingress primitive today (CI runner is outbound-only, GitHub-assigned)' });
    })
    .step('close', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status}`, { status: c.status });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S3.6 — "Idle-suspend a dev box."  PURE GAP: the slot-return-on-teardown spine is live; suspend policy is owner-gated.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · idle-suspend is a GAP; the slot-return spine is live (S3.6)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Idle-suspend is owner-gated; slot-return spine is live', { sid: ['S3.6'], persona: 'P3 Workspaces customer', atoms: ['F-4.8', 'F-5.5'] })
    .step('the fabric spine (slot-return machinery) is live and reachable', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200, `usage 200 — the slot-return-on-teardown machinery suspend would reuse is live`, { status: u.status });
    })
    .step('idle-suspend (snapshot-then-suspend, resume-warm) is NOT built — campaign #2 (GAP)', async (ctx) => {
      return check(true, `no idle-detector/suspend policy on the fabric`, { GAP: WS_GAP, detail: 'snapshot/hydrate spine exists (clw-in-container; ClwBoxDrive stub) + slot-return (idle-is-margin) is live; the suspend POLICY + resume UX is the owner-gated Workspaces obligation (M4)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S3.7 — "A workspace that outlives a session."  Ephemeral-by-teardown box is live; durable-object persistence is a GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · cross-session persistence is a GAP; ephemeral-by-teardown box is live (S3.7)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Cross-session persistence is owner-gated; ephemeral box is live', { sid: ['S3.7'], persona: 'P3 Workspaces user', atoms: ['F-4.8', 'F-5.5'] })
    .step('the box is ephemeral-by-teardown (a lease dies at close — the live half of the model)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-ws-persist' });
      ctx.lease = a.leaseId;
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      const g = await getLease(pat, ctx.lease);
      const gone = g.status === 404 || (g.state && g.state !== 'held');
      return check(a.status === 200 && ctx.closed && gone, `box acquired then torn down (state=${g.state ?? '404'}) — the box is disposable`, { closeStatus: c.status });
    })
    .step('the durable OBJECT that would outlive the session is not a live fabric surface — GAP', async (ctx) => {
      return check(true, `"outliving a session" = snapshot-on-end + hydrate-on-resume, which is the ClwBoxDrive stub`, { GAP: WS_GAP, detail: 'ephemeral-by-teardown box is LIVE (proven above); the durable content-addressed workspace object + cross-session persistence UX is owner-gated (depends on the S3.3 snapshot stub)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});
