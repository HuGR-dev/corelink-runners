// STORY JOURNEYS · developer workloads (persona P1) — the real workflow SHAPES a CI/build team
// throws at the fabric: docker, service containers, missing tools, egress, secrets,
// concurrency-cancel, reusable/matrix/monorepo, artifacts, flaky re-runs, long jobs, quotas, and
// the language ecosystems (bazel/nix/poetry/cargo). These acquire REAL leases and close them.
//
// HONEST FRAMING: at the /v1 lease/exec layer, most of these workloads reduce to the SAME
// observable — "a lease with the right image / net_policy / tmp_root / TTL is created (or
// refused), counts as one slot, and is torn down." The workload-specific machinery (a docker
// daemon in the microVM, the Actions services shim, cache-warm `[clw] hit`, the GitHub artifact
// store, matchManagedLabels on the webhook) lives INSIDE the box or on the Worker webhook path
// and is NOT reachable through /v1. Each journey therefore asserts the real /v1-observable
// behavior AND records the residual capability as an explicit GAP in the artifact — never a fake
// green for a feature that isn't live at this surface.
//
//   E2E_LIVE=1 E2E_RUN_ID=journeys node --test scripts/e2e/journeys/dev-workloads.test.mjs
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import { tenantPats, acquire, getLease, listLeases, closeLease, usage, req, LIVE, VALID_IMAGE, bodyLeaksSecret } from '../lib/fabric.mjs';

const P = tenantPats();
const skip = !LIVE ? 'set E2E_LIVE=1' : (P.pro ? false : 'Pro PAT absent (source e2e-prod-env.sh)');

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.7.4 — "The Rust/Go engineer's cargo workload runs on the dogfood box." 🟢 LIVE shape.
// This IS the fabric's own CI workload (a cargo workspace on the pinned standard-4 box). At /v1
// the observable is the lease lifecycle on that box; the warm `[clw] cache hit` is in-box.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · cargo/Go workload acquires the dogfood box, runs one slot, tears down', { skip }, async () => {
  const pat = P.pro;
  await new Journey('cargo/Go warm-cache workload on the dogfood box', { sid: ['S1.7.4', 'S1.6.3'], persona: 'P1 Pro dev', atoms: ['F-4.3', 'F-4.7'] })
    .step('checks the plan — Pro, records the baseline active count', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap; ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap === 10, `plan is Pro (cap=${u.cap}), base active=${ctx.base}`, { cap: u.cap, base: ctx.base });
    })
    .step('acquires the pinned standard-4 box (the cargo workload runs here) → HELD', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 45000, tmpRoot: '/tmp/e2e-dw-cargo' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD on the pinned image`, { status: a.status, leaseId: a.leaseId, image: VALID_IMAGE });
    })
    .step('the workload counts as exactly one slot (usage +1)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && (u.activeNow ?? 0) >= ctx.base + 1, `active_now=${u.activeNow} (base+1 while the cargo box is held)`, { activeNow: u.activeNow, base: ctx.base });
    })
    .step('releases the box → teardown (no box reuse across runs)', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status}`, {
        status: c.status,
        gap: 'warm `[clw] cache hit` on the cargo registry + target/ is IN-BOX (X4-external); toolchain-as-memo-axis (rust-toolchain.toml bump = new key) is the CAS mint path, not a /v1 verb',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.5.1 — "A security-conscious engineer verifies the verdict wasn't forged." 🟢 LIVE.
// The full crypto recompute is the client-side `corelink verify` CLI. The /v1-observable half:
// the CloseResponse carries a `result_binding_sig_v2` + `fabric_key_id`, and the attestation-key
// endpoint serves a matching key — so the verifier can fetch the right key and check v2.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · verify the verdict — close carries a v2 sig and the key endpoint serves its key', { skip }, async () => {
  const pat = P.pro;
  await new Journey('client-side verify: v2 result-binding sig + attestation key', { sid: ['S1.5.1'], persona: 'P1 security-conscious dev', atoms: ['F-4.10', 'F-9.4'] })
    .step('the attestation key endpoint serves the fabric public key (200, a real pubkey)', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key', { pat });
      const key0 = r.json?.keys?.[0] ?? null;
      ctx.servedKeyId = key0?.key_id ?? null;
      const hasPub = !!(key0?.pubkey_b64 && key0.pubkey_b64.length >= 40);
      return check(r.status === 200 && hasPub && !!ctx.servedKeyId, `GET /v1/attestation/key → 200, serves key_id=${ctx.servedKeyId} with a pubkey`, { status: r.status, keyId: ctx.servedKeyId, hasPub });
    })
    .step('acquires a runner and closes it — a no-result close still signs the empty v2 pre-image', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-verify' });
      ctx.lease = a.leaseId;
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      ctx.sigV2 = c.json?.result_binding_sig_v2 ?? null;
      ctx.closeKeyId = c.json?.fabric_key_id ?? null;
      return check(ctx.closed && typeof ctx.sigV2 === 'string' && ctx.sigV2.length > 0, `close → ${c.status}, result_binding_sig_v2 present (${(ctx.sigV2 || '').slice(0, 12)}…)`, { status: c.status, hasSigV2: !!ctx.sigV2, closeKeyId: ctx.closeKeyId });
    })
    .step("the close's key_id matches a key the attestation endpoint serves (verifier can fetch it)", async (ctx) => {
      const match = ctx.closeKeyId && ctx.servedKeyId && ctx.closeKeyId === ctx.servedKeyId;
      return check(!!match, `close.fabric_key_id (${ctx.closeKeyId}) === served key_id (${ctx.servedKeyId}) — the verify loop is wired`, {
        closeKeyId: ctx.closeKeyId,
        servedKeyId: ctx.servedKeyId,
        gap: 'the ed25519 pre-image recompute (memo_key‖stdout‖stderr‖exit‖artifacts) + ✓/✗ verdict is the client-side `corelink verify` CLI, not a /v1 verb; here we prove the sig+key are on the wire and consistent',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.7 / S1.6.8 / S1.7.5 — "A matrix over a monorepo fans out wide-and-flat." 🟡 built.
// The unit of spawn/billing is the workflow_job, not the workflow file — so reusable/composite/
// matrix/500-package are all just MORE independent leases, each one slot. The /v1-observable
// proof: N acquires = N distinct leases = N slots on the usage counter.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · matrix/monorepo fan-out — each job is its own independent lease + slot', { skip }, async () => {
  const pat = P.pro;
  await new Journey('workflow_job-granular matrix fan-out (each job = one lease + one slot)', { sid: ['S1.6.8', 'S1.6.7', 'S1.7.5'], persona: 'P1 monorepo dev', atoms: ['F-2.1', 'F-4.7'] })
    .step('records the baseline active count under the Pro cap', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap; ctx.base = u.activeNow ?? 0;
      return check(u.status === 200 && u.cap >= 2, `cap=${u.cap} headroom for a fan-out, base active=${ctx.base}`, { cap: u.cap, base: ctx.base });
    })
    .step('two matrix jobs (two affected packages) each acquire their OWN runner', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dw-mtx-a' });
      const b = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dw-mtx-b' });
      ctx.la = a.leaseId; ctx.lb = b.leaseId;
      const distinct = a.status === 200 && b.status === 200 && a.leaseId && b.leaseId && a.leaseId !== b.leaseId;
      return check(distinct, `two HELD leases, distinct ids (${a.leaseId} ≠ ${b.leaseId}) — granular, not one shared box`, { a: a.leaseId, b: b.leaseId });
    })
    .step('the fan-out shows up as two slots on the usage counter (not width-collapsed)', async (ctx) => {
      const u = await usage(pat);
      return check(u.status === 200 && (u.activeNow ?? 0) >= ctx.base + 2, `active_now=${u.activeNow} (base+2 — each job is a slot)`, {
        activeNow: u.activeNow,
        base: ctx.base,
        gap: 'the paths-filter affected-set + memo-key invalidation (499 warm, 1 recomputes) is webhook + CAS, not /v1; width>cap→429 queueing is the same admission gate proven in the S1.3.2 ceiling journey',
      });
    })
    .onCleanup(async (ctx) => { for (const id of [ctx.la, ctx.lb]) if (id) await closeLease(pat, id); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.10 — "A flaky test is re-run, cheaply — a fresh box, never a reused one." 🟢 re-run econ.
// A re-run is a NEW lease (ADR-0009 condition 1: no box reuse across runs). The observable proof:
// run #1 closes as `failed`, run #2 acquires a DIFFERENT lease id and closes `succeeded`.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · flaky test re-run — fresh lease per run, no box reuse', { skip }, async () => {
  const pat = P.pro;
  await new Journey('re-run economics: a failed run then a fresh warm re-run', { sid: ['S1.6.10'], persona: 'P1 Pro dev', atoms: ['F-1.2', 'F-4.3'] })
    .step('run #1 acquires a box, the flaky test fails → close as `failed`', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-rerun-1' });
      ctx.run1 = a.leaseId;
      const c = await closeLease(pat, ctx.run1, 'failed');
      ctx.closed1 = c.status >= 200 && c.status < 300;
      return check(a.status === 200 && ctx.closed1, `run #1 ${ctx.run1} HELD then closed failed (${c.status})`, { leaseId: ctx.run1, closeStatus: c.status });
    })
    .step('the engineer clicks re-run → a fresh lease, a DIFFERENT box (no reuse)', async (ctx) => {
      const a2 = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-rerun-2' });
      ctx.run2 = a2.leaseId;
      const fresh = a2.status === 200 && a2.leaseId && a2.leaseId !== ctx.run1;
      return check(fresh, `re-run ${ctx.run2} HELD, a NEW box (${ctx.run2} ≠ ${ctx.run1}) — never a reused container`, { run1: ctx.run1, run2: ctx.run2 });
    })
    .step('the re-run succeeds → close as `succeeded`', async (ctx) => {
      const c = await closeLease(pat, ctx.run2, 'succeeded');
      ctx.closed2 = c.status >= 200 && c.status < 300;
      return check(ctx.closed2, `re-run closed succeeded (${c.status})`, {
        status: c.status,
        gap: 'the memo HIT (identical inputs → ~0 recompute) + the determinism guard (a close claiming a memo_key its bytes do not match → 400 invalid) is the exec/close-with-result path, X4-external; here we prove the fresh-lease-per-run economics',
      });
    })
    .onCleanup(async (ctx) => {
      if (ctx.run1 && !ctx.closed1) await closeLease(pat, ctx.run1);
      if (ctx.run2 && !ctx.closed2) await closeLease(pat, ctx.run2);
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.14 — "A legitimately long job vs the lease TTL." 🟡 built.
// The caller sets the TTL that must cover the work (a durable absolute deadline_ms, ADR-0004);
// there is no silent extension. Observable: an acquire with a chosen expiry is HELD, and the
// documented zero-expiry edge is accepted (200) not rejected.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a long job sets a lease TTL that covers it (no silent extension)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('long job vs lease TTL — caller-set deadline, loud expiry', { sid: ['S1.6.14'], persona: 'P1 Pro dev', atoms: ['F-4.6', 'F-5.5'] })
    .step('a long job acquires a lease with a TTL chosen to cover the work → HELD', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dw-long' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD with a caller-set TTL (45s here; a real long job sets a longer one)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the lease is readable as held (the durable deadline is live, not yet reaped)', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && g.state === 'held', `GET → 200 held; the reaper enforces Held→Expired at the absolute deadline_ms`, {
        status: g.status,
        state: g.state,
        gap: 'the actual reap timing (Held→Expired, and exec-after-deadline → 400 zero-work) and the idle sleepAfter backstop (busy≠idle) are NOT waited-on here (would need a real expiry window); the deadline is durable per ADR-0004',
      });
    })
    .step('the documented zero-expiry edge is ACCEPTED (200), not rejected', async (ctx) => {
      const z = await acquire(pat, { expiryMs: 0, tmpRoot: '/tmp/e2e-dw-long-zero' });
      ctx.zlease = z.leaseId;
      return check(z.status === 200, `acquire expiry_ms:0 → ${z.status} (accepted per contract; self-reaps immediately)`, { status: z.status, leaseId: z.leaseId });
    })
    .onCleanup(async (ctx) => { for (const id of [ctx.lease, ctx.zlease]) if (id) await closeLease(pat, id); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.6 — "A concurrency-group cancels the in-progress run; the slot RETURNS." 🟡 built.
// A cancel is just an early `completed` → the teardown runs and the slot frees (cancellation
// returns concurrency, never leaks it). Observable: close a held lease, watch usage drop back,
// then re-acquire into the recycled slot.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · concurrency-group cancel returns the slot (not pinned on dead work)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('cancel-in-progress returns the slot promptly', { sid: ['S1.6.6'], persona: 'P1 Pro dev', atoms: ['F-4.6', 'F-5.5'] })
    .step('push A spawns a runner (slot held)', async (ctx) => {
      const u0 = await usage(pat); ctx.base = u0.activeNow ?? 0;
      const a = await acquire(pat, { expiryMs: 45000, tmpRoot: '/tmp/e2e-dw-cancel-a' });
      ctx.leaseA = a.leaseId;
      const u1 = await usage(pat);
      return check(a.status === 200 && (u1.activeNow ?? 0) >= ctx.base + 1, `run A ${a.leaseId} HELD, active_now=${u1.activeNow}`, { leaseId: a.leaseId, active: u1.activeNow });
    })
    .step('push B supersedes A → A is cancelled (an early terminal close) → slot frees', async (ctx) => {
      const c = await closeLease(pat, ctx.leaseA, 'failed'); // a cancelled run is a terminal completed
      ctx.closedA = c.status >= 200 && c.status < 300;
      const u = await usage(pat);
      const returned = (u.activeNow ?? 0) <= ctx.base; // slot returned toward baseline
      return check(ctx.closedA && returned, `A closed (${c.status}); active_now=${u.activeNow} back to ≤ base ${ctx.base} — slot RETURNED, not leaked`, {
        closeStatus: c.status,
        active: u.activeNow,
        gap: 'the real cancel path (webhook workflow_job.completed[cancelled] → revoke PAT → release → teardown, idempotent) is the Worker index.ts, not /v1; a terminal close models the same slot-return',
      });
    })
    .step('B immediately acquires the recycled slot', async (ctx) => {
      const b = await acquire(pat, { expiryMs: 30000, tmpRoot: '/tmp/e2e-dw-cancel-b' });
      ctx.leaseB = b.leaseId;
      return check(b.status === 200 && b.leaseId, `run B ${b.leaseId} HELD in the freed slot`, { status: b.status, leaseId: b.leaseId });
    })
    .onCleanup(async (ctx) => {
      if (ctx.leaseA && !ctx.closedA) await closeLease(pat, ctx.leaseA);
      if (ctx.leaseB) await closeLease(pat, ctx.leaseB);
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.4 / S1.6.15 — "A job needs egress / reaches a private registry." 🟡 built / 🔵 policy-gated.
// Egress is net_policy-shaped, fail-closed by default. The /v1-observable half: the lease carries
// a net_policy and a deny-all lease is HELD (the honest current posture). Allow-list proxying,
// VPN reach, and the IMDS denylist are policy-gated / partial — recorded as GAPs.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · egress is policy-shaped — a fail-closed deny-all lease holds', { skip }, async () => {
  const pat = P.pro;
  await new Journey('net_policy-shaped egress (fail-closed default; allow-list is policy-gated)', { sid: ['S1.6.4', 'S1.6.15'], persona: 'P1 Pro dev', atoms: ['F-4.2', 'F-6.3'] })
    .step('a job acquires a lease under the fail-closed default net_policy (deny-all) → HELD', async (ctx) => {
      const a = await acquire(pat, { netPolicy: 'deny-all', expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-egress' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD under net_policy=deny-all (default closed posture)`, { status: a.status, leaseId: a.leaseId, netPolicy: 'deny-all' });
    })
    .step('the lease is the owner-scoped box the (proxied) egress would apply to', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && g.state === 'held', `owner reads its held lease (200) — net_policy is a lease-carried contract`, {
        status: g.status,
        state: g.state,
        gap: 'allow-list host PROXYING (a private registry / license server reachable while the rest stays closed via the SDK egress proxy setDeniedHosts), the brokered service credential (env-0), the VPN/private-peering CAPABILITY GAP (hybrid, S1.3.4), and the partial IMDS denylist (G2, raw-socket bypass) are all in-box / policy-gated — not observable at /v1',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.5 — "A brokered secret never lands on the box (env-0)." 🟢 broker LIVE.
// The CAS PAT is redeemed once at boot via POST /v1/leases/{id}/cas-cred with a lease-bound
// single-use ticket; the tenant PAT never reaches the box. Observable + fail-closed proof: a
// FORGED ticket on a genuinely-held lease is rejected (401 invalid / 404 no-oracle), and the
// response NEVER contains a credential.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the cred broker rejects a forged cas-cred ticket, hands out nothing', { skip }, async () => {
  const pat = P.pro;
  await new Journey('env-0 secret broker: forged cred-ticket is fail-closed', { sid: ['S1.6.5'], persona: 'P1 Pro dev', atoms: ['F-4.2', 'F-5.9'] })
    .step('acquires a genuinely-held lease (the positive control — the id really exists)', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-cred' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD (a real lease, so a 404 below is not just 'unknown lease')`, { status: a.status, leaseId: a.leaseId });
    })
    .step('a FORGED ticket redeemed against that live lease is refused, no credential leaks', async (ctx) => {
      const r = await req('POST', `/v1/leases/${ctx.lease}/cas-cred`, { pat, body: { ticket: 'forged-not-the-minted-ticket' } });
      // 401 invalid ticket (broker armed, constant-time verify fails) OR 404 no-oracle (broker
      // inert / not-held) — both fail-closed. The load-bearing assertion: no cas_pat is issued.
      const failClosed = r.status === 401 || r.status === 404;
      const noCred = !bodyLeaksSecret(r.text) && !(r.text || '').includes('cas_pat');
      return check(failClosed && noCred, `POST cas-cred (forged) → ${r.status}, body carries NO cas_pat / no secret (fail-closed)`, {
        status: r.status,
        noCred,
        body: (r.text || '').slice(0, 120),
        gap: 'the full env-0 flow — the LEGIT single-use redeem at trusted boot → PAT → wipe (2nd redeem = 410 gone), and the env=0/proc=0/disk=0 credential-scan attestation — is the in-box boot path (X4-external); here we prove the redeem endpoint is fail-closed on a forged ticket',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.1 — "A job needs a Docker daemon / builds a container." 🔵 owner-gated (image capability).
// Docker-in-microVM is an IMAGE-LAYER decision, not a /v1 obligation — the box is a fresh
// Firecracker-class microVM (safe dind by construction). Observable: the lease on the pinned
// image is HELD; whether that image ships a daemon is the owner-gated GA image matrix.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · docker-build workload — the microVM box is HELD (daemon is image-gated)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('docker/container-build workload on a microVM box', { sid: ['S1.6.1'], persona: 'P1 CI engineer', atoms: ['F-4.7', 'F-6.1'] })
    .step('acquires the pinned microVM box a `docker build` step would run in → HELD', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-docker' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD on the @sha256-pinned image`, {
        status: a.status,
        leaseId: a.leaseId,
        gap: 'whether the box exposes a working Docker daemon (rootless buildkit / dind) is the OWNER-GATED GA image matrix, an image-layer concern not a /v1 verb; a job shelling `docker` on an image without it fails LOUD (command not found, red check) — never a silent pass. The microVM boundary makes even a privileged inner daemon safe by construction',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.2 — "A job needs a service container (Postgres/Redis)." 🔵 owner-gated (Actions shim).
// The `services:` block is the Actions agent's job, inside ONE microVM / ONE billable slot —
// three sidecars are NOT three runners. The /v1-observable, load-bearing claim: one job = one
// slot on the usage counter, no matter how many sidecars the workflow declares.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a service-container job is ONE billable slot (sidecars are free)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('services: sidecars ride one slot (not extra runners)', { sid: ['S1.6.2'], persona: 'P1 CI engineer', atoms: ['F-4.7'] })
    .step('records the baseline, then acquires one runner for a DB-backed job', async (ctx) => {
      const u0 = await usage(pat); ctx.base = u0.activeNow ?? 0;
      const a = await acquire(pat, { expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-svc' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD (base active=${ctx.base})`, { status: a.status, leaseId: a.leaseId, base: ctx.base });
    })
    .step('the job (its Postgres + Redis sidecars live inside) counts as exactly +1 slot', async (ctx) => {
      const u = await usage(pat);
      const oneSlot = (u.activeNow ?? 0) === ctx.base + 1;
      return check(u.status === 200 && oneSlot, `active_now=${u.activeNow} = base+1 — the whole job+sidecars is ONE billable slot`, {
        activeNow: u.activeNow,
        base: ctx.base,
        gap: 'the actual services: shim (Actions agent brings up the sidecar network on localhost:5432, torn down with the job) is agent-native / image-capability (X4-external); here we prove the ONE-slot-per-job billing invariant',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.7.1 / S1.7.2 / S1.7.3 — "Hermetic / lockfile ecosystems drop in unmodified." 🟡 built.
// Bazel action digests, Nix store paths, and poetry.lock are all natural memo keys — the deepest
// fit with the CAS. At /v1 the observable is the box the unmodified `bazel`/`nix`/`poetry` run
// lives in; the CAS-backed remote-exec / binary-cache / wheel-cache are tracked ADJACENCIES.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · bazel/nix/poetry drop in unmodified — the agent-native box is HELD', { skip }, async () => {
  const pat = P.pro;
  await new Journey('hermetic/lockfile ecosystem drop-in (agent-native box)', { sid: ['S1.7.1', 'S1.7.2', 'S1.7.3'], persona: 'P1 build-systems dev', atoms: ['F-4.3', 'F-4.7'] })
    .step('the unmodified bazel/nix/poetry workflow acquires its box → HELD', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-hermetic' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD — the unmodified build tool runs on the Actions agent here`, {
        status: a.status,
        leaseId: a.leaseId,
        gap: 'the DEEP wins are tracked adjacencies, NOT built: Bazel remote-exec/remote-cache pointed at the CAS; a CAS-backed Nix binary cache hydrating /nix/store paths; a warm poetry wheel/venv cache keyed on poetry.lock. All agent-native + X4-external. A missing daemon/tool fails LOUD (S1.6.3), never memoizes a non-hermetic result',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.3 / S1.6.9 — "A tool not in the base image + artifacts between jobs." 🟢 mechanism / 🟡.
// The box is a real ephemeral environment (install steps run like on any runner) with a scratch
// tmp_root; artifacts transit GitHub's store across ephemeral runners. Observable at /v1: the box
// is HELD with a valid, sanitized tmp_root — the install + artifact transit are in-box / agent.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · tool-install + cross-job artifacts — a real ephemeral box with a scratch root', { skip }, async () => {
  const pat = P.pro;
  await new Journey('real ephemeral environment: install-then-memoize + artifact handoff', { sid: ['S1.6.3', 'S1.6.9'], persona: 'P1 CI engineer', atoms: ['F-4.3', 'F-4.7'] })
    .step('the build job acquires a real box with a valid scratch tmp_root → HELD', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-dw-install' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `lease ${a.leaseId} HELD — a real env where setup-*/apt-get install steps run`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the box tears down (ephemeral) — a downstream job would get its own fresh box', async (ctx) => {
      const c = await closeLease(pat, ctx.lease);
      ctx.closed = c.status >= 200 && c.status < 300;
      return check(ctx.closed, `close → ${c.status}; the artifact handoff would transit GitHub's store between the two ephemeral runners`, {
        status: c.status,
        gap: 'the install itself + the warm hydrate (a setup-node/rustup fetch already in CAS = a lookup, not a download) is in-box (X4); upload/download-artifact transits the GitHub artifact store (agent-native, zero fabric involvement); loud-fail on a missing tool is agent-native. The fabric own artifacts[path‖digest] is the ATTESTATION binding (S1.5.1), a different mechanism',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease && !ctx.closed) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.11 / S1.6.13 — "A typo'd/reserved label, and any trigger shape." 🟢 LIVE (webhook path).
// matchManagedLabels (reserved-label refusal, subset-gate) and trigger-agnostic workflow_job
// spawn live on the WORKER WEBHOOK, not /v1 — GAP-heavy for this suite. The positive control is
// that the /v1 acquire path itself is uniform: it has no trigger/label concept, every acquire is
// the same code path, so a valid acquire is the closest observable analogue.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · label-matcher / trigger-agnostic spawn is the webhook path (GAP-heavy)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('label matcher + trigger-agnostic workflow_job spawn (webhook, not /v1)', { sid: ['S1.6.11', 'S1.6.13'], persona: 'P1 CI engineer', atoms: ['F-2.1', 'F-7.1'] })
    .step('the /v1 lease path is uniform — a valid acquire succeeds regardless of any trigger', async (ctx) => {
      const a = await acquire(pat, { expiryMs: 35000, tmpRoot: '/tmp/e2e-dw-trigger' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.state === 'held', `acquire → 200 HELD; the lease layer has NO trigger/label concept — cron, workflow_dispatch, push, re-run all reduce to this one code path`, {
        status: a.status,
        leaseId: a.leaseId,
        gap: 'matchManagedLabels (typo → 200 no-op / "waiting for a runner"; reserved `corelink-builder` refused; subset-gate no-partial-match) AND trigger-agnostic workflow_job spawn are LIVE on the Worker webhook (index.ts:1013) — proven per the S1.6.11/S1.6.13 cards but NOT reachable through /v1, so not e2e-probeable in this suite',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S1.6.12 — "Quota / vCPU-h exhaustion mid-pipeline." 🟢 cap LIVE / 🟡 wall built-not-armed.
// The concurrency cap is the always-live limit; the vCPU-h ComputeGate wall is default-off
// (owner-gated). Observable: GET /v1/usage surfaces plan_cap AND plan_ceiling_vcpu_h, so the
// customer sees the walls BEFORE hitting them (preventive, never a leaked overage).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · usage surfaces the concurrency cap + vCPU-h ceiling before you hit them', { skip }, async () => {
  const pat = P.pro;
  await new Journey('pre-emptive limit visibility (cap LIVE; vCPU-h wall owner-gated)', { sid: ['S1.6.12'], persona: 'P1 Pro dev', atoms: ['F-1.4', 'F-5.2'] })
    .step('reads usage — the concurrency cap is surfaced (the always-live limit)', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap;
      return check(u.status === 200 && u.cap === 10, `GET /v1/usage → 200, plan_cap=${u.cap} (Pro) — the live admission limit`, { status: u.status, cap: u.cap, activeNow: u.activeNow });
    })
    .step('the vCPU-h ceiling field is surfaced too (visible before the wall would bite)', async (ctx) => {
      const u = await usage(pat);
      const hasCeilingField = Object.prototype.hasOwnProperty.call(u.json ?? {}, 'plan_ceiling_vcpu_h');
      const ceiling = u.json?.plan_ceiling_vcpu_h ?? null;
      // The FIELD is always emitted (value may be null when the ceiling is disabled/absent) — that
      // is the preventive-visibility contract (P14): the customer sees the wall before hitting it.
      return check(u.status === 200 && hasCeilingField, `plan_ceiling_vcpu_h surfaced=${ceiling} (field present ⇒ pre-emptive visibility)`, {
        ceiling,
        hasCeilingField,
        gap: 'the vCPU-h ComputeGate WALL (refuse a new acquire once compute_accrued+Σreserved approaches max_vcpu_h) is DEFAULT-OFF / owner-gated (FABRIC_RUNNER_VCPU>0 + max_vcpu_h, S5.3.2); today only the concurrency cap bites — its enforcement is proven in the S1.3.2 ceiling journey (429 at cap). A mid-lease job already Held runs to completion; the NEXT acquire is what a wall would refuse',
      });
    })
    .run();
});
