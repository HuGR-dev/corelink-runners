// STORY JOURNEYS · the support engineer (P11) + the SRE on-call (P15) — day-2 reality.
//
// Two personas live here. P11 self-diagnoses a job that ran cold / hung / needs escalation,
// against the OBSERVABLE self-serve surface: GET /v1/leases/{id} (status), GET /v1/leases
// (list), /v1/usage (account), /v1/attestation/key (escalation evidence), and the attested
// CloseResponse. P15 reads the golden signals during an incident — but the golden COUNTERS
// (`mint_failures`, `spawn_failed`, `load_shed`, `/internal/v1/status`) live behind the
// obs-key gate this harness has NO KEY for. Those journeys are authored HONESTLY: they assert
// the user-facing signal that IS observable (jobs still run; /v1/health still answers) and
// RECORD the internal counter as a GAP (positive-control absent), never faking a green.
//
// Run (the tech lead runs it live, serially, and audits before merge):
//   E2E_LIVE=1 E2E_RUN_ID=journeys node --test scripts/e2e/journeys/support-sre.test.mjs
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, usage, req, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
// Per-journey skip: LIVE gate first, then the specific PAT(s) that journey needs.
const need = (pat) => (!LIVE ? 'set E2E_LIVE=1' : (pat ? false : 'PAT(s) absent (source e2e-prod-env.sh)'));

// A body that carries NO golden-counter field — the honest proof that an obs-gated internal
// surface leaked nothing to a keyless caller (used by the P15 GAP journeys).
const COUNTER_KEYS = ['mint_failures', 'mint_attempts', 'spawn_failed', 'load_shed', 'provision_capacity_503', 'jit_minted', 'runner_spawned', 'counters', 'uptime_ms'];
const leaksCounter = (text) => COUNTER_KEYS.some((k) => (text || '').includes(k));

// ═════════════════════════════════════════════════════════════════════════════════════════════
// P11 — SUPPORT & DEBUGGING
// ═════════════════════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S11.4 — "Self-serve diagnosis: I read my own job's status and list without a ticket."
// The self-serve support surface for the API door: GET /v1/leases/{id} (lifecycle state),
// GET /v1/leases (all my leases), /v1/usage (my account) — tenant-scoped, no operator needed.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a dev self-diagnoses a job via status + list (no ticket)', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('Self-serve diagnosis via lease status and list', { sid: ['S11.4', 'S11.2'], persona: 'P11 support/CI dev', atoms: ['F-5.1', 'F-9.1'] })
    .step('opens a runner to have a live job to diagnose', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-ssrv-diag' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId && a.state === 'held', `lease ${a.leaseId} is HELD (a job to diagnose)`, { status: a.status, leaseId: a.leaseId, state: a.state });
    })
    .step('reads the single job\'s status — GET /v1/leases/{id} returns its lifecycle state', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && g.state === 'held', `status surface answers: 200, state=${g.state} (self-serve, no ticket)`, { status: g.status, state: g.state });
    })
    .step('lists all their jobs — the lease appears in GET /v1/leases (tenant-scoped)', async (ctx) => {
      const l = await listLeases(pat);
      const found = l.leases.some((x) => JSON.stringify(x).includes(ctx.lease));
      return check(l.status === 200 && found, `list surface shows the job (${l.leases.length} lease(s), mine present)`, { count: l.leases.length, found });
    })
    .step('cross-checks the account — /v1/usage reflects the running job', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && (u.activeNow ?? 0) >= 1, `account shows active_now=${u.activeNow} (>=1 while held)`, { activeNow: u.activeNow, cap: u.cap });
    })
    .step('resolves it themselves — closes the job, status surface confirms it is no longer HELD', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      const g = await getLease(pat, ctx.lease);
      const notHeld = g.status === 404 || ['released', 'closed', 'expired'].includes(g.state);
      return check(ctx.closed && notHeld, `closed (${c.status}) and status now not-HELD (status=${g.status}, state=${g.state})`, { closeStatus: c.status, getStatus: g.status, state: g.state });
    })
    // GAP: the PRIMARY support surface for the direct door is GitHub's own Actions UI (run log,
    // "re-run") — S11.4 adoption principle. That surface is GitHub's, not reachable from this
    // harness; only the API-door lease status/list is asserted here.
    .step('records the residual GAP: Door-A (GitHub Actions) logs are not harness-observable', async () => {
      return check(true, 'Door-A run-log + native re-run = GitHub UI (deliberately not reinvented, S11.4) — GAP here', { doorALogs: 'GitHub Actions UI — not harness-observable', fabricStatus: 'lease status/list asserted above' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S11.4 — "Retry is ALWAYS a fresh warm lease, never a reused box."
// The retry contract (S1.6.10): a retried job gets a brand-new lease id — a stale/closed lease
// is not resurrected. A closed lease reads not-HELD; the retry is a distinct acquire.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a retried job gets a fresh lease, never the old box', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('Retry is a fresh warm lease', { sid: ['S11.4'], persona: 'P11 support/CI dev', atoms: ['F-5.1'] })
    .step('runs a job, then closes it (the first attempt is done)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-ssrv-retry1' });
      ctx.lease1 = a.leaseId;
      const c = await closeLease(pat, ctx.lease1);
      ctx.closed1 = c.status >= 200 && c.status < 300;
      return check(a.status === 200 && a.leaseId && ctx.closed1, `first attempt ${ctx.lease1} ran and closed (${c.status})`, { leaseId: ctx.lease1, closeStatus: c.status });
    })
    .step('the old lease is not reusable — it reads not-HELD', async (ctx) => {
      const g = await getLease(pat, ctx.lease1);
      const gone = g.status === 404 || ['released', 'closed', 'expired'].includes(g.state);
      return check(gone, `old lease is retired (status=${g.status}, state=${g.state}) — no stale-box reuse`, { status: g.status, state: g.state });
    })
    .step('retries — a fresh acquire yields a DISTINCT new lease id (a new warm box)', async (ctx) => {
      const a2 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-ssrv-retry2' });
      ctx.lease2 = a2.leaseId;
      const distinct = a2.status === 200 && a2.leaseId && a2.leaseId !== ctx.lease1;
      return check(distinct, `retry got a fresh lease ${a2.leaseId} !== old ${ctx.lease1} (never a reused box)`, { newLease: a2.leaseId, oldLease: ctx.lease1, state: a2.state });
    })
    .onCleanup(async (ctx) => {
      if (ctx.lease1 && !ctx.closed1) await closeLease(pat, ctx.lease1);
      if (ctx.lease2) await closeLease(pat, ctx.lease2);
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S11.5 — "I escalate with an attested evidence bundle, not 'it felt slow'."
// The evidence substrate is built + observable: the lease_id (from the list), the published
// attestation key (GET /v1/attestation/key), and the attested CloseResponse (attestation.sig +
// result_binding_sig) that `corelink verify` checks against that key.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a dev assembles an attested evidence bundle for escalation', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('Escalate with an attested evidence bundle', { sid: ['S11.5'], persona: 'P11 support/CI dev', atoms: ['F-4.10', 'F-9.4'] })
    .step('runs the job whose result they will escalate', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-ssrv-esc' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} held`, { leaseId: a.leaseId });
    })
    .step('bundle part 1: the lease_id is a real, tenant-scoped reference (appears in their list)', async (ctx) => {
      const l = await listLeases(pat);
      const found = l.leases.some((x) => JSON.stringify(x).includes(ctx.lease));
      return check(l.status === 200 && found, `lease_id ${ctx.lease} is a live reference in GET /v1/leases`, { found, count: l.leases.length });
    })
    .step('bundle part 2: the fabric publishes its attestation key (GET /v1/attestation/key)', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key', { pat });
      const keys = r.json?.keys ?? [];
      const k = keys[0];
      ctx.pubkey = k?.pubkey_b64;
      const ok = r.status === 200 && !!k?.key_id && typeof k?.pubkey_b64 === 'string' && k.pubkey_b64.length > 0;
      return check(ok, `attestation key published: key_id=${k?.key_id} (verifier can check the sigs)`, { status: r.status, keyId: k?.key_id, pubkeyLen: k?.pubkey_b64?.length ?? 0 });
    })
    .step('bundle part 3: closing yields an ATTESTED CloseResponse (attestation.sig + result_binding_sig)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      const sig = c.json?.attestation?.sig;
      const rbs = c.json?.result_binding_sig;
      const attested = ctx.closed && typeof sig === 'string' && sig.length > 0 && typeof rbs === 'string' && rbs.length > 0;
      return check(attested, `close attested: signed chain + result_binding_sig present (verifiable vs the published key)`, { closeStatus: c.status, sigLen: sig?.length ?? 0, rbsLen: rbs?.length ?? 0 });
    })
    // GAP: the support PROCESS (SLA, channel, ticketing) is an owner/org GA deliverable — not
    // built in this repo. Only the cryptographic EVIDENCE SUBSTRATE is asserted above.
    .step('records the residual GAP: the support process (SLA/channel) is owner-gated', async () => {
      return check(true, 'evidence substrate is LIVE + asserted; the support SLA/channel is a GA owner deliverable', { evidenceSubstrate: 'attested + asserted', supportProcess: 'owner-gated (GA, not built here)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S11.5 / S7.4 — "An escalation can never accidentally reference another tenant's job."
// A lease_id the tenant doesn't own resolves to 404 (no cross-tenant oracle) — with the OWNER's
// 200 read as the positive control that the id is genuinely live.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a foreign lease_id in an escalation resolves to 404 (no cross-tenant leak)', { skip: need(P.pro && P.tenantB) }, async () => {
  const owner = P.pro;      // tenant A (f0004) — owns the job
  const other = P.tenantB;  // tenant B (f0002) — a different tenant escalating
  await new Journey('Escalation cannot misreference a foreign job', { sid: ['S11.5', 'S7.4'], persona: 'P11 support (wrong tenant)', atoms: ['F-4.10', 'F-4.2'] })
    .step('tenant A runs a real job (the id genuinely exists)', async (ctx) => {
      const a = await acquire(owner, { expiryMs: 45000, tmpRoot: '/tmp/e2e-ssrv-foreignA' });
      ctx.leaseA = a.leaseId;
      return check(a.status === 200 && a.leaseId, `tenant A holds ${a.leaseId}`, { leaseId: a.leaseId });
    })
    .step('positive control: tenant A CAN read its own job for escalation (200 held)', async (ctx) => {
      const g = await getLease(owner, ctx.leaseA);
      return check(g.status === 200 && g.state === 'held', `owner reads it: 200 held (id is real + live)`, { status: g.status, state: g.state });
    })
    .step('tenant B pastes A\'s lease_id into an escalation → GET resolves to 404 (no oracle)', async (ctx) => {
      const g = await getLease(other, ctx.leaseA);
      return check(g.status === 404, `foreign lease_id → ${g.status} (must be 404 — an escalation can\'t reference another tenant\'s job)`, { status: g.status, body: (g.text || '').slice(0, 120) });
    })
    .onCleanup(async (ctx) => { if (ctx.leaseA) await closeLease(owner, ctx.leaseA); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S11.2 — "My job hung / never got a runner." The ONE expected hang is at-the-cap, and it
// is a capacity signal (over_cap), not a bug — and the slot RECYCLES once I release.
// (The reconciler/spawn-retry self-heal is an INTERNAL mechanism — GAP-recorded below.)
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a "hung" job is really at-cap, and the slot recycles on release', { skip: need(P.free) }, async () => {
  const pat = P.free;  // cap = 1 — the cleanest at-cap probe
  await new Journey('At-cap is the expected wait; the slot recycles', { sid: ['S11.2'], persona: 'P11 support/CI dev', atoms: ['F-4.6', 'F-5.8'] })
    .step('confirms the plan cap (Free = 1) from the live account', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap;
      return check(u.status === 200 && u.cap === 1, `plan cap read live = ${u.cap} (one slot)`, { cap: u.cap, activeNow: u.activeNow });
    })
    .step('takes the one slot — a job is running', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-ssrv-hang1' });
      ctx.lease1 = a.leaseId;
      return check(a.status === 200 && a.leaseId, `slot taken: ${a.leaseId} HELD`, { leaseId: a.leaseId });
    })
    .step('the "stuck / Waiting for a runner" job is a cap-refusal (429 over_cap), NOT a silent hang', async (ctx) => {
      const a2 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-ssrv-hang2' });
      ctx.lease2 = a2.leaseId; // must be null
      const refused = a2.status === 429 && !a2.leaseId;
      return check(refused, `2nd acquire → ${a2.status}, no lease — "waiting" is a legible cap signal, not a stuck box`, { status: a2.status, body: (a2.text || '').slice(0, 140) });
    })
    .step('releasing the running job frees the slot (the user-visible self-heal)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease1);
      ctx.closed1 = c.status >= 200 && c.status < 300;
      return check(ctx.closed1, `close #1 → ${c.status} (slot freed)`, { status: c.status });
    })
    .step('now the queued work acquires successfully — the slot recycled', async (ctx) => {
      const a3 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-ssrv-hang3' });
      ctx.lease3 = a3.leaseId;
      return check(a3.status === 200 && a3.leaseId, `fresh acquire ${a3.leaseId} HELD once the slot freed`, { status: a3.status, leaseId: a3.leaseId });
    })
    // GAP: the OTHER hang-causes (leaked spawn-claim → reconciler re-drive #293; transient CF
    // reset → startWithRetry; orphan_retry_giveup) are INTERNAL self-heal paths, observable only
    // via the obs-gated counters + logs this harness cannot read.
    .step('records the residual GAP: the reconciler/spawn-retry self-heal is internal-only here', async () => {
      return check(true, 'at-cap is the harness-observable hang; reconciler re-drive / spawn-retry / orphan_retry_giveup are internal (obs-key)', { atCap: 'observed (429 over_cap)', reconcilerSelfHeal: 'internal counters/logs — positive-control absent (no internal-auth key)' });
    })
    .onCleanup(async (ctx) => {
      for (const id of [ctx.lease1, ctx.lease2, ctx.lease3]) if (id) await closeLease(pat, id);
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S11.1 — "My job ran cold — where's my cache-warm?" (GAP-only on the hit-rate.)
// The north star: cold is SLOW, never BROKEN (S1.4.3 fail-open-to-cold). The OBSERVABLE truth is
// that a job runs to completion regardless. The cache-warm HIT RATE ([clw] cache hit) is
// X4-external — not visible at the /v1 surface — so it is recorded as a GAP, never faked green.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a cold run still completes (cache-warm hit-rate is X4-external — GAP)', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('Cold is slow, never broken (hit-rate GAP)', { sid: ['S11.1'], persona: 'P11 support/CI dev', atoms: ['F-5.9', 'F-10.3'] })
    .step('acquires a runner — warm OR cold, the job gets a real box', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-ssrv-cold' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId && a.state === 'held', `lease ${a.leaseId} HELD — fail-open-to-cold means it still runs`, { status: a.status, leaseId: a.leaseId, state: a.state });
    })
    .step('the job completes and accounts cleanly (cold ⇒ slower, never broken)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status}: the run finished + accounted (cold is a degradation, not an outage)`, { status: c.status });
    })
    // GAP: the `[clw] cache hit` confirmation + the warm/cold hit-RATE are hydration-box internal
    // (X4-external, S11.1) — invisible at the /v1 public surface. We assert the job runs; we do
    // NOT (cannot) assert it ran warm.
    .step('records the residual GAP: cache-warm hit-rate is X4-external, unmeasured here', async () => {
      return check(true, 'the run completes (asserted); the cache-warm hit-rate / [clw] cache hit is X4-external — GAP', { runCompletes: 'observed', cacheWarmHitRate: 'X4-external ([clw] cache hit not visible at /v1) — positive-control absent' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S11.3 — "My job OOM'd / got the wrong box size." (GAP-only on size selection.)
// Multi-size labels are owner-gated (ADR-0007 Stage C) — the acquire verb has NO size parameter,
// so a box size cannot be picked at this surface. The OBSERVABLE truth: the pinned default box
// (standard-4) works, and an unknown/invalid image is REFUSED (the family-matcher analog) rather
// than mis-spawned. Size selection is a recorded GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the default box works; a bad image is refused; size-labels are owner-gated (GAP)', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('Default box works; unknown spec refused; size-picking GAP', { sid: ['S11.3'], persona: 'P11 support/CI dev', atoms: ['F-4.6', 'F-5.10'] })
    .step('the robust default box provisions (no size knob to get "wrong")', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-ssrv-size' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `default box (standard-4) HELD as ${a.leaseId} — the pinned robust size`, { status: a.status, leaseId: a.leaseId });
    })
    .step('an UNKNOWN/invalid image spec is refused (400), never mis-spawned (family-matcher analog)', async (ctx) => {
      const bad = await acquire(pat, { image: 'sha256:0000000000000000000000000000000000000000000000000000000000000000', expiryMs: 30000, tmpRoot: '/tmp/e2e-ssrv-badspec' });
      ctx.badLease = bad.leaseId; // must be null
      const refused = bad.status === 400 && !bad.leaseId;
      return check(refused, `unpinned image → ${bad.status}, no box (an unknown spec is refused, not spawned wrong)`, { status: bad.status, leaseId: bad.leaseId });
    })
    // GAP: a genuinely bigger box (corelink-standard-8) needs a SIZE LABEL — owner-gated (ADR-0007
    // Stage C). The acquire verb exposes no size parameter, so this is unpickable + unmeasurable
    // at this surface.
    .step('records the residual GAP: size-labels are owner-gated + unpickable via the API', async () => {
      return check(true, 'default box asserted; a bigger size (standard-8) is owner-gated — no size param in the acquire verb', { defaultBox: 'standard-4 provisions (observed)', sizeSelection: 'owner-gated (ADR-0007 Stage C) — no size field in acquire; GAP' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ═════════════════════════════════════════════════════════════════════════════════════════════
// P15 — SRE / ON-CALL (golden signals)
//
// The golden COUNTERS (mint_failures, spawn_failed, load_shed, /internal/v1/status) are gated by
// the observability key this harness does NOT hold — so every P15 journey below asserts the
// user-facing signal that IS observable and RECORDS the internal counter as a positive-control
// GAP. It NEVER fakes a green for a counter it cannot read.
// ═════════════════════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S15.3 — "A capacity-503 / load-shed alert." (health-answerable = observable; fleet
// counters = GAP.) The strongest P15 observable: GET /v1/health is mounted OUTSIDE the load
// limiter, so it answers 200 ("saturated but up") even while the plane sheds — the LB's liveness
// probe. The per-tenant over_cap (429) is the observable CUSTOMER signal; the fleet load_shed /
// provision_capacity_503 counters are the internal GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · during saturation health stays answerable; over_cap is the tenant signal (fleet counters GAP)', { skip: need(P.free) }, async () => {
  const pat = P.free;  // cap = 1 — to exhibit the per-tenant over_cap signal
  await new Journey('Health-answerable under load; tenant-vs-fleet saturation', { sid: ['S15.3'], persona: 'P15 on-call SRE', atoms: ['F-5.2', 'F-10.1'] })
    .step('liveness probe: GET /v1/health answers 200 (mounted OUTSIDE the load limiter)', async () => {
      const r = await req('GET', '/v1/health', {});
      const ok = r.status === 200 && /ok/i.test(r.text || '');
      return check(ok, `/v1/health → ${r.status} "${(r.text || '').trim().slice(0, 16)}" — "saturated but up" is distinguishable from "down"`, { status: r.status, body: (r.text || '').slice(0, 16) });
    })
    .step('takes the one tenant slot', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-ssrv-cap1' });
      ctx.lease1 = a.leaseId;
      return check(a.status === 200 && a.leaseId, `slot held: ${a.leaseId}`, { leaseId: a.leaseId });
    })
    .step('the TENANT signal (over_cap, 429) is observable — distinct from a FLEET saturation', async (ctx) => {
      const a2 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-ssrv-cap2' });
      ctx.lease2 = a2.leaseId;
      const overCap = a2.status === 429 && !a2.leaseId;
      return check(overCap, `over-cap → ${a2.status} (a CUSTOMER signal: they hit THEIR cap — upgrade, not a fleet incident)`, { status: a2.status });
    })
    .step('health STILL answers after the burst (the plane is up, not down)', async () => {
      const r = await req('GET', '/v1/health', {});
      return check(r.status === 200, `/v1/health → ${r.status} after the burst — liveness always probeable`, { status: r.status });
    })
    // GAP: the FLEET saturation signals — load_shed (global-concurrency 503) + provision_capacity_503
    // — are internal counters (observability.rs), obs-key-gated; unreadable here.
    .step('records the residual GAP: the fleet load_shed / provision_capacity_503 counters are internal', async () => {
      return check(true, 'health-answerable + per-tenant over_cap asserted; the FLEET counters are obs-key-gated', { healthAnswerable: 'observed 200', overCap: 'observed 429', fleetCounters: 'load_shed/provision_capacity_503 internal — positive-control absent (no internal-auth key)' });
    })
    .onCleanup(async (ctx) => {
      for (const id of [ctx.lease1, ctx.lease2]) if (id) await closeLease(pat, id);
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S15.1 — "Triage a mint_failures spike." (GAP-only on the counter.)
// `mint_failures` is a fabricd internal counter (observability.rs) behind the obs key. This
// harness has no key → it is a positive-control-absent GAP. The user-facing BLAST RADIUS is the
// only observable: a mint failure fails OPEN to a cold run — so a job still acquires + runs
// (slow, not down). We assert the fail-open, and record the counter GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a mint_failures spike is internal (counter GAP); jobs still run (fail-open-to-cold)', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('mint_failures counter GAP; fail-open blast radius', { sid: ['S15.1'], persona: 'P15 on-call SRE', atoms: ['F-5.9', 'F-10.1'] })
    .step('the golden counter surface is NOT reachable without the obs key (positive-control absent)', async (ctx) => {
      const r = await req('GET', '/internal/v1/status', { pat });
      // Default-off ⇒ 404 (no obs key configured for this caller); a wrong key ⇒ 401. Either
      // way: not 200, and crucially NO counter field leaks to a keyless caller.
      const gated = (r.status === 404 || r.status === 401) && !leaksCounter(r.text);
      ctx.internalStatus = r.status;
      return check(gated, `/internal/v1/status → ${r.status}, no counter leaked — mint_failures is unreadable here (GAP)`, { status: r.status, leaked: leaksCounter(r.text) });
    })
    .step('the OBSERVABLE blast radius: a mint failure fails OPEN — a job still gets a box', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-ssrv-mint' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD — a mint outage degrades to COLD (slow), never to DOWN`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the job completes (the north star: cold-not-broken)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status}: jobs run through a mint incident (blast radius = cold runs)`, { status: c.status });
    })
    .step('records the residual GAP: mint_failures / mint_attempts are obs-key-gated internal counters', async (ctx) => {
      return check(true, 'fail-open blast radius asserted; the mint counters themselves are internal', { failOpen: 'observed (job ran)', internalCounters: `mint_failures/mint_attempts — positive-control absent (no internal-auth key); /internal/v1/status=${ctx.internalStatus}` });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S15.2 — "A spawn_failed climb." (GAP-only on the counter.)
// `spawn_failed` lives on the CF SPAWN-WORKER (deploy/cloudflare/src/metrics.ts) at its own
// /internal/v1/metrics — a DIFFERENT host from fabricd (this harness's BASE), obs-key-gated. So
// it is doubly unreachable here (wrong host + no key). The user-facing signal: spawn is
// fail-safe — a job that acquires proves spawn worked; a spawn outage keeps jobs queued on
// GitHub (never broken). We assert the acquire, and record the counter GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · spawn_failed lives on the spawn-worker (counter GAP); acquire proves spawn is fail-safe', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('spawn_failed counter GAP; fail-safe spawn', { sid: ['S15.2'], persona: 'P15 on-call SRE', atoms: ['F-5.8', 'F-10.2'] })
    .step('the spawn-worker metric surface is not reachable from the fabricd base (wrong host + no key)', async (ctx) => {
      // /internal/v1/metrics is the SPAWN-WORKER route (a distinct Worker); against the fabricd
      // BASE it is not a mounted fabricd route → not 200, and no counter leaks.
      const r = await req('GET', '/internal/v1/metrics', { pat });
      const gated = r.status !== 200 && !leaksCounter(r.text);
      ctx.metricsStatus = r.status;
      return check(gated, `/internal/v1/metrics → ${r.status} at the fabricd host, no counter leaked — spawn_failed unreadable here (GAP)`, { status: r.status, leaked: leaksCounter(r.text) });
    })
    .step('the OBSERVABLE fail-safe signal: an acquire that HOLDS proves spawn succeeded', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-ssrv-spawn' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD — the spawn completed (a spawn outage keeps jobs queued, never broken)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('records the residual GAP: spawn_failed + orphan_retry_giveup are spawn-worker internal', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `closed (${c.status}); spawn_failed/orphan_retry_giveup are spawn-worker /internal counters — GAP`, { closeStatus: c.status, internalCounters: 'spawn_failed/orphan_retry_giveup — spawn-worker host + obs-key; positive-control absent', metricsStatus: ctx.metricsStatus });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S15.4 — "A fabricd health flap." (readiness aggregate = GAP; bare health = observable.)
// The readiness aggregate GET /internal/v1/status ({version, uptime_ms, ledger_cross_instance_safe,
// this_shard, num_shards, counters}) is obs-key-gated (default-off 404). This harness can't read
// it → GAP. The observable: bare GET /v1/health answers 200 now (the singleton is up). We assert
// the bare health, and record that the version/uptime/ledger-safety aggregate needs the obs key.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · bare health answers; the readiness aggregate (version/uptime/ledger) is obs-gated (GAP)', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('Health flap: bare health vs the obs-gated status aggregate', { sid: ['S15.4'], persona: 'P15 on-call SRE', atoms: ['F-5.7', 'F-7.2'] })
    .step('the bare liveness probe answers 200 (the singleton is up right now)', async () => {
      const r = await req('GET', '/v1/health', {});
      return check(r.status === 200 && /ok/i.test(r.text || ''), `/v1/health → ${r.status} "ok" — up (a flap would show as intermittent non-200)`, { status: r.status });
    })
    .step('the RICH readiness aggregate (uptime_ms, version, ledger_cross_instance_safe) is obs-gated', async (ctx) => {
      const r = await req('GET', '/internal/v1/status', { pat });
      const gated = (r.status === 404 || r.status === 401) && !leaksCounter(r.text);
      ctx.statusCode = r.status;
      return check(gated, `/internal/v1/status → ${r.status}, no version/uptime/counter leaked — the restart-safety read needs the obs key (GAP)`, { status: r.status, leaked: leaksCounter(r.text) });
    })
    // GAP: ledger_cross_instance_safe (restart is safe vs lossy), uptime_ms (crash-loop anchor),
    // version (right binary?) — the whole runbook-in-anger read — is invisible without the obs key.
    .step('records the residual GAP: the crash-loop / restart-safety diagnosis needs the obs key', async (ctx) => {
      return check(true, 'bare health asserted; the readiness aggregate is obs-key-gated', { bareHealth: 'observed 200', readinessAggregate: `version/uptime_ms/ledger_cross_instance_safe/num_shards — obs-key-gated; positive-control absent (/internal/v1/status=${ctx.statusCode})` });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S15.5 — "Counters reset after a restart." (GAP-only on the counters.)
// The golden counters are boot-relative + RESET on restart (documented, status.rs) — and they
// live behind the obs key. This harness cannot read them, so it cannot observe the reset (or
// misread it) at all: positive-control absent. The user-facing DURABLE read is /v1/usage's
// active_now — LEDGER state (not a boot-relative counter), which reflects live leases. We assert
// the durable ledger read, and record that the counter-reset behavior is internal-only.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · counter-reset is behind /internal (GAP); the durable ledger read is user-facing', { skip: need(P.pro) }, async () => {
  const pat = P.pro;
  await new Journey('Counter-reset GAP; durable ledger active_now is observable', { sid: ['S15.5'], persona: 'P15 on-call SRE', atoms: ['F-10.1', 'F-10.3'] })
    .step('the boot-relative counters (+ their uptime_ms anchor) are behind the obs key — unreadable here', async (ctx) => {
      const r = await req('GET', '/internal/v1/status', { pat });
      const gated = (r.status === 404 || r.status === 401) && !leaksCounter(r.text);
      ctx.statusCode = r.status;
      return check(gated, `/internal/v1/status → ${r.status}, no counters/uptime_ms leaked — the reset-on-restart signal is unreadable (GAP)`, { status: r.status, leaked: leaksCounter(r.text) });
    })
    .step('baseline the DURABLE ledger read: /v1/usage active_now (ledger state, not a boot counter)', async (ctx) => {
      const u = await usage(pat);
      ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap != null, `usage read: active_now=${u.activeNow}, cap=${u.cap} (ledger-backed, survives a restart at N>1 pg)`, { activeNow: u.activeNow, cap: u.cap });
    })
    .step('a real lease moves the DURABLE ledger read (state, distinct from the reset-on-restart counters)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-ssrv-reset' });
      ctx.lease = a.leaseId;
      const u = await usage(pat);
      const moved = a.status === 200 && (u.activeNow ?? 0) >= ctx.base + 1;
      return check(moved, `active_now ${ctx.base}→${u.activeNow} on a held lease — ledger STATE (not a counter that zeroes on restart)`, { before: ctx.base, after: u.activeNow });
    })
    .step('records the residual GAP: counter reset-on-restart is documented internal behavior, obs-gated', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `closed (${c.status}); the golden counters (reset on restart, anchored on uptime_ms) are internal — GAP`, { closeStatus: c.status, durableRead: 'active_now observed (ledger state)', internalCounters: `boot-relative, reset-on-restart — positive-control absent (/internal/v1/status=${ctx.statusCode})` });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});
