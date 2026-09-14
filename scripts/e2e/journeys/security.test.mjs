// STORY JOURNEYS · the adversary — real cross-tenant isolation, with a LIVE foreign lease.
//
// This is the honest version of the cross-tenant proof the audit (F2/F3) demoted: tenant A
// opens a REAL lease, then tenant B tries to see and touch it. Only a 404/deny on a lease that
// actually EXISTS in another tenant proves "no cross-tenant oracle" — a random uuid never could.
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
const skip = !LIVE ? 'set E2E_LIVE=1' : (P.pro && P.tenantB ? false : 'two tenant PATs absent (source e2e-prod-env.sh)');

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S7.4 — "Tenant B cannot see or touch Tenant A's runner."
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a second tenant cannot see or touch the first tenant\'s live lease', { skip }, async () => {
  const A = P.pro;      // tenant A (f0004)
  const B = P.tenantB;  // tenant B (f0002)
  await new Journey('Cross-tenant isolation on a live foreign lease', { sid: ['S7.4', 'S14.4'], persona: 'P7 attacker (tenant B)', atoms: ['F-4.2', 'F-5.1', 'F-3.2'] })
    .step('tenant A opens a real runner and it is HELD', async (ctx) => {
      const a = await acquire(A, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-tenantA' });
      ctx.leaseA = a.leaseId;
      return check(a.status === 200 && a.leaseId, `tenant A holds lease ${a.leaseId}`, { status: a.status, leaseId: a.leaseId });
    })
    .step('tenant A can read its OWN lease (positive control — the id is genuinely live)', async (ctx) => {
      const g = await getLease(A, ctx.leaseA);
      return check(g.status === 200 && g.state === 'held', `owner reads it: 200 held (id is a real, existing lease)`, { status: g.status, state: g.state });
    })
    .step('tenant B tries to read tenant A\'s LIVE lease → 404, no oracle', async (ctx) => {
      const g = await getLease(B, ctx.leaseA);
      // 404 (not 200/403) on a lease that DEMONSTRABLY exists in tenant A = no cross-tenant
      // existence oracle. This is the proof a random uuid could never give.
      return check(g.status === 404, `tenant B GET A's live lease → ${g.status} (must be 404 — isolation holds)`, { status: g.status, body: (g.text || '').slice(0, 160) });
    })
    .step('tenant B tries to CLOSE tenant A\'s live lease → refused', async (ctx) => {
      const c = await closeLease(B, ctx.leaseA);
      return check(c.status === 404 || c.status === 403, `tenant B close A's lease → ${c.status} (cannot mutate another tenant's lease)`, { status: c.status });
    })
    .step('tenant B\'s own lease list does NOT contain A\'s lease', async (ctx) => {
      const l = await listLeases(B);
      const leaked = l.leases.some((x) => JSON.stringify(x).includes(ctx.leaseA));
      return check(l.status === 200 && !leaked, `tenant B list has ${l.leases.length} leases, none is A's (no leakage)`, { count: l.leases.length, leaked });
    })
    .step('tenant A\'s lease is still intact after B\'s attempts (B could not tamper)', async (ctx) => {
      const g = await getLease(A, ctx.leaseA);
      return check(g.status === 200 && g.state === 'held', `A's lease still HELD after B's probes (untampered)`, { status: g.status, state: g.state });
    })
    .onCleanup(async (ctx) => { if (ctx.leaseA) await closeLease(A, ctx.leaseA); })
    .run();
});
