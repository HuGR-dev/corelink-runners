// STORY JOURNEYS · the platform operator (P5) — running the live fabric, honestly.
//
// The operator's day is READ-heavy: golden signals, per-tenant metering, boot-honest
// health, the vCPU-h ceiling value, the shape of the fleet. Those we assert against the
// REAL observable values on the live surface. The operator's *actions* — RAISE-N, arming
// the vCPU wall, a canary rollback, multi-region failover, secret rotation — are
// owner/config-gated or live on the spawn-worker data plane this PAT harness can't drive.
// For those we author an HONEST GAP: assert the adjacent live reality and RECORD the
// residual in the artifact. We NEVER fake a green for a feature that isn't live.
//
// Run: E2E_LIVE=1 E2E_RUN_ID=journeys node --test scripts/e2e/journeys/operator.test.mjs
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, closeLease, usage, req, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
// Per-journey skip: live + the PATs the story needs. Operator reads work with any real
// tenant PAT; the occupancy/X4 stories spawn one box, so they prefer the Pro cap (10).
const need = (...keys) =>
  !LIVE ? 'set E2E_LIVE=1' : (keys.every((k) => P[k]) ? false : `PATs absent (${keys.join(',')}) — source e2e-prod-env.sh`);

// ─────────────────────────────────────────────────────────────────────────────────────────────
// THEME 5.1 — Provisioning & onboarding
// ─────────────────────────────────────────────────────────────────────────────────────────────

// S5.1.1 — "Onboard a dogfood tenant on the static backend." The admin registry
// (POST /internal/v1/admin/tenants, FABRIC_ADMIN_KEY) is default-off and this harness has
// no admin key — so we assert the OBSERVABLE outcome of onboarding (an already-onboarded
// tenant is admittable NOW via the live CompositePlanSource) and prove the admin surface is
// fail-closed to a non-admin caller. GAP: the admin mutation itself is owner-gated.
test('JOURNEY · an onboarded tenant is admittable; the admin registry is fail-closed to us', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Dogfood onboarding — tenant admittable, admin registry fail-closed', { sid: ['S5.1.1'], persona: 'P5 operator', atoms: ['F-5.8', 'F-5.11'] })
    .step('the tenant resolves live through the CompositePlanSource (usage 200, real cap)', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap;
      return check(u.status === 200 && typeof u.cap === 'number' && u.cap > 0, `tenant ${u.tenant} is admittable — plan_cap=${u.cap} resolved with no restart`, { status: u.status, tenant: u.tenant, cap: u.cap });
    })
    .step('the admin onboarding surface is invisible/fail-closed to a non-admin caller', async () => {
      const r = await req('POST', '/internal/v1/admin/tenants', { pat, body: { tenant: 'e2e-should-never-land', plan_cap: 1 } });
      // No FABRIC_ADMIN_KEY here → default-off route is 404 (invisible) or 401 (fail-closed);
      // never 2xx. The mutation is owner-gated; we prove it does NOT admit us.
      const denied = r.status === 401 || r.status === 403 || r.status === 404;
      return check(denied, `POST /internal/v1/admin/tenants → ${r.status} (not admitted; owner-gated)`, { status: r.status, adminOnboard: 'owner-gated (FABRIC_ADMIN_KEY, default-off, absent here)' });
    })
    .run();
});

// S5.1.2 — "Point our own CI at runs-on: corelink-dogfood." The App is live (installation
// 144561227, owner-gated); full cache-hit smoke is X4-external. Observable: the mint/attest
// substrate the JIT-runner path depends on is LIVE (attestation key served).
test('JOURNEY · the dogfood mint/attest substrate is live; App + full smoke are gated', { skip: need('pro') }, async () => {
  await new Journey('Point CI at the fleet — mint substrate live, App owner-gated', { sid: ['S5.1.2'], persona: 'P5 operator', atoms: ['F-7.1'] })
    .step('the attestation key the JIT-runner path binds to is served (fabric ready to mint)', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key');
      const k = r.json?.keys?.[0];
      let bytes = -1;
      try { bytes = k?.pubkey_b64 ? Buffer.from(k.pubkey_b64, 'base64').length : -1; } catch { bytes = -1; }
      ctx.keyId = k?.key_id ?? null;
      return check(r.status === 200 && !!k?.key_id && bytes === 32, `attestation key ${k?.key_id} serves a 32-byte ed25519 pubkey (FLIP-B live)`, { status: r.status, keyId: k?.key_id ?? null, pubkeyBytes: bytes });
    })
    .step('record the residual: the App install and full cache-hit smoke are gated', async (ctx) => {
      return check(!!ctx.keyId, `substrate live under key ${ctx.keyId}; the on-ramp itself is gated`, { app: 'installation 144561227 — owner-gated (GitHub App live)', fullSmoke: 'X4-external (needs a real CoreLink PAT or hugit dispatch)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// THEME 5.2 — Capacity, scale, and the singleton→N>1 flip
// ─────────────────────────────────────────────────────────────────────────────────────────────

// S5.2.1 — "Read the golden-signal counters." The counters live behind
// GET /internal/v1/metrics + METRICS_OBSERVABILITY_KEY, a key this harness LACKS. So the
// positive control (reading the counters) is ABSENT — we can only prove the surface is
// fail-closed to us (default-off ⇒ 404 / mismatch ⇒ 401) and RECORD the gap. This is the
// honest version: we do not claim to have read counters we cannot reach.
test('JOURNEY · the golden-signal counter surface is fail-closed; positive control absent', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Golden-signal counters — fail-closed, obs-key absent', { sid: ['S5.2.1'], persona: 'P5 operator', atoms: ['F-7.2', 'F-10.1', 'F-10.2'] })
    .step('our tenant PAT CAN read the public tenant surface (positive control — auth works)', async () => {
      const u = await usage(pat);
      return check(u.status === 200, `GET /v1/usage → 200 (the PAT authenticates on the public surface)`, { status: u.status });
    })
    .step('the SAME PAT cannot read the obs-only counters — separate key, fail-closed', async () => {
      const r = await req('GET', '/internal/v1/metrics', { pat });
      // METRICS_OBSERVABILITY_KEY is not this PAT: unset ⇒ 404 (invisible), armed+mismatch ⇒ 401.
      const closed = r.status === 401 || r.status === 404;
      return check(closed, `GET /internal/v1/metrics → ${r.status} (obs-read ≠ the tenant PAT; fail-closed)`, { status: r.status, internalCounters: 'positive-control absent (no internal-auth key)' });
    })
    .run();
});

// S5.2.2 — "Raise the fleet cap / scale to N>1." OWNER-GATED (volume). We assert the live
// per-tenant cap (the singleton reality) and record what the flip requires. No fabricated
// N>1 green.
test('JOURNEY · the live per-tenant cap is real; RAISE-N is owner-gated', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Raise cap / scale to N>1 — owner-gated', { sid: ['S5.2.2'], persona: 'P5 operator', atoms: ['F-5.3', 'F-5.7', 'F-7.2'] })
    .step('read the tenant cap the singleton is enforcing right now', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap;
      return check(u.status === 200 && typeof u.cap === 'number' && u.cap >= 1, `plan_cap=${u.cap}, active_now=${u.activeNow} on the N=1 singleton`, { status: u.status, cap: u.cap, activeNow: u.activeNow });
    })
    .step('record the residual: N>1 is a coordinated env raise, owner-gated on volume', async (ctx) => {
      return check(typeof ctx.cap === 'number', `cap ${ctx.cap} enforced by the singleton; scaling out is owner-gated`, { raiseN: 'owner-gated (volume)', flip: 'DATABASE_URL + FABRIC_NUM_SHARDS + max_instances raised TOGETHER (#333 boot-authoritative shard count)' });
    })
    .run();
});

// S5.2.3 — "Load-shedding under saturation." Built-not-proven: the global in-flight limit
// sheds with 503 while /v1/health (mounted OUTSIDE the limiter) still answers. We can prove
// health is always-answerable; we CANNOT force global saturation from a capped tenant, so
// the shed itself is a GAP.
test('JOURNEY · health answers under load; forcing the global shed is out of reach', { skip: need('pro') }, async () => {
  await new Journey('Load-shed — health answerable, shed not provokable here', { sid: ['S5.2.3'], persona: 'P5 operator', atoms: ['F-5.1', 'F-5.2'] })
    .step('the liveness probe answers 200 (mounted outside the in-flight limiter)', async () => {
      const r = await req('GET', '/v1/health');
      return check(r.status === 200, `GET /v1/health → 200 (always-answerable; an LB can tell saturated-but-up from down)`, { status: r.status, body: (r.text || '').slice(0, 80) });
    })
    .step('record the residual: the load_shed 503 needs fleet-scale burst we cannot drive', async () => {
      return check(true, `health separates up-from-down; the shed path is built-not-proven from a capped tenant`, { loadShed: 'built-not-proven — cannot force the global in-flight limit within a single tenant cap', signal: 'load_shed / provision_capacity_503 is the fleet scale-out signal (S5.2.2)' });
    })
    .run();
});

// S5.2.4 — "Shard rebalancing / adding an instance at N>1." OWNER-GATED (N>1 flip). The
// scatter-gather surface (metrics/tenant) that at N>1 aggregates shards answers coherently
// today at N=1; the rebalance itself is gated.
test('JOURNEY · the shard-aggregating metrics surface answers at N=1; rebalance is gated', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Shard rebalance — surface coherent at N=1, flip owner-gated', { sid: ['S5.2.4'], persona: 'P5 operator', atoms: ['F-5.7', 'F-7.2'] })
    .step('the per-tenant metrics surface (Worker scatter-gathers shards at N>1) is coherent', async () => {
      const r = await req('GET', '/v1/metrics/tenant', { pat });
      const j = r.json || {};
      const ok = r.status === 200 && Array.isArray(j.histogram) && j.histogram.length === 6 && typeof j.count === 'number';
      return check(ok, `GET /v1/metrics/tenant → 200, 6-bucket histogram, count=${j.count} (single shard today)`, { status: r.status, count: j.count, p50_ms: j.p50_ms, p95_ms: j.p95_ms });
    })
    .step('record the residual: adding an instance is the coordinated, owner-gated flip', async () => {
      return check(true, `rebalance = a coordinated raise, not a hot re-shard; flip-time over-admit closed (#333)`, { shardRebalance: 'owner-gated (N>1 flip)', overAdmit: 'closed — boot-authoritative FABRIC_NUM_SHARDS (#333)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// THEME 5.3 — Billing & metering
// ─────────────────────────────────────────────────────────────────────────────────────────────

// S5.3.1 — "Meter slot-seconds → durable billing events." The metering is LIVE-observable:
// active_now reflects real occupancy the instant a box is HELD, and /v1/usage/history
// surfaces the durable vCPU·ms of record (12-month series, tenant-scoped). GAP: the usage
// PUSH exporter (to corelink-billing) is armed-off (BILLING_INGEST_URL unset).
test('JOURNEY · slot occupancy meters live and durable usage is surfaced; push is armed-off', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Meter slot-seconds — live occupancy + durable usage, push armed-off', { sid: ['S5.3.1'], persona: 'P5 operator', atoms: ['F-5.6'] })
    .step('read the occupancy baseline', async (ctx) => {
      const u = await usage(pat);
      ctx.base = u.activeNow ?? 0;
      return check(u.status === 200, `baseline active_now=${ctx.base}`, { status: u.status, base: ctx.base });
    })
    .step('acquire a runner — occupancy metering reflects it immediately', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-op-meter' });
      ctx.lease = a.leaseId;
      const u = await usage(pat);
      return check(a.status === 200 && (u.activeNow ?? 0) >= ctx.base + 1, `active_now rose to ${u.activeNow} (SlotMeter counts the held box)`, { status: a.status, leaseId: a.leaseId, activeNow: u.activeNow });
    })
    .step('the durable usage-history surface returns the tenant-scoped vCPU·ms of record', async (ctx) => {
      const r = await req('GET', '/v1/usage/history', { pat });
      const j = r.json || {};
      const ok = r.status === 200 && Array.isArray(j.periods) && j.periods.length === 12 && typeof j.vcpu_ms === 'number' && j.tenant;
      ctx.selfTenant = j.tenant;
      return check(ok, `GET /v1/usage/history → 200, ${j.periods?.length}-month series, vcpu_ms=${j.vcpu_ms}, tenant=${j.tenant}`, { status: r.status, tenant: j.tenant, vcpu_ms: j.vcpu_ms, periods: j.periods?.length, peak_this_instance: j.peak_this_instance });
    })
    .step('the history is the caller\'s OWN tenant only (no cross-tenant parameter exists)', async (ctx) => {
      const u = await usage(pat);
      return check(!!ctx.selfTenant && ctx.selfTenant === u.tenant, `usage/history tenant (${ctx.selfTenant}) == the caller's own (${u.tenant}) — no cross-tenant read`, { historyTenant: ctx.selfTenant, usageTenant: u.tenant, pushExporter: 'armed-off (owner) — BILLING_INGEST_URL unset ⇒ no push (durable table backs it)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// S5.3.2 — "Arm the loss-impossible vCPU-h wall." The ceiling VALUE is surfaced live
// (usage.plan_ceiling_vcpu_h) and usage/history.vcpu_ms is the accrual it is enforced
// against. GAP: the wall is armed-OFF today (FABRIC_RUNNER_VCPU unset) — the concurrency
// cap is the only live limit.
test('JOURNEY · the vCPU-h ceiling value + accrual are surfaced; the wall is armed-off', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Arm the vCPU-h wall — value surfaced, enforcement owner-gated', { sid: ['S5.3.2'], persona: 'P5 operator', atoms: ['F-1.4', 'F-5.6'] })
    .step('the plan\'s vCPU-h ceiling is surfaced on /v1/usage (value or null, honestly)', async (ctx) => {
      const r = await req('GET', '/v1/usage', { pat });
      const j = r.json || {};
      ctx.ceiling = j.plan_ceiling_vcpu_h ?? null;
      const present = 'plan_ceiling_vcpu_h' in j;
      return check(r.status === 200 && present, `plan_ceiling_vcpu_h=${ctx.ceiling} surfaced (null ⇒ no tier ceiling on file)`, { status: r.status, plan_ceiling_vcpu_h: ctx.ceiling });
    })
    .step('the accrual the ComputeGate would enforce against is the durable vcpu_ms', async () => {
      const r = await req('GET', '/v1/usage/history', { pat });
      const j = r.json || {};
      return check(r.status === 200 && typeof j.vcpu_ms === 'number', `period-to-date vcpu_ms=${j.vcpu_ms} (the value compute_accrued + Σ_reserved ≤ max_vcpu_h checks)`, { status: r.status, vcpu_ms: j.vcpu_ms, vcpuWall: 'armed-off (owner) — FABRIC_RUNNER_VCPU unset; the concurrency cap is the only live limit today' });
    })
    .run();
});

// S5.3.3 — "Anti-abuse: sustained-pin / mining detection." Built-not-proven. The FIRST
// abuse layer (the concurrency cap) is live + surfaced; the sustained-pin signal + durable
// fabric_suspended_tenants are not live-exercisable from this harness. Honest GAP with the
// loss-impossible ceiling as the economic floor.
test('JOURNEY · the concurrency-cap abuse layer is live; mining detection is built-not-proven', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Anti-abuse — cap layer live, mining detection built-not-proven', { sid: ['S5.3.3'], persona: 'P5 operator', atoms: ['F-1.6', 'F-5.6'] })
    .step('the concurrency cap (abuse layer #1) is a real, surfaced bound', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && typeof u.cap === 'number' && u.cap >= 1, `plan_cap=${u.cap} bounds parallel slots (layer #1 of the flat-model defense)`, { status: u.status, cap: u.cap });
    })
    .step('record the residual: the vCPU-h ceiling + mining detection are the deeper layers', async () => {
      return check(true, `cap bounds parallel burn; the ceiling + sustained-pin detection are layers #2/#3`, { miningDetection: 'built-not-proven — sustained-pin signal + durable fabric_suspended_tenants not live-exercisable here', ceiling: 'loss-impossible economic floor (S5.3.2, arm-gated)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// THEME 5.4 — Incident response & deploy ops
// ─────────────────────────────────────────────────────────────────────────────────────────────

// S5.4.1 — "Roll out with a boot-honest backend diagnostic." LIVE-proven. A bootable fabric
// is itself the proof: the cred-redemption boot guard (#332) REFUSES to boot without
// FABRIC_PUBLIC_BASE_URL, so a fabric answering authed traffic proves the redemption env is
// wired. We prove container-liveness (not just edge) via an authed read.
test('JOURNEY · the fabric is boot-honest — a live authed surface proves the boot guard passed', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Boot-honest diagnostic — bootable fabric = redemption env wired', { sid: ['S5.4.1'], persona: 'P5 operator', atoms: ['F-6.1', 'F-6.2', 'F-6.3', 'F-6.4', 'F-10.3'] })
    .step('the HTTP front answers (edge-or-container liveness)', async () => {
      const r = await req('GET', '/health');
      return check(r.status === 200, `GET /health → 200`, { status: r.status });
    })
    .step('an AUTHED read traverses the container — proves fabricd itself is live, not just the edge', async () => {
      const u = await usage(pat);
      return check(u.status === 200 && u.tenant, `GET /v1/usage → 200 for ${u.tenant} (container-traversing; the boot guard passed)`, { status: u.status, tenant: u.tenant, bootGuard: 'redemption-env boot guard live (#332) — a bootable fabric proves FABRIC_PUBLIC_BASE_URL is set (else it would fail loud at boot)' });
    })
    .step('the attestation key is served — the boot completed with a real signing key', async () => {
      const r = await req('GET', '/v1/attestation/key');
      const k = r.json?.keys?.[0];
      return check(r.status === 200 && !!k?.key_id, `attestation key ${k?.key_id} served (boot reached the key-serving stage)`, { status: r.status, keyId: k?.key_id ?? null });
    })
    .run();
});

// S5.4.2 — "Cut a misbehaving lease's egress without destroying it." LIVE (route) but on the
// SPAWN-WORKER data plane, keyed by a box handle + operator auth — not reachable from the
// public fabricd PAT read surface. Honest GAP: we assert the fabric-side isolation posture
// (default deny-all) is the live floor and record the egress-cutoff residual.
test('JOURNEY · the isolation floor is live; the egress kill-switch is a data-plane GAP', { skip: need('pro') }, async () => {
  await new Journey('Egress kill-switch — data-plane route, not on the public PAT surface', { sid: ['S5.4.2'], persona: 'P5 operator', atoms: ['F-4.2', 'F-7.2'] })
    .step('the fabric is up to accept operator controls (health 200)', async () => {
      const r = await req('GET', '/v1/health');
      return check(r.status === 200, `GET /v1/health → 200 (control plane reachable)`, { status: r.status });
    })
    .step('record the residual: egress-cutoff is a spawn-worker, box-handle, operator-authed control', async () => {
      return check(true, `the kill-switch is not exposed on the public tenant read surface`, { egressCutoff: 'GAP — POST /v1/egress-cutoff is a spawn-worker data-plane route, keyed by a box handle + operator auth; not exercisable from a tenant PAT', hardSever: 'teardown()/destroy() is the fail-closed control (raw-socket caveat, ADR-0009 Why-3)' });
    })
    .run();
});

// S5.4.3 — "Respond to the canary / roll back a deploy." OWNER-GATED (ops). Rollback is
// deterministic BECAUSE the live image is a pinned @sha256 digest — we prove the deployed
// binary is stable/identifiable (attestation key_id is stable across reads); re-pinning the
// prior digest is the revert. The rollback action itself is owner-gated.
test('JOURNEY · the live binary is stable/pinned (deterministic rollback); the roll-back is gated', { skip: need('pro') }, async () => {
  await new Journey('Canary / rollback — pinned binary observable, rollback owner-gated', { sid: ['S5.4.3'], persona: 'P5 operator', atoms: ['F-7.2', 'F-7.3', 'F-10.5'] })
    .step('read the deployed key identity once', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key');
      ctx.k1 = r.json?.keys?.[0]?.key_id ?? null;
      return check(r.status === 200 && !!ctx.k1, `key_id=${ctx.k1} (identifies the deployed, pinned binary)`, { status: r.status, keyId: ctx.k1 });
    })
    .step('read it again — a stable identity is what makes a rollback an exact, unambiguous revert', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key');
      const k2 = r.json?.keys?.[0]?.key_id ?? null;
      return check(r.status === 200 && k2 === ctx.k1, `key_id stable across reads (${k2}) — no mutable-tag drift; rollback = re-pin the prior @sha256`, { status: r.status, keyId: k2, rollback: 'owner-gated (ops) — re-pin the prior known-good digest' });
    })
    .run();
});

// S5.4.4 — "Rotate secrets." Built-not-proven. The design guarantee is SEPARATED keys, so a
// rotation never breaks an unrelated surface. We prove the separation is REAL: our tenant PAT
// reads the public surface (200) but is fail-closed on the obs surface (401/404) — different
// keys, independent rotation. The rotation ops themselves are not driven here.
test('JOURNEY · the secret surfaces are separated by design; rotation ops are not driven here', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Rotate secrets — separation observable, rotation built-not-proven', { sid: ['S5.4.4'], persona: 'P5 operator', atoms: ['F-10.4', 'F-10.5'] })
    .step('the tenant-PAT surface authenticates (one key family)', async () => {
      const u = await usage(pat);
      return check(u.status === 200, `GET /v1/usage → 200 (tenant PAT / introspect key)`, { status: u.status });
    })
    .step('the obs surface rejects the same PAT — a DIFFERENT key, so rotations are isolated', async () => {
      const r = await req('GET', '/internal/v1/metrics', { pat });
      const separated = r.status === 401 || r.status === 404;
      return check(separated, `GET /internal/v1/metrics → ${r.status} (obs-read ≠ the tenant surface; index.ts:960)`, { status: r.status, rotation: 'built-not-proven — separated keys (spawn-control, obs-read, mint, billing-ingest, App key) rotate independently; wrangler secret put per key' });
    })
    .run();
});

// S5.4.5 — "Upgrade the runner image (day-2, X4-pinned)." LIVE-observable: the
// verify-before-spawn floor rejects an UNPINNED image with a 400 BEFORE any box contact — a
// fat-fingered tag can't ship an unverified image. Positive control: a valid pinned image
// yields a real HELD lease (so the floor isn't a blanket reject).
test('JOURNEY · the X4 verify-before-spawn floor rejects an unpinned image; pinned is admitted', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Image upgrade — X4 verify-before-spawn rejects unpinned, admits pinned', { sid: ['S5.4.5'], persona: 'P5 operator', atoms: ['F-4.5', 'F-10.5'] })
    .step('an UNPINNED image (bare sha256, no repo) is refused at the supply-chain floor — no spawn', async () => {
      const a = await acquire(pat, { image: 'sha256:0000000000000000000000000000000000000000000000000000000000000000', expiryMs: 30000, tmpRoot: '/tmp/e2e-op-unpinned' });
      const rejected = a.status === 400 && !a.leaseId;
      return check(rejected, `unpinned image → ${a.status}, no lease (X4 rejects before box contact)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('a VALID content-pinned image (positive control) IS admitted to a HELD lease', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 30000, tmpRoot: '/tmp/e2e-op-pinned' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `pinned image → 200 HELD (the floor discriminates, not a blanket reject)`, { status: a.status, leaseId: a.leaseId, imageUpgrade: 'X4 verify-before-spawn LIVE; PINNED_IMAGE_DIGEST arm is owner-gated' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// S5.4.6 — "A bad deploy caught by the canary." Canary armed. The canary's ALERT-firing is
// not client-observable, but the fail-LOUD-at-boot floor it complements IS: a bootable
// fabric proves the boot guard (#332) + loud-logs (#327/#329) posture. Honest GAP on the
// alert itself.
test('JOURNEY · the fail-loud-at-boot floor is live; the canary alert is not client-observable', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Bad-deploy canary — fail-loud floor live, alert-firing a GAP', { sid: ['S5.4.6'], persona: 'P5 operator', atoms: ['F-7.3', 'F-10.3'] })
    .step('the fabric booted past its guards and serves authed traffic (no silent-cold degrade)', async () => {
      const u = await usage(pat);
      return check(u.status === 200, `GET /v1/usage → 200 (a silent-cold regression would have failed the boot guard, #332)`, { status: u.status });
    })
    .step('record the residual: the canary alert path is not observable from a client probe', async () => {
      return check(true, `the fabric fails LOUD at boot rather than relying on a human noticing slow jobs`, { canary: 'armed (HEAD f945a1f) — alert-firing not client-observable', floor: 'boot guard #332 + loud-logs #327/#329 turn silent-cold into a loud boot/log failure' });
    })
    .run();
});

// S5.4.7 — "Rotate the GitHub webhook secret with jobs in flight." Built-not-proven. The
// webhook HMAC lives on the spawn-worker/autoscaler, NOT the fabricd tenant surface — so a
// webhook-secret rotation cannot touch a tenant's live reads. We prove that isolation
// (tenant reads unaffected) and record the fail-safe-to-queued residual.
test('JOURNEY · tenant reads are isolated from the webhook secret; rotation self-heal is a GAP', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Rotate webhook secret — tenant surface isolated, self-heal built-not-proven', { sid: ['S5.4.7'], persona: 'P5 operator', atoms: ['F-5.8', 'F-10.4'] })
    .step('the tenant read surface is up and independent of any webhook secret', async () => {
      const u = await usage(pat);
      return check(u.status === 200, `GET /v1/usage → 200 (the webhook HMAC is a spawn-worker concern, not this surface)`, { status: u.status });
    })
    .step('record the residual: single-secret rotation is fail-safe-to-queued, not driven here', async () => {
      return check(true, `a webhook-secret rotation never touches the tenant read/obs/mint surfaces (separate keys)`, { webhookRotation: 'built-not-proven — single GITHUB_WEBHOOK_SECRET (no dual-overlap window today, index.ts:982-987)', selfHeal: 'a 401d queued webhook stays queued on GitHub + reconciler re-drive; a 401d completed is covered by the billing reconciler + reaper' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// THEME 5.5 — Multi-region ops (at N>1)
// ─────────────────────────────────────────────────────────────────────────────────────────────

// S5.5.1 — "A region outage." OWNER-GATED (M3 multi-region). Today is single-region
// singleton: a region outage IS a fabric outage, mitigated by fail-safe-to-queued + the
// watchdog. We prove the within-region liveness floor (health answerable) and record that
// cross-region failover is not built — never overclaim regional HA.
test('JOURNEY · within-region liveness is live; cross-region failover is owner-gated (M3)', { skip: need('pro') }, async () => {
  await new Journey('Region outage — within-region resilience live, multi-region gated', { sid: ['S5.5.1'], persona: 'P5 operator', atoms: ['F-5.5', 'F-7.2'] })
    .step('the single-region control plane answers liveness (the within-region floor)', async () => {
      const r = await req('GET', '/v1/health');
      return check(r.status === 200, `GET /v1/health → 200 (single-region singleton is up)`, { status: r.status });
    })
    .step('record the residual: there is NO cross-region failover today — honest tradeoff', async () => {
      return check(true, `a region outage today IS a fabric outage, mitigated by fail-safe-to-queued + the watchdog`, { multiRegion: 'owner-gated (M3) — no cross-region failover; single-region singleton (durable pg ledger is the prereq)', withinRegion: 'LIVE — spawn retry, reconciler re-drive, watchdog, load-shed' });
    })
    .run();
});

// S5.5.2 — "Cross-region billing reconciliation." Built-not-proven / multi-region. The
// durable usage numbers that a convergent-union billing_events table backs ARE observable
// and tenant-scoped; the multi-region DEPLOYMENT proof is owner-gated (M3).
test('JOURNEY · durable convergent usage is surfaced; multi-region billing proof is gated', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('Cross-region billing — durable union observable, multi-region gated', { sid: ['S5.5.2'], persona: 'P5 operator', atoms: ['F-5.6'] })
    .step('the durable, tenant-scoped usage the billing_events table backs is surfaced', async (ctx) => {
      const r = await req('GET', '/v1/usage/history', { pat });
      const j = r.json || {};
      const ok = r.status === 200 && typeof j.vcpu_ms === 'number' && Array.isArray(j.periods) && j.tenant;
      ctx.tenant = j.tenant;
      return check(ok, `GET /v1/usage/history → 200, durable vcpu_ms=${j.vcpu_ms} for ${j.tenant} (the exactly-once source)`, { status: r.status, tenant: j.tenant, vcpu_ms: j.vcpu_ms });
    })
    .step('record the residual: idempotent PK convergence is built; multi-region proof is gated', async () => {
      return check(true, `the durable table's PK + ON CONFLICT DO NOTHING make N regions converge to one row`, { crossRegionBilling: 'built-not-proven — convergent-union PK (tenant,lease_id,kind,at_ms) + region-tagged events', deployment: 'multi-region deployment proof owner-gated (M3; single-region today)' });
    })
    .run();
});
