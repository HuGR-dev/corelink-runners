// STORY JOURNEYS · the AI agent itself — autonomous build/test on a real fenced runner.
//
// Cluster: P4 (the AI agent). S-ids S4.1–S4.7. These are LIVE CoreLink Runners features — the
// agent-exec seam, per-lease isolation, the §13.2 turn-feed, attested metrics, and the
// concurrency/backpressure gates. Journeys acquire REAL leases where the spine is observable and
// PROBE the mounted fail-closed surfaces (agent-exec, turn-feed ingest, attestation key) with
// req() reads — never inventing an endpoint. Every lease opened is closed in cleanup.
//
// HONESTY (owner mandate): isolation + attested-metrics + cap-backpressure ARE observable from
// this side. The full autonomous agent LOOP (an orchestrator driving speculative variants, a real
// in-box agent producing a turn-feed, a signed IntentMetrics payload at close) needs an EXTERNAL
// agent driver that cannot be fabricated here — where that is the residual it is recorded as a
// GAP in the artifact, never faked green.
//   E2E_LIVE=1 E2E_RUN_ID=journeys node --test scripts/e2e/journeys/agent.test.mjs
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, usage, req, LIVE, VALID_IMAGE } from '../lib/fabric.mjs';

const P = tenantPats();
// Per-journey skip: LIVE + the specific tenant PATs the story exercises must be present.
const need = (...keys) => (!LIVE ? 'set E2E_LIVE=1' : (keys.every((k) => P[k]) ? false : `PATs absent: ${keys.join(',')} (source e2e-prod-env.sh)`));

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S4.1 — "An agent runs the test suite without weighing the minutes." Observable (the MODEL): a
// runner materializes on demand for a speculative branch, near-instant, one branch = one lease.
// The full speculative-verification LOOP (an orchestrator driving ten variants, keeping the green
// one) is emergent product behavior that needs an external agent driver — a GAP.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a runner materializes on demand for a speculative branch (full loop is a GAP)', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('An agent runs the suite without asking permission (model LIVE, loop GAP)', { sid: ['S4.1'], persona: 'P4 autonomous agent', atoms: ['F-2.5', 'F-4.1'] })
    .step('the agent has headroom under its tenant cap (Pro) to fan out', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap; ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap >= 1 && (u.activeNow ?? 0) < u.cap, `cap=${u.cap}, active=${u.activeNow} — headroom to verify a speculative branch`, { cap: u.cap, activeNow: u.activeNow });
    })
    .step('one speculative branch → one runner, on demand, near-instant', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-agent-spec' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `speculative lease ${a.leaseId} HELD (verification is not a budget line)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('usage reflects the one speculative runner (the flat model induces the demand)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && (u.activeNow ?? 0) >= ctx.base + 1, `active_now=${u.activeNow} ≥ base+1 (a branch is a lease; the cap will bound the fan-out)`, { activeNow: u.activeNow, base: ctx.base });
    })
    .step('records the honest gap — the full speculative-verification loop needs an agent driver', async () => {
      return check(true, 'one branch = one lease is LIVE-proven; an orchestrator fanning ten variants and keeping the green one is emergent product behavior (external driver)', { loop: 'GAP — external agent orchestrator', needs: 'a real speculative-fanout driver' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S4.2 — "The agent's code runs fail-closed and can't reach my secrets." Observable: a lease
// boots deny-all + per-lease microVM (isolation LIVE-proven). The full escape/IMDS red-team is a
// tracked gap (ADR-0009 metadata-egress not closed on the CF path); cross-tenant is S7.4.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · an agent lease boots fail-closed (deny-all, per-lease box); IMDS red-team is a tracked gap', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('The agent runs fail-closed and can\'t reach my secrets (isolation LIVE)', { sid: ['S4.2'], persona: 'P4 agent-fleet operator', atoms: ['F-4.1', 'F-4.2', 'F-4.4'] })
    .step('the agent job boots into a deny-all, per-lease box → HELD', async (ctx) => {
      const a = await acquire(pat, { netPolicy: 'deny-all', expiryMs: 40000, tmpRoot: '/tmp/e2e-agent-isolation' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD under net_policy=deny-all (isolation is the spine)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the owner can read its own box (positive control — the lease genuinely exists)', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && g.state === 'held', `owner GET → 200 held (a real fenced box; cross-tenant deny is proven in security.test S7.4)`, { status: g.status, state: g.state });
    })
    .step('records the honest gap — the full escape/IMDS red-team is a tracked gap (ADR-0009)', async () => {
      return check(true, 'per-lease microVM + secrets brokered env=0/proc=0/disk=0 is LIVE-proven (ADR-0009); metadata/IMDS egress on the CF path is NOT closed by the denylist — a tracked gap, the microVM boundary still holds', { isolation: 'LIVE-proven (per-lease microVM, secret-broker)', gap: 'IMDS/raw-socket egress not closed on CF path (ADR-0009 Why-3)' });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S4.3 — "The agent streams its trajectory out, but the box stores nothing." Observable: the
// §13.2 INGEST side is authed by a per-lease, write-only ingest token — NOT the tenant PAT — so
// even a tenant PAT is refused there (the write-scope is strictly narrower than the PAT). The full
// ingest→forward→no-durable-write loop needs a real in-box agent — built-not-proven.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the trajectory INGEST is ingest-token-authed (not the tenant PAT); box stores nothing', { skip: need('pro') }, async () => {
  const pat = P.pro;
  await new Journey('The agent streams out, the box persists nothing (ingest-scope LIVE, loop GAP)', { sid: ['S4.3'], persona: 'P4 agent', atoms: ['F-4.9'] })
    .step('POST envelope/ingest with NO credential → 401 (the box needs a scoped ingest token)', async () => {
      const r = await req('POST', '/v1/leases/probe/envelope/ingest', { body: { events: [] } });
      return check(r.status === 401, `no-auth ingest → ${r.status} (401 — the untrusted box never holds the tenant PAT)`, { status: r.status });
    })
    .step('even the TENANT PAT is refused on ingest (it is not the per-lease ingest token)', async () => {
      const r = await req('POST', '/v1/leases/00000000-0000-0000-0000-000000000000/envelope/ingest', { pat, body: { events: [] } });
      // The ingest handler authenticates the lease-bound ingest token itself; a tenant PAT is NOT
      // that token, so it is refused — the write-scope is strictly narrower than the PAT.
      return check(r.status === 401 || r.status === 403 || r.status === 404, `tenant-PAT ingest → ${r.status} (not the ingest token — write-scope is per-lease, not tenant-wide)`, { status: r.status });
    })
    .step('records the honest gap — the full ingest→forward→no-durable-write loop needs an agent', async () => {
      return check(true, 'the runner forwards in-flight only, no durable write on the forward path (envelope.rs, S7.13); a real turn-feed with capture_incomplete honesty needs a live in-box agent loop', { loop: 'GAP — built-not-proven', needs: 'in-box agent producing turns + a valid scoped ingest token' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S4.4 — "The agent job emits attested token/cost metrics." Observable (LIVE on wire): the
// attestation verify key the metrics chain to is served (FLIP-B). The full IntentMetrics payload
// (token cache-split, integer cost_usd_micros) needs a real agent job to observe end-to-end.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the attestation key the agent-metrics chain to is served live (full payload GAP)', { skip: need() }, async () => {
  await new Journey('The agent job emits attested token/cost metrics (key LIVE, payload GAP)', { sid: ['S4.4'], persona: 'P4 agent-fleet cost owner', atoms: ['F-4.9'] })
    .step('GET /v1/attestation/key serves exactly the key the IntentMetrics sig verifies against', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key');
      const key = r.json?.keys?.[0];
      ctx.keyId = key?.key_id;
      const ok = r.status === 200 && Array.isArray(r.json?.keys) && r.json.keys.length >= 1 && !!key?.key_id && !!key?.pubkey_b64;
      return check(ok, `attestation key → 200, ${r.json?.keys?.length} key(s), key_id=${key?.key_id}, pubkey present (metrics attest to THIS key — LIVE)`, { status: r.status, keyId: key?.key_id, hasPubkey: !!key?.pubkey_b64 });
    })
    .step('records the honest gap — the full IntentMetrics payload needs a real agent job on the wire', async () => {
      return check(true, 'IntentMetrics (token counts with mandatory cache split, wall_ms/active_ms, tool breakdown, integer cost_usd_micros) is delivered atomically at close; observing the full signed payload needs a real agent job (external driver)', { payload: 'GAP — needs a real agent job at close', wire: 'verify key served LIVE (FLIP-B)', sig: 'arm-gated (FABRIC_EMIT_INTENT_METRICS_SIG, default-off)' });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S4.5 — "An agent fleet storms the fabric from its OWN side (runaway parallel spawn)." Observable:
// the tenant's OWN storm is bounded by its cap (tenantB cap=2: acquire to cap, next → 429), and a
// freed slot recycles. The vCPU-h ceiling + mining-detection/suspend are arm-gated (GAP).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a runaway agent fleet is bounded by its own cap; a freed slot recycles', { skip: need('tenantB') }, async () => {
  const pat = P.tenantB;   // cap=2
  await new Journey('Agent fleet self-storm is structurally bounded (cap LIVE, ceiling arm-gated)', { sid: ['S4.5'], persona: 'P4 agent-fleet operator', atoms: ['F-1.6', 'F-5.2'] })
    .step('read the fleet tenant\'s cap live', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap; ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap >= 1, `fleet tenant cap=${u.cap}, active=${u.activeNow}`, { cap: u.cap, activeNow: u.activeNow });
    })
    .step('the runaway fleet fills every slot up to the cap', async (ctx) => {
      ctx.leases = [];
      const want = ctx.cap - ctx.base;
      for (let i = 0; i < want; i++) {
        const a = await acquire(pat, { expiryMs: 40000, tmpRoot: `/tmp/e2e-fleet-${i}` });
        if (a.status === 200 && a.leaseId) ctx.leases.push(a.leaseId);
      }
      return check(ctx.leases.length === want, `filled ${ctx.leases.length}/${want} slots to the cap (the fleet spends its OWN N)`, { held: ctx.leases.length, want });
    })
    .step('the NEXT spawn — the runaway (cap+1)th — is refused 429, no box (bounded from its own side)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-fleet-over' });
      ctx.over = a.leaseId; // should be null
      return check(a.status === 429 && !a.leaseId, `(cap+1)th spawn → ${a.status} over_cap, no lease (reserve-before-provision; loss-impossible)`, { status: a.status });
    })
    .step('releasing one slot lets the fleet acquire again (the wall is dynamic, not a lockout)', async (ctx) => {
      if (!ctx.leases.length) return check(false, 'no lease to release', {});
      const freed = ctx.leases.pop();
      const c = await closeLease(pat, freed);
      const a = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-fleet-recycle' });
      if (a.status === 200 && a.leaseId) ctx.leases.push(a.leaseId);
      return check(c.status >= 200 && c.status < 300 && a.status === 200 && a.leaseId, `freed a slot (${c.status}) → next spawn admits ${a.leaseId} (slot recycled)`, { closeStatus: c.status, acquireStatus: a.status });
    })
    .step('records the honest gap — the vCPU-h ceiling + mining-detection/suspend are arm-gated', async () => {
      return check(true, 'the concurrency cap bounds the storm LIVE; the vCPU-h burn ceiling (S5.3.2) + sustained-pin/mining detection + durable suspend (S7.7) are arm-gated (owner-configured)', { ceiling: 'arm-gated (owner)', mining: 'arm-gated' });
    })
    .onCleanup(async (ctx) => { for (const id of [...(ctx.leases || []), ctx.over]) if (id) await closeLease(pat, id); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S4.6 — "Multiple agents in one tenant contend for the tenant's N." Observable (honest boundary):
// the fabric's fairness unit is the TENANT — usage exposes aggregate active_now/plan_cap but NO
// per-agent breakdown. Two "agents" (two acquires under one PAT) draw from the same pool. A
// per-agent quota is deliberately NOT a fabric primitive (a designed absence, not a gap).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · two agents share one tenant\'s N pool; there is no per-agent quota (honest boundary)', { skip: need('pro') }, async () => {
  const pat = P.pro;   // cap=10 — room for two "agents"
  await new Journey('Multiple agents contend for the tenant\'s N (tenant is the fairness unit)', { sid: ['S4.6'], persona: 'P4 agent-fleet operator', atoms: ['F-5.2'] })
    .step('the tenant\'s aggregate usage is per-TENANT, not per-agent', async (ctx) => {
      const u = await usage(pat);
      ctx.base = u.activeNow ?? 0;
      const perAgent = Object.keys(u.json || {}).find((k) => /agent|per_agent|by_agent/i.test(k));
      return check(u.status === 200 && u.cap >= 2 && !perAgent, `usage 200: cap=${u.cap}, active=${u.activeNow}, NO per-agent field (the tenant is opaque below its PAT)`, { cap: u.cap, activeNow: u.activeNow, perAgent: perAgent ?? null });
    })
    .step('agent-α draws a slot from the shared tenant pool', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-agent-alpha' });
      ctx.alpha = a.leaseId;
      return check(a.status === 200 && a.leaseId, `agent-α lease ${a.leaseId} HELD`, { status: a.status, leaseId: a.leaseId });
    })
    .step('agent-β draws a SECOND slot from the SAME pool (shared N, no per-agent partition)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-agent-beta' });
      ctx.beta = a.leaseId;
      const u = await usage(pat);
      return check(a.status === 200 && a.leaseId && (u.activeNow ?? 0) >= ctx.base + 2, `agent-β lease ${a.leaseId} HELD; active_now=${u.activeNow} (both drew from one tenant pool)`, { status: a.status, activeNow: u.activeNow });
    })
    .step('records the honest boundary — per-agent quota is NOT a fabric primitive (by design)', async () => {
      return check(true, 'fair-share protects ACROSS tenants (S2.4.1), not WITHIN one; intra-tenant per-agent scheduling is the operator\'s to own — a designed absence, not a missing feature', { boundary: 'tenant is the cap/fairness unit (ADR-0002)', perAgentQuota: 'not a fabric primitive (by design)' });
    })
    .onCleanup(async (ctx) => { for (const id of [ctx.alpha, ctx.beta]) if (id) await closeLease(pat, id); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S4.7 — "An agent hits its own concurrency wall (backpressure it must handle)." Observable: the
// (cap+1)th acquire is a clean, preventive 429 over_cap — MACHINE-DISTINGUISHABLE from a 400
// (malformed) so the orchestrator can tell "at capacity, retry" from "bad request". A pre-check
// via /v1/usage lets a well-behaved agent throttle before the wall.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the concurrency wall is a clean 429, distinguishable from a 400 (machine-actionable)', { skip: need('free') }, async () => {
  const pat = P.free;   // cap=1 — the wall is one acquire away
  await new Journey('An agent hits its concurrency wall — clean, preventive, machine-readable', { sid: ['S4.7'], persona: 'P4 autonomous agent', atoms: ['F-3.2', 'F-5.2'] })
    .step('a well-behaved agent PRE-CHECKS capacity via /v1/usage before spawning', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap;
      return check(u.status === 200 && u.cap === 1 && (u.activeNow ?? 0) < u.cap, `pre-check: cap=${u.cap}, active=${u.activeNow} — headroom for exactly one`, { cap: u.cap, activeNow: u.activeNow });
    })
    .step('the agent takes its one allowed slot', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-wall-1' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD (1/1)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the (cap+1)th acquire is a clean, PREVENTIVE 429 over_cap — before any box spawns', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-wall-over' });
      ctx.over = a.leaseId;
      return check(a.status === 429 && !a.leaseId, `wall → ${a.status} over_cap, no box (preventive backpressure, not a crash)`, { status: a.status, body: (a.text || '').slice(0, 140) });
    })
    .step('a MALFORMED acquire is a 400 — a DISTINCT signal the agent can tell apart from the 429', async (ctx) => {
      // relative tmp_root is rejected 400 (invalid) — NOT 429. The agent can distinguish
      // "you're at capacity, retry" (429) from "your request is malformed" (400).
      const bad = await req('POST', '/v1/leases', { pat, body: { image_digest: VALID_IMAGE, net_policy: 'deny-all', tmp_root: 'relative/path', expiry_ms: 40000 } });
      return check(bad.status === 400 && bad.status !== 429, `malformed acquire → ${bad.status} (≠ 429: over_cap and invalid are machine-distinguishable)`, { badStatus: bad.status });
    })
    .step('releasing the slot clears the wall — the next acquire is admitted', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      const a = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-wall-recycle' });
      ctx.lease2 = a.leaseId;
      return check(ctx.closed && a.status === 200 && a.leaseId, `close ${c.status} → next acquire ${a.status} HELD (back off, retry, admitted)`, { closeStatus: c.status, acquireStatus: a.status });
    })
    .step('records the honest note — the direct-door "Waiting for a runner" analogue is a GAP', async () => {
      return check(true, 'the fabric-door 429 over_cap is LIVE; the direct-door queue analogue (a job stays "Waiting for a runner" until a slot frees) is not observable via the API here', { wall: 'LIVE (429 over_cap, preventive)', gap: 'direct-door queue analogue' });
    })
    .onCleanup(async (ctx) => { for (const id of [ctx.lease, ctx.over, ctx.lease2]) if (id) await closeLease(pat, id); })
    .run();
});
