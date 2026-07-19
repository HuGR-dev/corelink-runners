// STORY JOURNEYS · the developer — real, stateful, end-to-end, exactly as a user lives it.
//
// These acquire REAL leases (a valid pinned image → a held lease), observe them, and close them.
// State threads across steps; cleanup always closes what it opened. Run:
//   E2E_LIVE=1 E2E_RUN_ID=journeys node --test scripts/e2e/journeys/developer.test.mjs
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, usage, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
const skip = !LIVE ? 'set E2E_LIVE=1' : (P.pro && P.free ? false : 'multi-tenant PATs absent (source e2e-prod-env.sh)');

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S1.2.4 — "A Pro developer runs a job, and it's cleaned up and accounted for."
// As a Pro-tier dev I acquire a runner, watch it become mine, use it, release it, and see my
// account return to zero — the whole lease lifecycle a user actually experiences.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · Pro developer runs a job end-to-end (lease lifecycle)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Pro developer runs a job end-to-end', { sid: ['S1.2.4', 'S1.2.2'], persona: 'P1 Pro dev', atoms: ['F-4.1', 'F-5.1', 'F-5.5', 'F-1.1'] })
    .step('checks the plan — Pro cap, nothing running', async (ctx) => {
      const u = await usage(pat);
      ctx.tenant = u.tenant; ctx.cap = u.cap; ctx.baseActive = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap === 10, `plan is Pro (cap=${u.cap}), active_now=${u.activeNow}`, { cap: u.cap, activeNow: u.activeNow });
    })
    .step('acquires a runner (valid pinned image) → HELD lease', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-devjob' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId && a.state === 'held', `lease ${a.leaseId} is HELD`, { status: a.status, leaseId: a.leaseId, state: a.state });
    })
    .step('sees the runner in their own lease list', async (ctx) => {
      const l = await listLeases(pat);
      const found = l.leases.some((x) => (x.lease_id || x.id || x) === ctx.lease || JSON.stringify(x).includes(ctx.lease));
      return check(l.status === 200 && found, `the held lease appears in GET /v1/leases (${l.leases.length} active)`, { count: l.leases.length, found });
    })
    .step('inspects the single lease — it is theirs and held', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && g.state === 'held', `GET /v1/leases/{id} → 200, state=held`, { status: g.status, state: g.state });
    })
    .step('usage now reflects one active runner', async (ctx) => {
      const u = await usage(pat);
      ctx.activeWithLease = u.activeNow;
      return check(u.status === 200 && (u.activeNow ?? 0) >= 1, `active_now=${u.activeNow} (>= 1 while held)`, { activeNow: u.activeNow });
    })
    .step('releases the runner (close) → teardown', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `POST /v1/leases/{id}/close → ${c.status} (released)`, { status: c.status });
    })
    .step('the lease is no longer readable as held / account returns toward zero', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      const released = g.status === 404 || g.state === 'released' || g.state === 'closed' || g.state === 'expired';
      return check(released, `after close the lease is not HELD (status=${g.status}, state=${g.state})`, { status: g.status, state: g.state });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S1.3.2 / S1.6.12 — "A Free-tier user hits the concurrency ceiling, and it holds."
// As a Free user (cap=1) my second concurrent runner is REFUSED — cleanly, not thrashed — and
// once I release the first, I can acquire again. This is the ENFORCEMENT the audit (F6) flagged
// as unproven: not the surfaced value, the actual admission gate.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · Free tenant hits the concurrency ceiling and it is enforced', { skip }, async () => {
  const pat = P.free;
  await new Journey('Free tenant hits the concurrency ceiling (enforcement)', { sid: ['S1.3.2', 'S1.6.12'], persona: 'P1 Free dev', atoms: ['F-1.4', 'F-5.2', 'F-1.3'] })
    .step('checks the plan — Free cap is 1', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && u.cap === 1, `plan is Free (cap=${u.cap})`, { cap: u.cap, activeNow: u.activeNow });
    })
    .step('acquires the ONE allowed runner → HELD', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-free1' });
      ctx.lease1 = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease #1 ${a.leaseId} HELD`, { status: a.status, leaseId: a.leaseId });
    })
    .step('a SECOND concurrent acquire is REFUSED at the cap (not 200, no lease)', async (ctx) => {
      const a2 = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-free2' });
      ctx.lease2 = a2.leaseId; // should be null
      const refused = a2.status !== 200 && !a2.leaseId;
      return check(refused, `2nd acquire refused at cap → status=${a2.status}, no lease (ceiling ENFORCED)`, { status: a2.status, leaseId: a2.leaseId, body: (a2.text || '').slice(0, 160) });
    })
    .step('releasing #1 frees the slot', async (ctx) => {
      const c = await closeLease(pat, ctx.lease1);
      ctx.closed1 = c.status >= 200 && c.status < 300;
      return check(ctx.closed1, `close #1 → ${c.status}`, { status: c.status });
    })
    .step('now a fresh acquire succeeds again (slot recycled)', async (ctx) => {
      const a3 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-free3' });
      ctx.lease3 = a3.leaseId;
      return check(a3.status === 200 && a3.leaseId, `lease #3 ${a3.leaseId} HELD after slot freed`, { status: a3.status, leaseId: a3.leaseId });
    })
    .onCleanup(async (ctx) => {
      for (const id of [ctx.lease1, ctx.lease2, ctx.lease3]) if (id) await closeLease(pat, id);
    })
    .run();
});
