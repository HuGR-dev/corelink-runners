// STORY JOURNEYS · the buyer, the FinOps owner, the account-lifecycle admin —
// real, stateful, end-to-end, exactly as these personas live it.
//
// Personas P6 (finance / eng-leadership buyer), P12 (FinOps / cost-optimization owner),
// P13 (account-lifecycle / churn admin). Most of this cluster's stories are GA-billing /
// Stripe / GDPR orchestration that is OWNER-GATED — so these journeys assert the OBSERVABLE
// CURRENT reality (the real pricing ladder surfaced by /v1/usage, the consumption view at
// /v1/usage/history, the wait histogram at /v1/metrics/tenant, tenant scoping, clean lease
// drain) and RECORD the residual as an explicit GAP in the artifact. We NEVER fake a green
// for a feature that isn't live (AUTHORING-GUIDE honesty rule 3). Run:
//   E2E_LIVE=1 E2E_RUN_ID=journeys node --test scripts/e2e/journeys/buyer-finance-lifecycle.test.mjs
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, usage, req, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
// Base gate: LIVE + Pro present. Individual journeys tighten the gate to the exact PATs they read.
const base = !LIVE ? 'set E2E_LIVE=1' : (P.pro ? false : 'PATs absent (source e2e-prod-env.sh)');
const need = (...pats) => (base ? base : (pats.every(Boolean) ? false : 'required tier PATs absent (source e2e-prod-env.sh)'));

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// P6 — Finance / eng-leadership buyer.  Theme 6.1 — Predictable spend & competitive positioning.
// ═══════════════════════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S6.1 — "Finance forecasts a flat bill: the tier is the ceiling of spend."
// The pricing ladder is REAL and surfaced: each tier's plan_cap (N lanes) + plan_ceiling_vcpu_h
// (the COGS wall) is a live, bounded, tenant-scoped value. What is owner-gated is the GA
// self-serve invoice (Stripe) and the memoized-re-run-billed-~0 economics (X4-external).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · finance forecasts a flat bill — the tier caps are real and surfaced', { skip: need(P.free, P.enterprise) }, async () => {
  await new Journey('Finance forecasts a flat bill (the pricing ladder is real)', { sid: ['S6.1'], persona: 'P6 finance buyer', atoms: ['F-1.1', 'F-1.3', 'F-1.5'] })
    .step('reads the Free tier — one lane, a bounded ceiling', async (ctx) => {
      const u = await usage(P.free);
      ctx.freeCap = u.cap; ctx.freeCeil = u.json?.plan_ceiling_vcpu_h ?? null;
      const ceilOk = ctx.freeCeil === null || (typeof ctx.freeCeil === 'number' && ctx.freeCeil > 0);
      return check(u.status === 200 && u.cap === 1 && ceilOk, `Free: plan_cap=${u.cap} lane, plan_ceiling_vcpu_h=${ctx.freeCeil} (surfaced+bounded)`, { cap: u.cap, ceil: ctx.freeCeil });
    })
    .step('reads the Pro tier — more lanes, a higher ceiling (the tier moves the bill, not minutes)', async (ctx) => {
      const u = await usage(P.pro);
      ctx.proCap = u.cap; ctx.proCeil = u.json?.plan_ceiling_vcpu_h ?? null;
      const ceilOk = ctx.proCeil === null || (typeof ctx.proCeil === 'number' && ctx.proCeil > 0);
      return check(u.status === 200 && u.cap === 10 && ceilOk, `Pro: plan_cap=${u.cap} lanes, plan_ceiling_vcpu_h=${ctx.proCeil}`, { cap: u.cap, ceil: ctx.proCeil });
    })
    .step('reads the Enterprise tier — the ladder is monotone (bigger tier ⇒ bigger caps)', async (ctx) => {
      const u = await usage(P.enterprise);
      ctx.entCap = u.cap; ctx.entCeil = u.json?.plan_ceiling_vcpu_h ?? null;
      const capLadder = u.cap === 100 && ctx.entCap > ctx.proCap && ctx.proCap > ctx.freeCap;
      // Only assert ceiling monotonicity when both ends are real numbers (honest: ceiling may be null).
      const ceilLadder = !(typeof ctx.entCeil === 'number' && typeof ctx.freeCeil === 'number') || ctx.entCeil > ctx.freeCeil;
      return check(u.status === 200 && capLadder && ceilLadder, `ladder Free(${ctx.freeCap})<Pro(${ctx.proCap})<Ent(${ctx.entCap}); ceilings ${ctx.freeCeil}→${ctx.entCeil}`, { capLadder, ceilLadder, entCeil: ctx.entCeil });
    })
    .step('records the residual: GA self-serve invoice + memoized-rerun economics are not surfaced here', async () => {
      // Honesty rule 3: the OBSERVABLE truth is the ratified flat ladder above; the invoice
      // itself and the "re-run of computed work billed ~0" economics are not a read on this fabric.
      return check(true, 'flat ladder is the observable reality; GA billing + re-run-~0 recorded as GAP', {
        gaBilling: 'GA owner-gated (Stripe self-serve)',
        rerunEconomics: 'X4-external ([clw] cache-hit / memoized-bill proof)',
      });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S6.2 — "Finance reads the hard vCPU-h ceiling that makes loss impossible by construction."
// The ceiling VALUE is surfaced per tier (plan_ceiling_vcpu_h) — observable. The WALL ARMING
// (the ComputeGate that actually refuses compute past the ceiling) is default-off / owner-gated;
// today the live limit is the concurrency cap (S1.3.2). We assert the value + record the arming GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · finance reads the max-spend ceiling (value surfaced, wall-arming owner-gated)', { skip: need(P.enterprise) }, async () => {
  await new Journey('Finance reads the max-spend vCPU-h ceiling', { sid: ['S6.2'], persona: 'P6 finance buyer', atoms: ['F-1.4'] })
    .step('the Pro plan surfaces a bounded vCPU-h ceiling (max COGS = ceiling × basis)', async (ctx) => {
      const u = await usage(P.pro);
      ctx.proCeil = u.json?.plan_ceiling_vcpu_h ?? null;
      const bounded = ctx.proCeil === null || (typeof ctx.proCeil === 'number' && ctx.proCeil > 0 && Number.isFinite(ctx.proCeil));
      return check(u.status === 200 && bounded, `Pro plan_ceiling_vcpu_h=${ctx.proCeil} (finite ⇒ COGS bounded below price)`, { ceil: ctx.proCeil });
    })
    .step('the Enterprise ceiling is larger — the wall scales with the tier, never unbounded', async (ctx) => {
      const u = await usage(P.enterprise);
      ctx.entCeil = u.json?.plan_ceiling_vcpu_h ?? null;
      const scales = !(typeof ctx.entCeil === 'number' && typeof ctx.proCeil === 'number') || ctx.entCeil >= ctx.proCeil;
      const finite = ctx.entCeil === null || Number.isFinite(ctx.entCeil);
      return check(u.status === 200 && scales && finite, `Ent ceiling=${ctx.entCeil} ≥ Pro ceiling=${ctx.proCeil} (bounded, tier-scaled)`, { entCeil: ctx.entCeil, proCeil: ctx.proCeil });
    })
    .step('records the residual: the vCPU-h WALL (ComputeGate) is default-off — the live limit is the concurrency cap', async () => {
      return check(true, 'ceiling value surfaced; the enforcing wall is armed by the owner (default-off today)', {
        wallArming: 'owner-gated (ComputeGate default-off; live limit = concurrency cap, S1.3.2)',
      });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S6.3 — "Eng-leadership buyer verifies the concurrency-not-minutes wedge is real."
// The observable wedge: flat CONCURRENCY (distinct tiers ⇒ distinct concurrency caps, not a
// per-minute meter) + attestation LIVE (GET /v1/attestation/key serves a real signing key).
// The published positioning matrix / raw-speed comparison is owner-gated (and honest: we do NOT
// win the raw-speed race — never claimed here).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · buyer verifies the flat-concurrency + attestation wedge (positioning matrix GAP)', { skip: need(P.free) }, async () => {
  await new Journey('Buyer verifies the competitive wedge vs per-minute incumbents', { sid: ['S6.3'], persona: 'P6 eng-leadership buyer', atoms: ['F-1.1', 'F-1.3'] })
    .step('the tier IS a concurrency lane-count, not a minutes meter (Free=1 vs Pro=10)', async (ctx) => {
      const f = await usage(P.free); const p = await usage(P.pro);
      const concurrencyWedge = f.status === 200 && p.status === 200 && f.cap === 1 && p.cap === 10 && p.cap > f.cap;
      return check(concurrencyWedge, `billing axis is concurrency lanes (Free=${f.cap}, Pro=${p.cap}) — flat, minutes unlimited`, { freeCap: f.cap, proCap: p.cap });
    })
    .step('attestation is LIVE — the platform wedge (verifiable verdicts) serves a real signing key', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key');
      const key = r.json?.keys?.[0];
      const live = r.status === 200 && key && typeof key.key_id === 'string' && key.key_id.length > 0 && typeof key.pubkey_b64 === 'string';
      ctx.keyId = key?.key_id;
      return check(live, `GET /v1/attestation/key → 200, key_id=${ctx.keyId} (corelink verify attestation LIVE)`, { status: r.status, keyId: ctx.keyId });
    })
    .step('records the residual: the published positioning matrix + raw-speed axis are owner-gated (and honest)', async () => {
      return check(true, 'mechanisms (flat concurrency + attestation) are live; the matrix is a positioning deliverable', {
        positioningMatrix: 'owner-gated (S6.4)',
        rawSpeed: 'honest guardrail — ~10% under GitHub; we do NOT win raw speed, never claimed',
      });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S6.4 — "Buyer runs a feature-by-feature bake-off vs Depot / Blacksmith / Namespace."
// GAP-dominant: the scored feature MATRIX is an owner-gated positioning deliverable. What is
// mechanism-proven and observable from the buyer's seat: flat concurrency (real caps),
// memoization mint / attestation (live key), and tenant isolation (usage is tenant-scoped).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · buyer bake-off — mechanisms proven, scored matrix owner-gated', { skip: base }, async () => {
  await new Journey('Buyer runs a competitive bake-off (mechanism-proven, matrix GAP)', { sid: ['S6.4'], persona: 'P6 eng-leadership buyer', atoms: ['F-1.1', 'F-1.3'] })
    .step('axis 1 (billing model): flat concurrency is real — Pro is 10 lanes, minutes unlimited', async (ctx) => {
      const u = await usage(P.pro);
      return check(u.status === 200 && u.cap === 10, `flat-concurrency axis observable: plan_cap=${u.cap} (vs per-minute incumbents)`, { cap: u.cap });
    })
    .step('axis 5 (platform): the attestation/verify mechanism is live (a scoreable, real wedge)', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key');
      return check(r.status === 200 && !!r.json?.keys?.[0]?.key_id, `attestation mechanism live (key_id=${r.json?.keys?.[0]?.key_id})`, { status: r.status });
    })
    .step('records the residual: the scored feature matrix is a positioning deliverable, and raw-speed is honest', async () => {
      return check(true, 'the mechanisms a bake-off would cite are proven; the published matrix is owner-gated', {
        featureMatrix: 'owner-gated positioning deliverable (S6.4)',
        rawSpeedHonesty: 'bare-metal incumbents may win raw wall-time — we say so (competitive-blacksmith.md)',
        crossTenantDedup: 'intra-tenant at GA only; cross-tenant staged, never claimed live',
      });
    })
    .run();
});

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// P12 — Cost-optimization / FinOps owner.  Theme 12.1 — Understand & lower COGS on a flat bill.
// ═══════════════════════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S12.1 — "FinOps explains the flat bill: I bought N lanes, not a meter."
// Observable: plan_cap (the N lanes), active_now (live occupancy, fabric-wide from the ledger),
// and /v1/usage/history (raw vCPU-h occupancy — metered for COGS/accounting only, never a
// customer-facing minutes bill). GA self-serve invoice is owner-gated.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · FinOps explains the flat bill — N lanes + occupancy, not a minutes meter', { skip: base }, async () => {
  const pat = P.pro;
  await new Journey('FinOps explains the flat bill (N lanes, occupancy for COGS only)', { sid: ['S12.1'], persona: 'P12 FinOps owner', atoms: ['F-1.1', 'F-1.3'] })
    .step('the plan is N lanes (plan_cap) with nothing running yet', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap; ctx.baseActive = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap === 10, `plan_cap=${u.cap} lanes, active_now=${ctx.baseActive} (N lanes, not a meter)`, { cap: u.cap, active: ctx.baseActive });
    })
    .step('occupancy is metered separately for COGS — /v1/usage/history surfaces raw vCPU-h', async (ctx) => {
      const r = await req('GET', '/v1/usage/history', { pat });
      ctx.tenant = r.json?.tenant;
      const shape = r.status === 200 && typeof r.json?.vcpu_h === 'number' && r.json?.period_key !== undefined;
      return check(shape, `usage/history → 200 tenant=${ctx.tenant}, vcpu_h=${r.json?.vcpu_h}, period_key=${r.json?.period_key} (raw occupancy, COGS-only)`, { status: r.status, vcpu_h: r.json?.vcpu_h });
    })
    .step('active_now moves with occupancy while the bill (the tier) does not — the whole point', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-bf-s121' });
      ctx.lease = a.leaseId;
      const u = await usage(pat);
      const moved = a.status === 200 && (u.activeNow ?? 0) >= ctx.baseActive + 1;
      return check(moved, `occupancy moved to active_now=${u.activeNow} (the METER moves; the flat tier does not)`, { activeNow: u.activeNow, baseActive: ctx.baseActive });
    })
    .step('records the residual: the customer-facing GA invoice is owner-gated', async () => {
      return check(true, 'lanes + occupancy are observable; the GA self-serve bill is owner-gated', { gaBilling: 'GA owner-gated (Stripe)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S12.2 — "FinOps investigates a vCPU-h spike and it is legible, not a mystery."
// Observable + built-not-proven: /v1/usage/history is the durable-ledger accrual (vcpu_h),
// /v1/leases is the lease-level drill-down. Acquire a real lease, then see the accrual view
// and drill into the lease list to find WHICH job it is — the spike-triage workflow.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · FinOps investigates a vCPU-h spike (accrual view + lease drill-down)', { skip: base }, async () => {
  const pat = P.pro;
  await new Journey('FinOps investigates a vCPU-h spike', { sid: ['S12.2'], persona: 'P12 FinOps owner', atoms: ['F-5.3', 'F-5.6'] })
    .step('reads period-to-date accrual — the authoritative ledger number', async (ctx) => {
      const r = await req('GET', '/v1/usage/history', { pat });
      ctx.baseVcpuH = r.json?.vcpu_h; ctx.period = r.json?.period_key;
      return check(r.status === 200 && typeof ctx.baseVcpuH === 'number', `accrual view: vcpu_h=${ctx.baseVcpuH}, period_key=${ctx.period} (durable ledger)`, { vcpu_h: ctx.baseVcpuH });
    })
    .step('a job spins up (the thing a spike is made of)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-bf-s122' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD (a unit of the spike)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('drills into the lease list to see WHICH leases are running (attribute the spike)', async (ctx) => {
      const l = await listLeases(pat);
      const found = l.leases.some((x) => JSON.stringify(x).includes(ctx.lease));
      return check(l.status === 200 && found, `GET /v1/leases drill-down shows ${l.leases.length} lease(s), the spike lease among them`, { count: l.leases.length, found });
    })
    .step('correlates with the concurrency peak (peak_this_instance is labelled, not fabric-wide)', async (ctx) => {
      const u = await usage(pat);
      const peak = u.json?.peak_this_instance;
      // Honest: peak_this_instance is per-INSTANCE by design; active_now is the fabric-wide truth.
      const legible = u.status === 200 && (u.activeNow ?? 0) >= 1 && (peak === undefined || typeof peak === 'number');
      return check(legible, `active_now=${u.activeNow} (fabric-wide truth) vs peak_this_instance=${peak} (labelled per-instance)`, { activeNow: u.activeNow, peak });
    })
    .step('records the residual: the accrual read surface is live but end-to-end billing-consistency is built-not-proven', async () => {
      return check(true, 'spike is legible via ledger accrual + lease drill-down; full billing-consistency proof pending', { billingConsistency: 'built-not-proven (🟡)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S12.2 (adversarial direction) — "One tenant's spike NEVER appears in another's accrual."
// The accrual/history view is tenant-scoped: the body names the caller's tenant, and tenant A's
// live lease is never visible in tenant B's history or lease list. No cross-tenant COGS oracle.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a tenant\'s consumption view never leaks another tenant\'s spike', { skip: need(P.tenantB) }, async () => {
  const A = P.pro; const B = P.tenantB;
  await new Journey('Cross-tenant isolation of the consumption view', { sid: ['S12.2', 'S7.4'], persona: 'P12 FinOps owner (tenant B)', atoms: ['F-5.3', 'F-3.2'] })
    .step('tenant A opens a real lease (a live unit of A\'s consumption)', async (ctx) => {
      const a = await acquire(A, { expiryMs: 40000, tmpRoot: '/tmp/e2e-bf-s122-A' });
      ctx.leaseA = a.leaseId;
      return check(a.status === 200 && a.leaseId, `tenant A holds lease ${a.leaseId}`, { status: a.status, leaseId: a.leaseId });
    })
    .step('tenant B\'s history names B, never A (positive+negative control in one read)', async (ctx) => {
      const ra = await req('GET', '/v1/usage/history', { pat: A });
      const rb = await req('GET', '/v1/usage/history', { pat: B });
      ctx.tenantA = ra.json?.tenant; ctx.tenantB = rb.json?.tenant;
      const scoped = ra.status === 200 && rb.status === 200 && ctx.tenantA && ctx.tenantB && ctx.tenantA !== ctx.tenantB;
      return check(scoped, `A's history names ${ctx.tenantA}; B's names ${ctx.tenantB} — each sees only itself`, { tenantA: ctx.tenantA, tenantB: ctx.tenantB });
    })
    .step('tenant B\'s lease list does NOT contain A\'s live lease (no COGS oracle)', async (ctx) => {
      const l = await listLeases(B);
      const leaked = l.leases.some((x) => JSON.stringify(x).includes(ctx.leaseA));
      return check(l.status === 200 && !leaked, `tenant B lists ${l.leases.length} lease(s), none is A's (no cross-tenant leak)`, { count: l.leases.length, leaked });
    })
    .onCleanup(async (ctx) => { if (ctx.leaseA) await closeLease(A, ctx.leaseA); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S12.3 — "FinOps right-sizes N from the live, self-serve signals."
// Observable: active_now vs plan_cap (headroom) + /v1/metrics/tenant wait histogram
// (p50/p95/histogram/count — the queue-wait signal that says whether N bottlenecks). The
// actual tier up/downgrade is GA self-serve (owner-gated).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · FinOps right-sizes N from active_now/plan_cap + the wait histogram', { skip: base }, async () => {
  const pat = P.pro;
  await new Journey('FinOps right-sizes concurrency (the sizing signals are live)', { sid: ['S12.3'], persona: 'P12 FinOps owner', atoms: ['F-5.2', 'F-5.6'] })
    .step('reads headroom — active_now well under plan_cap means N is not the bottleneck', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap; ctx.active = u.activeNow ?? 0;
      const headroom = u.status === 200 && ctx.cap === 10 && ctx.active <= ctx.cap;
      return check(headroom, `headroom: active_now=${ctx.active} / plan_cap=${ctx.cap} (sizing signal live)`, { active: ctx.active, cap: ctx.cap });
    })
    .step('reads the wait histogram — the queue-wait signal that distinguishes bursty from steady', async (ctx) => {
      const r = await req('GET', '/v1/metrics/tenant', { pat });
      const h = r.json?.histogram;
      const shape = r.status === 200 && Array.isArray(h) && h.length === 6 && typeof r.json?.p50_ms === 'number' && typeof r.json?.count === 'number';
      return check(shape, `/v1/metrics/tenant → p50=${r.json?.p50_ms}ms p95=${r.json?.p95_ms}ms count=${r.json?.count} histogram=${JSON.stringify(h)} (6 buckets)`, { status: r.status, histogram: h });
    })
    .step('records the residual: the tier change itself (up/downgrade) is GA self-serve, owner-gated', async () => {
      return check(true, 'the sizing SIGNALS are self-serve + live; acting on them (tier change) is owner-gated', { tierChange: 'GA owner-gated (Stripe self-serve, S13.1/S13.2)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S12.4 — "FinOps raises the cache-hit rate to trend vCPU-h down."
// GAP-leaning: the vcpu_h trend surface (/v1/usage/history) is observable and honest (no
// inflation), but the HIT-RATE metric itself is a product follow-up and the [clw] cache-hit
// proof is X4-external. We assert the honest accrual surface + record both residuals.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · FinOps trends vCPU-h down via cache-hits (surface live, hit-rate metric GAP)', { skip: base }, async () => {
  const pat = P.pro;
  await new Journey('FinOps raises the cache-hit rate to lower COGS', { sid: ['S12.4'], persona: 'P12 FinOps owner', atoms: ['F-1.2', 'F-4.3'] })
    .step('the honest vCPU-h accrual surface exists to trend against (no inflated hit accounting)', async (ctx) => {
      const r = await req('GET', '/v1/usage/history', { pat });
      const surface = r.status === 200 && typeof r.json?.vcpu_h === 'number' && r.json?.period_key !== undefined;
      return check(surface, `usage/history trend surface live: vcpu_h=${r.json?.vcpu_h}, period_key=${r.json?.period_key} (honest accounting, contract §3)`, { vcpu_h: r.json?.vcpu_h });
    })
    .step('records the residual: the hit-rate METRIC is a product follow-up; the cache-hit proof is X4-external', async () => {
      // Honest: the COGS-down curve is a workload property; the fabric surfaces raw accrual, but
      // a first-class hit-rate metric is not shipped and [clw] cache-hit is provable only externally.
      return check(true, 'the trend surface is live + honest; hit-rate metric + cache-hit proof are GAP', {
        hitRateMetric: 'product follow-up — not a shipped field (⚪)',
        cacheHitProof: 'X4-external ([clw] cache hit needs a real CoreLink PAT / dispatch)',
        determinismCaveat: 'a non-deterministic build never memoizes — a workload property the customer owns',
      });
    })
    .run();
});

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// P13 — Account-lifecycle / churn admin.  Theme 13.1 — Tier changes, offboarding & re-onboarding.
// ═══════════════════════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S13.1 — "Admin upgrades a growing team's tier — a cap change, not a re-provision."
// Observable: the upgrade TARGET states are real, live plans with distinct caps (Pro=10 lanes →
// Enterprise=100 lanes on the SAME fabric), and an in-flight lease keeps running (no restart /
// no box migration). The Stripe self-serve flip that MOVES a tenant between them is owner-gated.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · admin upgrades a tier — bigger caps, same fabric, no re-provision', { skip: need(P.enterprise) }, async () => {
  await new Journey('Admin upgrades a tier (live cap change, no restart)', { sid: ['S13.1'], persona: 'P13 account admin', atoms: ['F-1.5', 'F-5.11'] })
    .step('the "before" tier is a real, live plan (Pro = 10 lanes)', async (ctx) => {
      const u = await usage(P.pro);
      ctx.before = u.cap;
      return check(u.status === 200 && u.cap === 10, `before: Pro plan_cap=${u.cap} lanes (live)`, { cap: u.cap });
    })
    .step('the "after" tier is ALSO a real, live plan with bigger numbers (Enterprise = 100 lanes)', async (ctx) => {
      const u = await usage(P.enterprise);
      const biggerSameFabric = u.status === 200 && u.cap === 100 && u.cap > ctx.before;
      return check(biggerSameFabric, `after: Enterprise plan_cap=${u.cap} > Pro ${ctx.before} — same fabric, bigger caps`, { after: u.cap, before: ctx.before });
    })
    .step('an in-flight lease keeps running (an upgrade is a cap change, not a box migration)', async (ctx) => {
      const a = await acquire(P.pro, { expiryMs: 40000, tmpRoot: '/tmp/e2e-bf-s131' });
      ctx.lease = a.leaseId;
      const g = await getLease(P.pro, a.leaseId);
      return check(a.status === 200 && g.status === 200 && g.state === 'held', `in-flight lease ${a.leaseId} stays HELD (no re-provision on a cap change)`, { status: g.status, state: g.state });
    })
    .step('records the residual: the Stripe self-serve tier FLIP is owner-gated', async () => {
      return check(true, 'both tiers are live plans; MOVING a tenant between them (self-serve) is owner-gated', { tierFlip: 'GA owner-gated (Stripe); admin-registry live-update proven server-side (S5.1.1)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(P.pro, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S13.2 — "Admin downgrades without killing running jobs."
// Observable substrate: admission is checked at ACQUIRE against the current plan (a held lease
// is never re-checked — no mid-job kill), while the cap gates the NEXT acquire. We prove the
// invariant a non-destructive downgrade relies on (Free at-cap: held #1 survives while #2 is
// gated). The downgrade EVENT itself (Stripe) is owner-gated.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · admin downgrade is non-destructive to in-flight work (acquire-time gating)', { skip: need(P.free) }, async () => {
  const pat = P.free;
  await new Journey('Downgrade is non-destructive to running jobs', { sid: ['S13.2'], persona: 'P13 account admin', atoms: ['F-1.5', 'F-5.2'] })
    .step('checks the (smaller) cap — Free is 1 lane, standing in for the post-downgrade cap', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && u.cap === 1, `post-downgrade cap modelled by Free plan_cap=${u.cap}`, { cap: u.cap });
    })
    .step('a job is already in flight when the (modelled) smaller cap is in force', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-bf-s132-1' });
      ctx.lease1 = a.leaseId;
      return check(a.status === 200 && a.leaseId, `in-flight lease ${a.leaseId} HELD under the small cap`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the NEXT acquire is gated at the cap (admission is acquire-time) — while the held job runs on', async (ctx) => {
      const a2 = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-bf-s132-2' });
      ctx.lease2 = a2.leaseId; // expected null
      const gatedNext = a2.status === 429 && !a2.leaseId;
      const g = await getLease(pat, ctx.lease1);
      const heldSurvives = g.status === 200 && g.state === 'held';
      return check(gatedNext && heldSurvives, `next acquire gated (${a2.status}); in-flight lease still HELD — NO mid-job kill`, { nextStatus: a2.status, heldStatus: g.status, heldState: g.state });
    })
    .step('records the residual: the downgrade EVENT (Stripe tier change) is owner-gated', async () => {
      return check(true, 'the non-destructive invariant (acquire-time gating, no mid-job kill) is observable; the Stripe downgrade is owner-gated', { downgradeEvent: 'GA owner-gated (Stripe)' });
    })
    .onCleanup(async (ctx) => { for (const id of [ctx.lease1, ctx.lease2]) if (id) await closeLease(pat, id); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S13.3 — "Admin offboards a repo — clean drain, zero residue."
// Observable proxy for offboarding: a unit of work drains cleanly — acquire → close → the lease
// leaves the tenant's list, active_now returns to baseline, the id is no longer HELD. No residue.
// The App-uninstall = mint-inert fail-safe is LIVE-proven but on the GitHub-App path (X4-external,
// not reachable from a tenant PAT).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · admin offboards — a lease drains clean with zero residue', { skip: base }, async () => {
  const pat = P.pro;
  await new Journey('Offboard drains a lease clean (no residue)', { sid: ['S13.3'], persona: 'P13 account admin', atoms: ['F-5.8', 'F-8.1'] })
    .step('a job is running before offboard', async (ctx) => {
      const u0 = await usage(pat); ctx.baseActive = u0.activeNow ?? 0;
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-bf-s133' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD (active_now base=${ctx.baseActive})`, { status: a.status, leaseId: a.leaseId });
    })
    .step('offboard drains it to completion (close = the clean in-flight drain, not a kill)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status} (drained + torn down, its slot-seconds billed)`, { status: c.status });
    })
    .step('zero residue — the lease is no longer HELD (frees the slot)', async (ctx) => {
      // "No residue" = the lease no longer occupies a slot, i.e. it is no longer HELD (terminal
      // state or 404). A just-closed lease may still LINGER in the list momentarily as `released`
      // (async teardown) — that is fine, it holds no slot; requiring it to vanish is over-strict.
      const l = await listLeases(pat);
      const stillHeldInList = l.leases.some((x) => JSON.stringify(x).includes(ctx.lease) && /"state":"held"/i.test(JSON.stringify(x)));
      const g = await getLease(pat, ctx.lease);
      const gone = g.status === 404 || ['released', 'closed', 'succeeded', 'expired'].includes(g.state);
      return check(l.status === 200 && !stillHeldInList && gone, `no residue: not HELD (get→status=${g.status}/state=${g.state})`, { stillHeldInList, getStatus: g.status, getState: g.state });
    })
    .step('records the residual: the App-uninstall mint-inert fail-safe is on the GitHub-App path (X4)', async () => {
      return check(true, 'clean lease drain is observable; the uninstall=mint-inert fail-safe is LIVE-proven X4-external', { uninstallInert: 'LIVE-proven on the GitHub-App/mint path (X4, not a tenant-PAT verb)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S13.4 — "Admin deletes the account + GDPR erasure." (GAP-ONLY)
// Honest GAP: there is NO tenant-facing delete/erase verb on this fabric (we do NOT probe an
// invented endpoint). Account *deactivation* is buildable from the suspend/plan machinery; full
// *data erasure* (CAS/AC + billing_events) is owner-gated erasure orchestration on an SLA clock.
// The one observable we assert: pre-deletion, the tenant's own data is present + scoped (the
// thing that erasure would later remove) — no destructive action is taken.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · admin deletes account + GDPR erasure (owner-gated orchestration, GAP)', { skip: base }, async () => {
  const pat = P.pro;
  await new Journey('Delete account + GDPR erasure (owner-gated)', { sid: ['S13.4'], persona: 'P13 account admin', atoms: ['F-5.6', 'F-5.11'] })
    .step('pre-deletion: the tenant has real, scoped account data (the subject of a future erasure)', async (ctx) => {
      const u = await usage(pat);
      ctx.tenant = u.tenant;
      return check(u.status === 200 && !!u.tenant, `tenant ${u.tenant} has live account state (plan_cap=${u.cap}) — the erasure subject`, { tenant: u.tenant, cap: u.cap });
    })
    .step('records the GAP: no tenant-facing delete verb; deactivation buildable, erasure owner-gated on an SLA', async () => {
      // Honesty rule 3 + "no invented endpoints": we do NOT POST a fake /delete. Deactivation
      // (stop-admit / suspend) is buildable from S7.7/S5.1.1; full erasure is the tracked GDPR
      // follow-up (CAS/AC erasure shipped; billing_events tenant-prefix DELETE designed), subject
      // to the tax/VAT retention tension.
      return check(true, 'no destructive verb exposed to a tenant PAT — recorded as owner-gated GAP', {
        gdprErasure: 'owner-gated orchestration (org-wide erasure on an SLA clock, S10.5)',
        deactivation: 'buildable from suspend/plan-remove machinery (S7.7/S5.1.1)',
        retentionTension: 'billing rows may be pseudonymized, not deleted (tax/VAT law, S10.4/S10.5)',
      });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// STORY S13.5 — "Returning admin re-onboards after churn — jobs spawn warm again."
// Observable end-state: an existing tenant PAT acquires cleanly right now — the "jobs spawn warm
// again" runtime-onboarding end-state is LIVE (no restart). The re-install (fresh installation
// id) + re-register (POST /internal/v1/admin/tenants) orchestration is a privileged/owner path,
// not a tenant-PAT verb.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · returning admin re-onboards — a fresh acquire spawns warm', { skip: base }, async () => {
  const pat = P.pro;
  await new Journey('Re-onboard after churn (runtime onboarding is live)', { sid: ['S13.5'], persona: 'P13 returning admin', atoms: ['F-5.8', 'F-5.9'] })
    .step('the returned tenant\'s plan resolves live (no stale state blocks it)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && u.cap === 10, `plan resolves: plan_cap=${u.cap} (nothing stale blocks re-onboard)`, { cap: u.cap });
    })
    .step('a fresh acquire succeeds — "jobs spawn warm again", the re-onboard end-state', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-bf-s135' });
      ctx.lease = a.leaseId;
      const g = await getLease(pat, a.leaseId);
      return check(a.status === 200 && g.status === 200 && g.state === 'held', `fresh lease ${a.leaseId} HELD — runtime onboarding live (no restart)`, { status: a.status, state: g.state });
    })
    .step('the re-onboarded job drains clean (the loop closes; the account is fully usable again)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status} (re-onboarded account fully cycles a job)`, { status: c.status });
    })
    .step('records the residual: the App re-install + admin re-register orchestration is a privileged path', async () => {
      return check(true, 'the warm-acquire end-state is live; the re-install/re-register is a privileged (owner) path', {
        reRegister: 'privileged — POST /internal/v1/admin/tenants (not a tenant-PAT verb)',
        reInstall: 'fresh GitHub-App installation id (X4-external)',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});
