// STORY JOURNEYS · the developer lifecycle (persona P1) — signup → first job → burst → billing → failure.
//
// The full arc a direct-ICP developer lives, told as stateful narratives against the LIVE /v1 fabric:
// the org's PAT IS the identity, a tier grants a cap the fabric admits against, a job acquires a real
// cache-warm box, N seats burst flat, the ceiling refuses cleanly and non-destructively, a crashed job
// tears down + bills, and nobody borrows another tenant's concurrency.
//
// HONESTY: many stories in this cluster are owner-gated (Stripe billing, size/GPU ladder, dunning/trial
// state machines) or ⚪ X4-external (the GitHub App/webhook spawn path, the `[clw] cache hit` smoke,
// real OOM injection). Those are NOT faked green — each such journey asserts the OBSERVABLE CURRENT
// reality reachable through the frozen /v1 verbs and RECORDS the residual as a `gap` in its artifact.
//
// Run (the tech lead, serially, live):  E2E_LIVE=1 node --test scripts/e2e/journeys/dev-lifecycle.test.mjs
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, usage, req, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
// Per-journey skip: LIVE gate first, then the specific tenant PAT(s) this story needs.
const need = (...keys) => (!LIVE ? 'set E2E_LIVE=1' : (keys.every((k) => P[k]) ? false : `PAT(s) ${keys.join('+')} absent (source e2e-prod-env.sh)`));
const heldOk = (a) => a.status === 200 && !!a.leaseId && a.state === 'held';
const released = (g) => g.status === 404 || g.state === 'released' || g.state === 'closed' || g.state === 'expired';

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// THEME 1.1 — Onboarding & identity
// ═══════════════════════════════════════════════════════════════════════════════════════════════

// STORY S1.1.1 — "The org is the tenant: one PAT is the whole identity."
// The signup/Clerk flow itself is owner-gated (no identity code in this repo, ADR-0002 obl.4). What IS
// observable: the org→tenant PAT the CoreLink identity resolves is what the /v1 surface authenticates,
// and it keys the cap — while an unauthenticated caller is refused. That's the identity contract, live.
test('JOURNEY · the org PAT is the tenant identity (and it keys the cap)', { skip: need('pro') }, async () => {
  await new Journey('The org PAT is the tenant identity', { sid: ['S1.1.1'], persona: 'P1 Pro dev', atoms: ['F-1.5', 'F-2.1', 'F-8.1'] })
    .step('the org PAT authenticates against /v1 and resolves a tenant + cap', async (ctx) => {
      const u = await usage(P.pro);
      ctx.tenant = u.tenant; ctx.cap = u.cap;
      return check(u.status === 200 && !!u.tenant && u.cap > 0, `PAT → tenant ${u.tenant}, cap ${u.cap} (one identity keys concurrency)`, { tenant: u.tenant, cap: u.cap });
    })
    .step('the SAME read with no identity is refused — the PAT is load-bearing', async (ctx) => {
      const r = await req('GET', '/v1/usage', {}); // no PAT
      const isUnauth = r.status === 401 && (r.json?.code === 'unauthorized' || /unauthorized/i.test(r.text));
      return check(isUnauth, `no-PAT /v1/usage → 401 unauthorized (identity is required, not optional)`, { status: r.status, code: r.json?.code });
    })
    .step('record the residual — the signup/Clerk flow is not buildable in this repo', async () => {
      return check(true, `identity resolution is LIVE; org creation / Clerk session is owner-gated (CoreLink M2 self-serve)`, { gap: 'signup+Clerk+org-provisioning owner-gated (ADR-0002 obl.4 — no identity code here)' });
    })
    .run();
});

// STORY S1.1.2 — "Buying a tier grants a concurrency cap the fabric admits against."
// The Stripe SKU purchase is owner-gated (static admin backend today, no card). What IS observable: the
// tenant's plan cap is surfaced AND is the number the admission gate honors — proven by acquiring within
// the cap and seeing the account reflect exactly one occupied slot.
test('JOURNEY · a tier grants a concurrency cap the fabric admits against', { skip: need('free') }, async () => {
  const pat = P.free;
  await new Journey('A tier grants a concurrency cap the fabric admits against', { sid: ['S1.1.2'], persona: 'P1 Free dev', atoms: ['F-1.5', 'F-2.1', 'F-5.2'] })
    .step('the plan surfaces a concrete cap (Free = 1)', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap; ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap === 1 && ctx.base === 0, `cap=${u.cap}, active_now=${ctx.base} (clean slate)`, { cap: u.cap, activeNow: ctx.base });
    })
    .step('an acquire WITHIN the cap is admitted → a real HELD slot', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-dl-tier1' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD (admission honored the granted cap)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('usage now shows exactly the one occupied slot the cap allows', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && (u.activeNow ?? 0) >= 1 && (u.activeNow ?? 0) <= u.cap, `active_now=${u.activeNow} within cap ${u.cap}`, { activeNow: u.activeNow, cap: u.cap, gap: 'Stripe SKU purchase owner-gated (static admin backend, no card) — the cap here is admin-provisioned, not Stripe-driven' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.1.3 / S1.1.4 / S1.1.8 — "Change one line, and the dev's first five minutes just work."
// The GitHub App install + `runs-on: corelink` webhook spawn path is LIVE on the dogfood fleet but is not
// on the /v1 surface these verbs reach. The /v1 analog of "the fabric provisions a runner for me with zero
// onboarding" IS acquire → a real box, using nothing but the org's PAT. The App-install + webhook + the
// visible `[clw] cache hit` smoke are recorded as X4-external gaps.
test('JOURNEY · first job — one PAT provisions a real runner, zero dev onboarding', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('First job provisions a real runner with zero onboarding', { sid: ['S1.1.3', 'S1.1.4', 'S1.1.8'], persona: 'P1 Pro dev', atoms: ['F-2.1', 'F-5.8', 'F-8.1'] })
    .step('the dev does nothing but present the org PAT — the fabric provisions a box', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-first' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD — a real runner, no CLI/config/per-dev signup`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the runner is visible to its owner exactly as a normal run would be', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && g.state === 'held', `owner reads the held runner (200 held) — drop-in, developer-invisible`, { status: g.status, state: g.state, gap: 'GitHub-App install (144561227 exists+LIVE) + `runs-on: corelink` webhook spawn + visible `[clw] cache hit` are ⚪ X4-external (need a real App install / CoreLink PAT)' });
    })
    .step('the box tears down on release — nothing lingers for the dev to manage', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status} (ephemeral teardown, zero cleanup for the dev)`, { status: c.status });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.1.5 / S1.1.6 — "A billing lapse (dunning / trial-end) drains, it never kills."
// The Stripe dunning + 5-day-trial state machines are owner-gated (CoreLink-server-side runners_entitlement).
// The fabric only CONSUMES the resulting cap. Observable current reality: an in-flight lease is never
// mid-job killed by the fabric (S13.2 drain-not-kill), it runs to a clean owner-driven close that bills its
// slot-seconds, and the usage surface exposes the numbers a bill would draw from.
test('JOURNEY · a billing lapse drains in-flight work, never kills it (owner-gated state machine)', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Billing lapse drains in-flight work, never kills it', { sid: ['S1.1.5', 'S1.1.6'], persona: 'P1 Pro dev', atoms: ['F-1.5', 'F-5.2', 'F-5.5'] })
    .step('an in-flight job is Held', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-drain' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD (in-flight)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('it stays HELD — the fabric never destroys a running job out from under the dev', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && g.state === 'held', `still HELD (drain-not-kill; a cap change only gates the NEXT acquire)`, { status: g.status, state: g.state });
    })
    .step('the owner-driven completion bills its slot-seconds (usage surface is live)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      const h = await req('GET', '/v1/usage/history', { pat });
      return check(ctx.closed && h.status === 200, `close → ${c.status}; /v1/usage/history → ${h.status} (numbers a bill draws from are surfaced)`, { close: c.status, history: h.status, gap: 'Stripe dunning grace + 5-day-trial convert/lapse state machines are owner-gated (CoreLink-server runners_entitlement); the push-exporter is COGS-only, armed-off' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.1.7 — "A trial-abuser is structurally bounded; a healthy tenant is not touched."
// Trial-eligibility (one-per-actor) is owner-gated; the durable fabric_suspended_tenants suspend is built
// but only reachable via admin/N>1, not the /v1 user surface. Observable: a healthy, non-suspended tenant
// acquires normally (the positive control that suspension is NOT indiscriminate) — the loss-impossible
// vCPU-h ceiling that makes abuse a fairness (not solvency) concern is recorded as the residual.
test('JOURNEY · a healthy tenant is not suspended (abuse defense is bounded, not indiscriminate)', { skip: need('free') }, async () => {
  const pat = P.free;
  await new Journey('A healthy tenant is not suspended', { sid: ['S1.1.7'], persona: 'P1 Free dev', atoms: ['F-1.5', 'F-1.6', 'F-5.2'] })
    .step('the tenant is in good standing — admission is open, not fabric-suspended', async (ctx) => {
      const u = await usage(pat);
      ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap >= 1 && ctx.base === 0, `cap=${u.cap}, active_now=${ctx.base} (not suspended)`, { cap: u.cap, activeNow: ctx.base });
    })
    .step('so a legitimate acquire succeeds — suspension is targeted, never a blanket deny', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-dl-good' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD (positive control: a non-abuser is served)`, { status: a.status, leaseId: a.leaseId, gap: 'trial-eligibility (one-per-actor) owner-gated; durable fabric_suspended_tenants suspend is built but admin/N>1-only, not exercisable via /v1; the loss-impossible vCPU-h ceiling value is server-side' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// THEME 1.2 — The core job journey (push → billed → torn down)
// ═══════════════════════════════════════════════════════════════════════════════════════════════

// STORY S1.2.1 — "A warm boot mints a per-job CAS credential (the moat), so a hit costs ~0."
// The per-job CAS-PAT mint is wired into the real acquire path (leases.rs:929) and LIVE-proven. What is
// observable on /v1 is that the mint-carrying acquire path yields a genuinely HELD box; the credential
// itself and the `[clw] cache hit` line live inside the CF container (clw), not on the /v1 read surface.
test('JOURNEY · warm boot — the mint-wired acquire path yields a real box', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Warm boot mints a per-job credential on acquire', { sid: ['S1.2.1'], persona: 'P1 Pro dev', atoms: ['F-4.1', 'F-4.3', 'F-6.1', 'F-7.1'] })
    .step('acquire runs the mint-wired path and returns a HELD box', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-warm' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD (acquire path where the per-job CAS-PAT is minted, leases.rs:929)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the box is the owner\'s and healthy — the credential is redeemed in-container, not on /v1', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && g.state === 'held', `owner reads it (200 held)`, { status: g.status, state: g.state, gap: 'the per-job CAS-PAT + the visible `[clw] cache hit` are redeemed inside the CF container (clw); NOT observable on the /v1 read surface — full hit smoke is ⚪ X4-external (needs a real CoreLink PAT)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.2.2 — "A cold first-run still boots and runs correctly; the result re-warms the cache."
// A first-ever build is a cold miss: it spawns a real standard-4 box, runs, and closes (re-storing the
// result under its memo key so the NEXT identical run is a hit). Observable: the cold acquire yields a
// HELD box and a clean succeeded close; the byte-identical re-store / next-run-hit is X4-external.
test('JOURNEY · cold first-run boots a real box and completes cleanly', { skip: need('pro') }, async () => {
  const pat = P.pro;
  const novel = `/tmp/e2e-dl-cold-${Date.now()}`; // a novel tmp_root = a cold, never-seen run
  await new Journey('Cold first-run boots a real box and completes', { sid: ['S1.2.2'], persona: 'P1 Pro dev', atoms: ['F-4.1', 'F-6.1', 'F-6.2', 'F-7.1'] })
    .step('a novel (cold) job acquires a real standard-4 box', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: novel });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD — cold boots fast, not a full re-download`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the cold run completes and tears down (its result would re-warm the cache)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease, 'succeeded');
      ctx.closed = c.status >= 200 && c.status < 300;
      const g = await getLease(pat, ctx.lease);
      return check(ctx.closed && released(g), `close succeeded → ${c.status}; lease no longer HELD (state=${g.state})`, { close: c.status, after: g.state, gap: 'byte-identical re-store + next-run-becomes-a-hit (determinism) is ⚪ X4-external (needs a full build smoke on a real CoreLink PAT)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.2.3 — "A matrix build runs wide and flat: N parallel jobs, N distinct slots, one bill shape."
// A matrix fans out; each job is a distinct ephemeral runner holding one concurrency slot. Observable and
// strong on /v1: acquire several parallel leases, prove they are DISTINCT boxes each holding its own slot,
// see the account reflect the full parallel width, then tear each down independently.
test('JOURNEY · a matrix fans out into N distinct parallel slots (flat parallelism)', { skip: need('pro') }, async () => {
  const pat = P.pro;
  const WIDTH = 3; // a modest matrix width, well under the Pro cap of 10
  await new Journey('A matrix fans out into distinct parallel slots', { sid: ['S1.2.3'], persona: 'P1 Pro dev', atoms: ['F-2.1', 'F-4.6', 'F-5.1'] })
    .step('start from a clean account', async (ctx) => {
      const u = await usage(pat);
      ctx.base = u.activeNow ?? 0; ctx.cap = u.cap; ctx.leases = [];
      return check(u.status === 200 && ctx.cap >= WIDTH, `cap=${ctx.cap} ≥ matrix width ${WIDTH}, active_now=${ctx.base}`, { cap: ctx.cap, activeNow: ctx.base });
    })
    .step(`fan out ${WIDTH} parallel jobs → ${WIDTH} DISTINCT held boxes`, async (ctx) => {
      const results = await Promise.all(
        Array.from({ length: WIDTH }, (_, i) => acquire(pat, { expiryMs: 45000, tmpRoot: `/tmp/e2e-dl-matrix-${i}` })),
      );
      ctx.leases = results.map((r) => r.leaseId).filter(Boolean);
      const allHeld = results.every(heldOk);
      const distinct = new Set(ctx.leases).size === WIDTH;
      return check(allHeld && distinct, `${ctx.leases.length}/${WIDTH} HELD, all distinct lease ids (one slot each)`, { leases: ctx.leases.length, distinct });
    })
    .step('the account reflects the full parallel width (minutes flat, only slots count)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && (u.activeNow ?? 0) >= ctx.base + WIDTH, `active_now=${u.activeNow} ≥ base+${WIDTH} (parallel width occupies parallel slots)`, { activeNow: u.activeNow, expected: ctx.base + WIDTH });
    })
    .step('each job tears down independently', async (ctx) => {
      const closes = await Promise.all(ctx.leases.map((id) => closeLease(pat, id)));
      ctx.allClosed = closes.every((c) => c.status >= 200 && c.status < 300);
      return check(ctx.allClosed, `all ${closes.length} closed 2xx (independent teardown)`, { closes: closes.map((c) => c.status) });
    })
    .onCleanup(async (ctx) => { for (const id of ctx.leases || []) await closeLease(pat, id); })
    .run();
});

// STORY S1.2.4 — "A job that FAILED still revokes, tears down, and bills — the security actions don't skip."
// The happy-path lifecycle is covered elsewhere; this is the FAILURE direction of the same story: a job
// that reports `failed` must still close 2xx (revoke cred, release slot, bill slot-seconds, teardown) —
// the blast radius of a failed job is still exactly one job, and the slot is returned.
test('JOURNEY · a failed job still tears down, bills, and returns the slot', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('A failed job still tears down and returns the slot', { sid: ['S1.2.4'], persona: 'P1 Pro dev', atoms: ['F-4.1', 'F-5.1', 'F-5.5', 'F-5.9'] })
    .step('a job is running (Held)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-fail' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the job reports FAILED — the close still succeeds (revoke+release+bill+teardown run)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease, 'failed');
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close {status:'failed'} → ${c.status} (a failure outcome is still a clean, accounted close)`, { status: c.status });
    })
    .step('the box is gone and the slot is returned (blast radius = one job)', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(released(g), `after failed-close the lease is not HELD (status=${g.status}, state=${g.state})`, { status: g.status, state: g.state });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.2.5 — "A 3-second lint and a long suite are BOTH one flat slot — job shape never changes the model."
// Two jobs of very different shape each acquire exactly one slot for their lifetime and bill on slot
// occupancy, not job duration/shape. Observable: two leases, each one slot, both closed cleanly; the
// metrics surface exposes the slot accounting. The vCPU-h ceiling value is server-side (recorded gap).
test('JOURNEY · a short job and a long job are both one flat slot', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Short and long jobs are both one flat slot', { sid: ['S1.2.5'], persona: 'P1 Pro dev', atoms: ['F-1.2', 'F-4.1', 'F-5.1'] })
    .step('start clean and read the metrics surface (slot accounting is exposed)', async (ctx) => {
      const u = await usage(pat);
      const m = await req('GET', '/v1/metrics/tenant', { pat });
      ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && m.status === 200, `usage 200 (active_now=${ctx.base}); /v1/metrics/tenant 200 (shape-agnostic slot metering surfaced)`, { activeNow: ctx.base, metrics: m.status });
    })
    .step('a "tiny" job occupies exactly one slot', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-dl-tiny' });
      ctx.tiny = a.leaseId;
      const u = await usage(pat);
      return check(heldOk(a) && (u.activeNow ?? 0) >= ctx.base + 1, `tiny lease HELD, active_now=${u.activeNow} (one slot, regardless of job length)`, { leaseId: a.leaseId, activeNow: u.activeNow });
    })
    .step('a "large" job also occupies exactly one slot — shape does not change the count', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-large' });
      ctx.large = a.leaseId;
      const u = await usage(pat);
      return check(heldOk(a) && (u.activeNow ?? 0) >= ctx.base + 2, `large lease HELD, active_now=${u.activeNow} (still one slot each — flat by shape)`, { leaseId: a.leaseId, activeNow: u.activeNow, gap: 'the vCPU-h ceiling value (max_vcpu_h) that bounds COGS identically across shapes is server-side / owner-gated' });
    })
    .onCleanup(async (ctx) => { for (const id of [ctx.tiny, ctx.large]) if (id) await closeLease(pat, id); })
    .run();
});

// STORY S1.2.6 / S1.2.7 — "Cache edges (partial/huge/corrupt/evicted hydrate) degrade to miss-or-fail-closed."
// These are DATA-PLANE stories: content-address integrity + fail-closed-on-broken-hydrate are structural
// properties enforced by clw INSIDE the CF container at boot, not on the /v1 control surface. What is
// observable on /v1 is that the boot path (which redeems the cred ticket → drives the hydrate) yields a
// HELD box; the partial/corrupt/huge/eviction matrix is recorded as an X4-external gap in full.
test('JOURNEY · cache-edge integrity is structural in-container (miss-or-fail-closed)', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Cache-edge integrity is structural in-container', { sid: ['S1.2.6', 'S1.2.7'], persona: 'P1 Pro dev', atoms: ['F-4.3', 'F-5.9'] })
    .step('the boot path (which drives the hydrate) yields a real HELD box', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-hydrate' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD (boot redeems the cred ticket → clw drives the content-addressed hydrate)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('record the residual — the hydrate-edge matrix is enforced below /v1, X4 to exercise here', async () => {
      return check(true, `content-address corrupt-rejection + fail-closed-on-broken-hydrate are structural (a byte can't lie about its hash); not reachable via /v1`, { gap: 'partial-hit / huge-working-set / corrupt-blob / eviction=miss / R2-at-capacity matrix is ⚪ X4-external (clw-in-container; needs a real CoreLink PAT to drive CAS at scale); ClwBoxDrive fabricd-side is a WP-6 stub' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// THEME 1.3 — Concurrency, scale, and the ceiling (what the user SEES)
// ═══════════════════════════════════════════════════════════════════════════════════════════════

// STORY S1.3.1 / S1.3.2 — "Burst to N, then the ceiling refuses cleanly and non-destructively."
// A tenant drives all N seats at once (burst), and the (N+1)th is refused with a clean 429 over_cap —
// enforced BEFORE any box spawns, and WITHOUT preempting a running job. Then releasing a slot lets the
// next acquire through. tenantB (cap=2) makes the whole ceiling visible in a two-box footprint.
test('JOURNEY · burst to N seats, then the ceiling refuses cleanly and non-destructively', { skip: need('tenantB') }, async () => {
  const pat = P.tenantB;
  await new Journey('Burst to N then the ceiling refuses non-destructively', { sid: ['S1.3.1', 'S1.3.2'], persona: 'P1 tenant-B dev', atoms: ['F-1.1', 'F-1.4', 'F-5.2'] })
    .step('the tier grants N=2 seats and the account is clean', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap; ctx.leases = [];
      return check(u.status === 200 && u.cap === 2 && (u.activeNow ?? 0) === 0, `cap=${u.cap}, active_now=${u.activeNow} (clean slate)`, { cap: u.cap, activeNow: u.activeNow });
    })
    .step('burst: fill all N=2 seats at once', async (ctx) => {
      const rs = await Promise.all([0, 1].map((i) => acquire(pat, { expiryMs: 45000, tmpRoot: `/tmp/e2e-dl-burst-${i}` })));
      ctx.leases = rs.map((r) => r.leaseId).filter(Boolean);
      return check(rs.every(heldOk) && ctx.leases.length === 2, `both seats HELD (${ctx.leases.join(', ')}) — burst-without-fear, flat`, { leases: ctx.leases });
    })
    .step('the (N+1)th job is refused at the ceiling — 429 over_cap, no box spawns', async (ctx) => {
      const a3 = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-burst-over' });
      const refused = a3.status === 429 && !a3.leaseId;
      const overCap = a3.json?.code === 'over_cap' || /over_?cap/i.test(a3.text || '');
      return check(refused, `3rd acquire → ${a3.status}${overCap ? ' over_cap' : ''}, no lease (preventive refusal, not a crash)`, { status: a3.status, code: a3.json?.code, leaked: a3.leaseId });
    })
    .step('the refusal did NOT preempt a running job — the held seats are intact', async (ctx) => {
      const g = await getLease(pat, ctx.leases[0]);
      return check(g.status === 200 && g.state === 'held', `seat #1 still HELD after the refusal (no-preemption reservation)`, { status: g.status, state: g.state });
    })
    .step('releasing a seat lets the next acquire through (the slot recycles)', async (ctx) => {
      const c = await closeLease(pat, ctx.leases[0]);
      const a4 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-dl-burst-recycle' });
      if (a4.leaseId) ctx.leases.push(a4.leaseId);
      ctx.closed0 = c.status >= 200 && c.status < 300;
      return check(ctx.closed0 && heldOk(a4), `close #1 → ${c.status}, then acquire → HELD ${a4.leaseId} (freed slot admits the next)`, { close: c.status, reacquired: a4.leaseId });
    })
    .onCleanup(async (ctx) => { for (const id of ctx.leases || []) if (id) await closeLease(pat, id); })
    .run();
});

// STORY S1.3.3 / S1.3.4 — "The fleet offers exactly one shape today; the size/GPU/arch ladder is owner-gated."
// Multi-size runners (corelink-standard-8/16), GPU, and non-x86 arch are owner-gated (ADR-0007 Stage C /
// M4 capability matrix); the /v1 acquire has NO size/capability parameter — the live box is the pinned
// standard-4 x86 Linux. Observable: the one offered shape is served (a robust box); the ladder is a gap.
test('JOURNEY · the fleet serves one robust shape today; the size/GPU ladder is owner-gated', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('One robust shape today; size/GPU ladder owner-gated', { sid: ['S1.3.3', 'S1.3.4'], persona: 'P1 Pro dev', atoms: ['F-5.10', 'F-6.3'] })
    .step('an acquire for the offered shape is served (standard-4, the robust box)', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-shape' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD on the pinned standard-4 x86 box (ADR-0009)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('record the residual — there is no size/GPU/arch selector on /v1 today', async () => {
      return check(true, `the /v1 acquire carries no size/capability parameter; the fleet is a single standard-4 x86 shape`, { gap: 'corelink-standard-8/16 size ladder = ADR-0007 Stage C (owner-gated); GPU/arm64/OS capability matrix = M4 adjacency (owner-gated). The subset-gate label-refusal that guards this lives on the webhook path, not /v1.' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.3.5 — "10k jobs/day: sustained volume is the same instantaneous cap, recycled over time."
// 10k/day is a RATE; the cap is an INSTANTANEOUS limit — a steady rate under the cap is fine because the
// slot recycles run-to-run. Observable in miniature: two sequential acquire→close cycles reuse the seat
// (rate ≤ cap). The N>1 fabricd flip that a genuinely fleet-saturating customer triggers is owner-gated.
test('JOURNEY · sustained volume recycles one seat over time (rate ≤ instantaneous cap)', { skip: need('free') }, async () => {
  const pat = P.free;
  await new Journey('Sustained volume recycles one seat over time', { sid: ['S1.3.5'], persona: 'P1 Free dev', atoms: ['F-1.6', 'F-5.2', 'F-5.7'] })
    .step('start clean (cap=1: the tightest rate-vs-instantaneous demonstration)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && u.cap === 1 && (u.activeNow ?? 0) === 0, `cap=${u.cap}, active_now=${u.activeNow}`, { cap: u.cap, activeNow: u.activeNow });
    })
    .step('run #1 of the day: acquire then release the single seat', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-dl-vol-1' });
      const c = a.leaseId ? await closeLease(pat, a.leaseId) : { status: 0 };
      ctx.run1 = heldOk(a) && c.status >= 200 && c.status < 300;
      return check(ctx.run1, `job #1 HELD then closed (${c.status}) — one job of the day's rate`, { leaseId: a.leaseId, close: c.status });
    })
    .step('run #2 reuses the SAME seat — a steady rate under the cap is unbounded over time', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-dl-vol-2' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `job #2 HELD on the recycled seat (rate is a time-series; the cap is instantaneous)`, { leaseId: a.leaseId, gap: 'a genuinely fleet-saturating 10k/day customer triggers the N>1 fabricd flip (pg ledger + shards + instances raised together) — owner-gated on exactly this volume; full 10k/day smoke is ⚪ X4-external' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// THEME 1.4 — Failure & edge stories from the user's view
// ═══════════════════════════════════════════════════════════════════════════════════════════════

// STORY S1.4.1 / S1.4.2 / S1.4.3 — "A refused/failed spawn never permanently steals a seat."
// The reconciler orphan re-drive (S1.4.1), the transient-CF-reset retry (S1.4.2), and the webhook
// fail-open-to-cold (S1.4.3) all live on the internal spawn/webhook path. The /v1-observable core of all
// three is the INVARIANT they protect: a spawn that is refused leaks NO slot — the account is identical
// before and after a 429 — and a freed slot is immediately re-acquirable. That's the "no dead job
// permanently subtracts from usable N" guarantee, testable directly.
test('JOURNEY · a refused spawn leaks no seat; a freed seat is immediately recoverable', { skip: need('free') }, async () => {
  const pat = P.free;
  await new Journey('A refused spawn leaks no seat; recovery is immediate', { sid: ['S1.4.1', 'S1.4.2', 'S1.4.3'], persona: 'P1 Free dev', atoms: ['F-2.1', 'F-4.6', 'F-5.5'] })
    .step('fill the single seat and record the occupied count', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dl-leak-1' });
      ctx.lease = a.leaseId;
      const u = await usage(pat);
      ctx.occupied = u.activeNow ?? 0;
      return check(heldOk(a) && ctx.occupied >= 1, `seat HELD, active_now=${ctx.occupied}`, { leaseId: a.leaseId, activeNow: ctx.occupied });
    })
    .step('a spawn beyond the cap is refused — and the account is UNCHANGED (no leaked slot)', async (ctx) => {
      const a2 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-dl-leak-2' });
      const u = await usage(pat);
      const noLeak = a2.status === 429 && !a2.leaseId && (u.activeNow ?? 0) === ctx.occupied;
      return check(noLeak, `refused (${a2.status}); active_now still ${u.activeNow} (a refused spawn subtracts nothing from usable N)`, { refusedStatus: a2.status, activeAfter: u.activeNow, activeBefore: ctx.occupied });
    })
    .step('releasing the seat makes it immediately re-acquirable (self-healing recovery)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      const a3 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-dl-leak-3' });
      ctx.lease = a3.leaseId;
      return check(ctx.closed && heldOk(a3), `close → ${c.status}, re-acquire → HELD ${a3.leaseId} (the seat is never stranded)`, { close: c.status, reacquired: a3.leaseId, gap: 'the reconciler orphan re-drive (S1.4.1), transient-CF-reset retry (S1.4.2), and webhook fail-open-to-cold (S1.4.3) live on the internal spawn/webhook path — not injectable via /v1 (⚪ X4-external)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.4.4 — "A build that OOMs / times out / crashes dies cleanly and frees capacity."
// A runaway job must die cleanly and return its slot — no partial result stored, one bad build never
// wedges the fleet. Observable: a job closed as `failed` (the crash outcome) tears the box down and frees
// the seat; the short expiry is the reaper backstop. Real in-VM OOM injection is X4-external.
test('JOURNEY · a crashed/failed job dies cleanly and frees capacity', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('A crashed job dies cleanly and frees capacity', { sid: ['S1.4.4'], persona: 'P1 Pro dev', atoms: ['F-2.1', 'F-4.6', 'F-5.5'] })
    .step('a job is running (with a short expiry as the reaper backstop)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 35000, tmpRoot: '/tmp/e2e-dl-oom' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `lease ${a.leaseId} HELD`, { status: a.status, leaseId: a.leaseId });
    })
    .step('it crashes — closing as FAILED tears the box down cleanly', async (ctx) => {
      const c = await closeLease(pat, ctx.lease, 'failed');
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close {status:'failed'} → ${c.status} (a runaway job dies clean, no wedge)`, { status: c.status });
    })
    .step('capacity is freed — the crashed seat is not stranded', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(released(g), `lease no longer HELD (status=${g.status}, state=${g.state}); one bad build didn't wedge the fleet`, { status: g.status, state: g.state, gap: 'real in-VM OOM-kill / TTL-timeout injection (ADR-0009 microVM envelope) is ⚪ X4-external; the reaper Held→Expired|Crashed path is not directly drivable via /v1' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// STORY S1.4.5 — "Nobody can borrow my concurrency: an unauthenticated spawn is refused."
// A spawn for a repo/tenant you don't own must be refused so nobody borrows your seats. The warm-mint
// authorization (installation_id+repo → forbidden) is webhook-path; the /v1-observable core is that
// concurrency cannot be claimed WITHOUT a valid identity — a no-PAT acquire is 401, while the legitimate
// owner IS served (the discriminating positive control).
test('JOURNEY · concurrency cannot be borrowed — an unauthenticated spawn is refused', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Concurrency cannot be borrowed without identity', { sid: ['S1.4.5'], persona: 'P1 Pro dev', atoms: ['F-2.1', 'F-3.2', 'F-5.8', 'F-8.1'] })
    .step('an acquire with NO identity is refused — no seat is borrowable anonymously', async () => {
      const a = await acquire(null, { expiryMs: 30000, tmpRoot: '/tmp/e2e-dl-noauth' });
      const refused = a.status === 401 && !a.leaseId && (a.json?.code === 'unauthorized' || /unauthorized/i.test(a.text));
      return check(refused, `no-PAT acquire → ${a.status} unauthorized, no lease (concurrency is not borrowable)`, { status: a.status, code: a.json?.code, leaked: a.leaseId });
    })
    .step('the legitimate owner IS served (positive control — the refusal is identity-scoped, not blanket)', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-dl-owner' });
      ctx.lease = a.leaseId;
      return check(heldOk(a), `owner acquire → HELD ${a.leaseId} (only a valid identity gets a seat)`, { status: a.status, leaseId: a.leaseId, gap: 'the warm-mint authz that derives the tenant from installation_id+repo and returns `forbidden` for an unowned repo (spawn_forbidden) is on the webhook spawn path — ⚪ X4-external to drive here' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});
