// STORY JOURNEYS · the attacker (persona P7) — ADVERSARIAL, live-run, audited.
//
// These are the SECURITY stories. The rigor bar is the highest in the suite: a false-green here
// is the worst failure. Every rejection assertion carries a DISCRIMINATING POSITIVE CONTROL —
// the legitimate case that SUCCEEDS where the attack FAILS — so a 401/404/400/429 is proven to
// mean "isolation held", never "the fabric was simply down / the id was simply random".
//
// HONESTY POSTURE (AUTHORING-GUIDE rule 3). Several P7 attacks land on box-internal state that
// the public /v1 API cannot reach from the outside: a fence escape, a secret scan, an IMDS
// probe, cache-poisoning, envelope-ingest flooding. For those the journey asserts the
// API-level guardrail that IS observable (net_policy forced/rejected, the ingest/PAT seam
// separation, the tmp_root injection guard, the published attestation key) AND records the
// residual in the step artifact as `{ gap: '…', reachableFrom: '…' }`. NEVER a faked green for
// an in-box behaviour that this API surface cannot exercise.
//
// Run (tech lead, serially, then audits the emitted narrative):
//   E2E_LIVE=1 E2E_RUN_ID=journeys node --test scripts/e2e/journeys/attacker.test.mjs
import { test } from 'node:test';
import { Journey, check } from '../lib/journey.mjs';
import {
  tenantPats, acquire, getLease, listLeases, closeLease, usage, req,
  bodyLeaksSecret, LIVE, VALID_IMAGE, DUMMY_ACQUIRE,
} from '../lib/fabric.mjs';

const P = tenantPats();
// Cross-tenant journeys need a REAL live foreign lease (tenant A holds, tenant B is denied), so
// the file gates on two distinct tenant PATs — the pro-only journeys are a subset of that guard.
const skip = !LIVE ? 'set E2E_LIVE=1' : (P.pro && P.tenantB ? false : 'two tenant PATs absent (source e2e-prod-env.sh)');

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.1 — Attempt a fence escape. The path-fence (`..`/absolute/prefix-collision) is enforced
// IN-BOX by the FenceManifest at exec, not at the API. The observable API-level cousin is the
// acquire-time tmp_root guard: an unsafe or relative `tmp_root` is rejected BEFORE any box.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a job cannot smuggle a path outside its claimed set (tmp_root guard + in-box fence gap)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Fence escape is rejected — tmp_root injection guard, in-box FenceManifest recorded as gap', { sid: ['S7.1'], persona: 'P7 attacker', atoms: ['F-4.2', 'F-4.4'] })
    .step('POSITIVE CONTROL — a clean, absolute tmp_root is admitted (a legit job runs)', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-fence-ok' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `clean absolute tmp_root → 200 HELD (${a.leaseId})`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — a shell-injection tmp_root is rejected BEFORE any box (400, no lease)', async () => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/x; rm -rf /' });
      return check(a.status === 400 && !a.leaseId, `injection tmp_root → ${a.status} invalid, no box spawned`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — a relative tmp_root (ambient-FS escape) is rejected (400, no lease)', async () => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: 'relative/escape' });
      return check(a.status === 400 && !a.leaseId, `relative tmp_root → ${a.status} invalid (fence is the claimed set, not ambient FS)`, {
        status: a.status,
        gap: 'The `..` traversal / absolute-injection / `srcfoo`-vs-`src/` prefix-collision fence is enforced by the per-claim FenceManifest at EXEC time inside the box (C5a suite, contract §4).',
        reachableFrom: 'in-box only — needs a spawned box shell running a job against the FenceManifest, not the API.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.2 — Attempt to exfiltrate a secret from the box. env-0: the CAS PAT is never in the box
// env/argv/disk. The scan (env=0/proc=0/disk=0) is in-box; the API-observable invariant is that
// NO secret material ever appears on the wire of a real provisioned lease's responses.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · no secret material leaks on the wire of a real provisioned lease (env-0 in-box scan is a gap)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Secret exfil — the wire never carries a PAT/cred-ticket; the in-box env-0 scan recorded as gap', { sid: ['S7.2'], persona: 'P7 attacker', atoms: ['F-4.2', 'F-5.9'] })
    .step('POSITIVE CONTROL — a real box IS provisioned (the thing a secret could leak from exists)', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-exfil' });
      ctx.lease = a.leaseId;
      ctx.acquireBody = a.text;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD — a genuinely provisioned box`, { status: a.status, leaseId: a.leaseId });
    })
    .step('the acquire response body carries NO secret-shaped material', async (ctx) => {
      const leaks = bodyLeaksSecret(ctx.acquireBody);
      return check(!leaks, `acquire body clean (no corelink_/PAT/cred-ticket/PEM on the wire)`, { leaks });
    })
    .step('reading the lease back leaks no secret either (the box holds only a soon-dead ticket, never the PAT)', async (ctx) => {
      const g = await getLease(pat, ctx.lease);
      return check(g.status === 200 && !bodyLeaksSecret(g.text), `GET lease body clean, state=${g.state}`, {
        status: g.status,
        leaks: bodyLeaksSecret(g.text),
        gap: 'The env=0/proc=0/disk=0 credential-scan attestation (fail-closed on any unparseable scan) runs INSIDE the box against its env/proc/disk.',
        reachableFrom: 'in-box only — needs a spawned box shell scanning env/proc/disk, not the API.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.3 — Attempt to forge a verdict. The forgery defence is `verify_strict` binding the full
// outcome (exit+artifacts+refs) against the fabric's ed25519 key. The API-observable half is
// that the PUBLIC KEY is published (unauthenticated) and stable — the mechanism that lets ANY
// client detect a forged verdict. The tamper-rejection itself is client/SDK-side (a gap here).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the fabric publishes a stable attestation key so a forged verdict is client-detectable', { skip }, async () => {
  await new Journey('Forge a verdict — the published verify key is the anti-forge mechanism; verify_strict is client-side (gap)', { sid: ['S7.3'], persona: 'P7 attacker / MITM', atoms: ['F-3.1', 'F-3.3', 'F-5.4'] })
    .step('POSITIVE CONTROL — the attestation key is served unauthenticated (200, real pubkey)', async (ctx) => {
      const r = await req('GET', '/v1/attestation/key');
      const entry = r.json?.keys?.[0] ?? null;
      ctx.pubkey = entry?.pubkey_b64 ?? null;
      const ok = r.status === 200 && typeof ctx.pubkey === 'string' && ctx.pubkey.length >= 32;
      return check(ok, `GET /v1/attestation/key → ${r.status}, pubkey_b64 present (key_id=${entry?.key_id})`, { status: r.status, key_id: entry?.key_id, pubkeyLen: ctx.pubkey?.length ?? 0 });
    })
    .step('the published key is STABLE across reads (a client can pin it to verify every verdict)', async (ctx) => {
      const r2 = await req('GET', '/v1/attestation/key');
      const again = r2.json?.keys?.[0]?.pubkey_b64 ?? null;
      return check(r2.status === 200 && again === ctx.pubkey, `key stable across two reads (pinnable) → ${again === ctx.pubkey}`, {
        stable: again === ctx.pubkey,
        gap: 'v2 verify_strict rejecting a flipped exit:1→0 or a rewritten artifacts[] (result_binding_sig_v2, conformance_result_binding_v2.rs) is exercised in the SDK/Rust suite, not over the live HTTP API.',
        reachableFrom: 'client verify / Rust conformance — the fabric HTTP surface only PUBLISHES the key, it does not expose a tamper-a-verdict endpoint.',
      });
    })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.4 — Attempt cross-tenant access. The sharpest proof: a foreign tenant's LIVE lease and a
// random UNKNOWN id collapse to the SAME 404 — no existence oracle. (Complements the isolation
// journey in security.test.mjs with the unknown-vs-foreign indistinguishability direction.)
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a foreign live lease and an unknown id are byte-identical 404s (no existence oracle)', { skip }, async () => {
  const A = P.pro;      // tenant A owns a genuinely-existing lease
  const B = P.tenantB;  // tenant B is the attacker
  await new Journey('Cross-tenant — a real foreign lease is indistinguishable from a non-existent one (unified 404)', { sid: ['S7.4'], persona: 'P7 attacker (tenant B)', atoms: ['F-3.2', 'F-3.3'] })
    .step('POSITIVE CONTROL — tenant A opens a lease that DEMONSTRABLY exists', async (ctx) => {
      const a = await acquire(A, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-xt' });
      ctx.leaseA = a.leaseId;
      const g = await getLease(A, ctx.leaseA);
      return check(a.status === 200 && g.status === 200 && g.state === 'held', `A holds ${ctx.leaseA} and reads it 200/held (it truly exists)`, { status: a.status, ownerRead: g.status });
    })
    .step('tenant B GETs A\'s LIVE lease → 404 (existing-but-not-yours)', async (ctx) => {
      const g = await getLease(B, ctx.leaseA);
      ctx.foreignStatus = g.status;
      ctx.foreignBody = (g.text || '').slice(0, 160);
      return check(g.status === 404, `B GET A's real lease → ${g.status} (must be 404, never 403 — a 403 would confirm existence)`, { status: g.status, body: ctx.foreignBody });
    })
    .step('tenant B GETs a random UNKNOWN id → the SAME 404, indistinguishable (no oracle)', async (ctx) => {
      const g = await getLease(B, 'lease-00000000-0000-0000-0000-000000000000');
      const identical = g.status === ctx.foreignStatus;
      return check(g.status === 404 && identical, `unknown id → ${g.status}; identical to the foreign-lease 404 → ${identical} (no existence oracle)`, { status: g.status, identical });
    })
    .onCleanup(async (ctx) => { if (ctx.leaseA) await closeLease(A, ctx.leaseA); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.5 — Attempt a supply-chain injection (unpinned image). The X4 verify-before-spawn floor:
// an unpinned image is 400 BEFORE any box contact; a content-pinned image is admitted.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · an unpinned image is refused before any box contact (X4 verify-before-spawn floor)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Supply-chain injection — unpinned image 400 before box; pinned image admitted', { sid: ['S7.5'], persona: 'P7 attacker', atoms: ['F-4.5'] })
    .step('POSITIVE CONTROL — a content-pinned image (repo@sha256:…) is admitted → HELD', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-x4-ok' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `pinned image → 200 HELD (${a.leaseId})`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — an unpinned image (bare sha256, no repo) is rejected 400 BEFORE any spawn', async () => {
      const a = await acquire(pat, { image: DUMMY_ACQUIRE.image_digest, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-x4-bad' });
      return check(a.status === 400 && !a.leaseId, `unpinned image → ${a.status} invalid, no box (a fat-fingered tag can never ship unverified)`, { status: a.status, leaseId: a.leaseId });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.6 — Probe the metadata/IMDS egress block (G2, owner-gated tracked gap). Honest: the
// exact-host denylist is partial (CIDR inert, raw sockets bypass) and lives in-box. The
// API-observable posture is that egress isolation is SERVER-AUTHORITATIVE at admission — an
// isolated policy is admitted, a permissive one rejected — so a caller can't ASK for IMDS reach.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · IMDS/metadata reach cannot be requested — isolated net_policy admitted, permissive rejected (G2 gap)', { skip }, async () => {
  const pat = P.pro;
  await new Journey('IMDS probe — egress is server-authoritative at admission; the metadata denylist itself is the honest G2 gap', { sid: ['S7.6'], persona: 'P7 attacker', atoms: ['F-4.2'] })
    .step('POSITIVE CONTROL — an isolated (deny-all) lease is admitted', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, netPolicy: 'deny-all', expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-imds' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `isolated lease → 200 HELD (${a.leaseId})`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — asking for a permissive net_policy to widen egress is refused at admission (400)', async () => {
      const a = await acquire(pat, { image: VALID_IMAGE, netPolicy: 'open', expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-imds-open' });
      return check(a.status === 400 && !a.leaseId, `permissive net_policy → ${a.status} invalid (isolation is not caller-selectable)`, {
        status: a.status,
        gap: 'The metadata/IMDS block (169.254.169.254 / metadata.google.internal / link-local) is a PARTIAL exact-host denylist: CIDR ranges are inert (no CIDR math in simpleGlobMatch) and raw sockets bypass the SDK proxy. G2 is NOT closed on the CF path — it needs platform-network-layer filtering (owner-gated follow-up).',
        reachableFrom: 'in-box only — needs a spawned box issuing a real link-local request; the per-lease microVM boundary still contains blast radius.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.7 — Suspend an over-ceiling / abusive tenant (built-not-proven). The durable
// fabric_suspended_tenants gate is consulted at admission, but SUSPENDING is an OPERATOR action
// on an /internal route — a tenant cannot self-serve it. Observable: the suspend route is not
// tenant-reachable, and an un-suspended tenant passes admission (the gate is armed, allowing).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a tenant cannot suspend a tenant — suspend is operator-gated; admission consults the durable gate', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Suspend abuse — the suspend control is operator-only; the durable-table enforcement is the gap', { sid: ['S7.7'], persona: 'P7 attacker', atoms: ['F-1.6'] })
    .step('POSITIVE CONTROL — an un-suspended tenant passes admission (the gate is armed and ALLOWS)', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-suspend' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `un-suspended tenant admitted → 200 (fabric_suspended_tenants consulted, not blocking)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — a tenant PAT cannot drive the operator suspend route (not a self-serve weapon)', async (ctx) => {
      // The suspend control is an /internal operator route; a tenant PAT is not an operator.
      const r = await req('POST', '/internal/v1/admin/tenants/f0002/suspend', { pat });
      const notGranted = r.status !== 200 && r.status !== 204;
      return check(notGranted, `tenant PAT → suspend route → ${r.status} (NOT a 2xx — a tenant cannot suspend a tenant)`, {
        status: r.status,
        gap: 'The durable fabric_suspended_tenants admission cut-off (fabric-wide, N>1-safe, reversible by a table delete) is enforced against an operator-written pg row; CF-path enforcement is an ADR-0009 follow-up (built-not-proven).',
        reachableFrom: 'operator credential + pg table write — not the tenant /v1 API.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.8 — A compromised customer GitHub App install (built-not-proven, blast-radius bounded). The
// mint path (installation_id+repo → server-derived tenant) is a webhook seam, not tenant-API
// reachable. Observable: the blast radius is bounded — a compromised install is still ONE tenant,
// finitely capped (usage.cap), and cannot cross into another tenant (a live foreign lease → 404).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a compromised App install stays bounded to one capped tenant, never crosses tenancy', { skip }, async () => {
  const A = P.pro;
  const B = P.tenantB;
  await new Journey('Compromised install — blast radius is one finitely-capped tenant; the mint derivation is the gap', { sid: ['S7.8'], persona: 'P7 attacker (holds a customer install)', atoms: ['F-4.2', 'F-5.8'] })
    .step('POSITIVE CONTROL — the tenant\'s concurrency cap is a FINITE COGS bound (the ceiling on a compromised install)', async (ctx) => {
      const u = await usage(A);
      ctx.cap = u.cap;
      return check(u.status === 200 && typeof u.cap === 'number' && u.cap > 0, `tenant cap = ${u.cap} (a compromised install can burn at most this many parallel slots)`, { cap: u.cap });
    })
    .step('tenant A opens a real lease (the resource a breached install would operate on)', async (ctx) => {
      const a = await acquire(A, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-appinstall' });
      ctx.leaseA = a.leaseId;
      return check(a.status === 200 && a.leaseId, `A holds ${a.leaseId}`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — a breached install in tenant B cannot reach into tenant A (cross-tenant → 404)', async (ctx) => {
      const g = await getLease(B, ctx.leaseA);
      return check(g.status === 404, `B (the compromised install's tenant) GET A's lease → ${g.status} (blast radius stops at the tenant boundary)`, {
        status: g.status,
        gap: 'The per-installation token scoping + server-side tenant derivation (installation_id+repo → tenant, spawn_forbidden on a bad derivation) runs on the GitHub-App/webhook mint path.',
        reachableFrom: 'GitHub webhook + installationToken mint — not the tenant PAT /v1 API.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.leaseA) await closeLease(A, ctx.leaseA); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.9 — Webhook replay. The webhook (workflow_job) is the Cloudflare Worker seam, HMAC-gated,
// with claimSpawn/claimCompletion idempotency — it is NOT a route on the fabricd /v1 API. The
// observable fabricd-side invariant: no unauthenticated mutation exists for a replay to drive.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the fabricd API exposes no unauthenticated mutation a replayed webhook could drive', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Webhook replay — idempotency lives at the CF Worker seam; the fabricd API has no anon mutation (gap)', { sid: ['S7.9'], persona: 'P7 attacker', atoms: ['F-5.8', 'F-7.1'] })
    .step('POSITIVE CONTROL — an AUTHED mutation (acquire) works for a real tenant', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-webhook' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `authed acquire → 200 HELD (${a.leaseId})`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — the same mutation with NO credential (what a replay reaching fabricd would have) → 401', async () => {
      const r = await req('POST', '/v1/leases', { body: { image_digest: VALID_IMAGE, net_policy: 'deny-all', tmp_root: '/tmp/e2e-atk-webhook-anon', expiry_ms: 40000 } });
      return check(r.status === 401, `anon acquire → ${r.status} (no unauthenticated spawn surface on fabricd)`, {
        status: r.status,
        gap: 'Webhook HMAC verification + claimSpawn/claimCompletion exactly-once idempotency (a replayed queued/completed is a no-op) live in the Cloudflare Worker (index.ts), not on fabricd. Freshness (timestamp/nonce on the HMAC) is a tracked hardening, not built.',
        reachableFrom: 'CF Worker webhook seam + the GitHub HMAC secret — not authorable against the fabricd HTTP API.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.10 — A malicious net_policy request. On the untrusted check-exec path a permissive/forged
// net_policy is not honoured — it is REJECTED at admission (the allowed set is isolated-only), so
// a caller cannot talk their way past isolation. Even naming the runner egress sentinel fails.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a caller cannot widen egress via net_policy — permissive & egress-sentinel values are rejected', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Malicious net_policy — isolation is server-authoritative; a forged policy buys nothing', { sid: ['S7.10'], persona: 'P7 attacker', atoms: ['F-4.2', 'F-5.1'] })
    .step('POSITIVE CONTROL — a legitimate isolated policy (deny-all) is admitted → HELD', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, netPolicy: 'deny-all', expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-np-ok' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `isolated net_policy → 200 HELD (${a.leaseId})`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — a permissive "open" policy is rejected at admission (400, no egress bought)', async () => {
      const a = await acquire(pat, { image: VALID_IMAGE, netPolicy: 'open', expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-np-open' });
      return check(a.status === 400 && !a.leaseId, `net_policy "open" → ${a.status} invalid (check-exec accepts only the isolated set)`, { status: a.status });
    })
    .step('ATTACK — even naming the runner egress sentinel ("egress-runner") on a check lease is rejected (400)', async () => {
      const a = await acquire(pat, { image: VALID_IMAGE, netPolicy: 'egress-runner', expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-np-egress' });
      return check(a.status === 400 && !a.leaseId, `net_policy "egress-runner" on a check lease → ${a.status} (isolation is spec-derived, not string-derived — C2)`, {
        status: a.status,
        gap: 'The server-side FORCING (overwrite to egress-runner/egress-agent) that ignores the caller value is the runner/agent-mode acquire (req.runner/req.agent); the frozen check-exec acquire cannot reach that path — it instead REJECTS any non-isolated value.',
        reachableFrom: 'runner/agent-mode acquire (needs the runner/agent request fields), not the frozen check-exec acquire verb.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.11 — Credential-ticket replay across leases. The cred route verifies the ticket's signature
// OVER the lease_id before any lookup: a forged ticket → 401 invalid ticket on a REAL held lease,
// and the same on an unknown lease (no oracle). The 410-gone single-use latch needs a real ticket.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a forged cred-ticket is rejected on a live lease and yields no existence oracle', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Cred-ticket replay — lease-bound signature verify rejects a forged ticket; single-use latch is the gap', { sid: ['S7.11'], persona: 'P7 attacker', atoms: ['F-4.2', 'F-5.9'] })
    .step('POSITIVE CONTROL — a real HELD lease exists (the cred route is genuinely reachable)', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-cred' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — a forged ticket presented to the real lease → 401 invalid ticket (signature over lease_id fails)', async (ctx) => {
      const r = await req('POST', `/v1/leases/${ctx.lease}/cas-cred`, { body: { ticket: 'forged-ticket-not-signed-for-this-lease' } });
      ctx.realStatus = r.status;
      return check(r.status === 401, `forged ticket on real lease → ${r.status} (must be 401 — the ticket is bound to the lease_id)`, { status: r.status, body: (r.text || '').slice(0, 120) });
    })
    .step('ATTACK — the same forged ticket on an UNKNOWN lease → the same 401 (verify precedes lookup: no oracle)', async (ctx) => {
      const r = await req('POST', `/v1/leases/lease-00000000-0000-0000-0000-000000000000/cas-cred`, { body: { ticket: 'forged-ticket-not-signed-for-this-lease' } });
      const identical = r.status === ctx.realStatus;
      return check(r.status === 401 && identical, `unknown lease → ${r.status}; identical to the real-lease 401 → ${identical} (no existence oracle)`, {
        status: r.status,
        identical,
        gap: 'The single-use latch → 410 gone (Some⇒hand-out, None⇒410) fires only AFTER a first legitimate redemption of a REALLY-minted ticket; a forged ticket never reaches the latch.',
        reachableFrom: 'in-box only — needs the fabric-minted CLW_CRED_TICKET the box redeems once at boot.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.12 — A cache-poisoning attempt. Content-address + memo-key integrity + determinism guard are
// data-plane/close-path/in-box. The API-observable firewall: cross-tenant poisoning is
// UNREPRESENTABLE because cross-tenant dedup is not live and cross-tenant access is denied.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · cross-tenant cache-poisoning is unrepresentable — the tenant boundary is the firewall', { skip }, async () => {
  const A = P.pro;
  const B = P.tenantB;
  await new Journey('Cache-poisoning — the tenant boundary firewalls it; memo-key/content-address integrity is the data-plane gap', { sid: ['S7.12'], persona: 'P7 attacker (tenant B)', atoms: ['F-4.3', 'F-5.4'] })
    .step('POSITIVE CONTROL — tenant A has a real lease (its warm set is intra-tenant only)', async (ctx) => {
      const a = await acquire(A, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-poison' });
      ctx.leaseA = a.leaseId;
      return check(a.status === 200 && a.leaseId, `A holds ${a.leaseId} (dedup/warm set is intra-tenant at GA)`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — tenant B cannot even reach A\'s lease to plant a poisoned result (cross-tenant → 404)', async (ctx) => {
      const g = await getLease(B, ctx.leaseA);
      return check(g.status === 404, `B GET A's lease → ${g.status} (cross-tenant dedup is staged, not live — no shared set to poison)`, {
        status: g.status,
        gap: 'Content-address rejection of a tampered blob, memo-key integrity reject (memo_key ≠ SHA-256(LP(tree)‖LP(def)‖LP(toolchain)) → 400), and the determinism guard against canonizing a flaky green are close-path / in-box CAS mechanisms.',
        reachableFrom: 'close-path with a forged CheckResult / in-box CAS — plus client-side result_binding_sig_v2 verify — not a single API probe.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.leaseA) await closeLease(A, ctx.leaseA); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.13 — A malicious / oversized envelope-ingest. The write side is gated by a per-lease scoped
// ingest token (Bearer), NOT the tenant PAT. Observable seam separation: the PAT can READ a lease
// but CANNOT ingest (401) — the box never holds the PAT. The bounded-surface/overflow/malformed
// behaviour needs a valid ingest token (in-box).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the tenant PAT cannot write the turn-feed — ingest is a separate per-lease scoped token', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Envelope-ingest — read/write seam separation (PAT reads, cannot ingest); bounded surfaces are the in-box gap', { sid: ['S7.13'], persona: 'P7 attacker (in-box agent)', atoms: ['F-3.2', 'F-4.9'] })
    .step('POSITIVE CONTROL — the tenant PAT CAN read its own lease (the read seam works)', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-ingest' });
      ctx.lease = a.leaseId;
      const g = await getLease(pat, ctx.lease);
      return check(a.status === 200 && g.status === 200, `PAT reads lease ${ctx.lease} → 200 (read seam OK)`, { acquire: a.status, read: g.status });
    })
    .step('ATTACK — the tenant PAT presented as the ingest Bearer is NOT an ingest token → 401', async (ctx) => {
      const r = await req('POST', `/v1/leases/${ctx.lease}/envelope/ingest`, { pat, body: { kind: 'tool_call', tool: 'x'.repeat(4096) } });
      return check(r.status === 401, `PAT-as-ingest-token → ${r.status} (write side needs the per-lease scoped token, never the PAT)`, { status: r.status });
    })
    .step('ATTACK — an absent Bearer on the ingest route → 401 (fail-closed, write-only credential)', async (ctx) => {
      const r = await req('POST', `/v1/leases/${ctx.lease}/envelope/ingest`, { body: { kind: 'tool_call', tool: 'y' } });
      return check(r.status === 401, `no ingest Bearer → ${r.status} (fail-closed)`, {
        status: r.status,
        gap: 'The bounded surfaces (no durable spill, poll-drain), MAX_DISTINCT_TOOLS=256 / MAX_TOOL_NAME_LEN=128 folding, overflow→capture_incomplete, and malformed-event→400 all require a VALID per-lease ingest token to exercise.',
        reachableFrom: 'in-box only — the in-box agent loop holding the injected per-lease ingest token.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.14 — Ingest-token replay across the turn-feed. The ingest verify is constant-time over a
// lease-folded HMAC and runs BEFORE the registry lookup: a wrong token on a real lease and any
// token on a non-lease are byte-identical 401s (no oracle, no cross-lease acceptance).
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · a captured ingest token gives no cross-lease write and no existence oracle', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Ingest-token replay — lease-folded HMAC, verify-before-lookup: wrong-token & non-lease are identical 401s', { sid: ['S7.14'], persona: 'P7 attacker', atoms: ['F-4.9'] })
    .step('POSITIVE CONTROL — a real HELD lease exists (a genuine ingest target)', async (ctx) => {
      const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 40000, tmpRoot: '/tmp/e2e-atk-ingest-replay' });
      ctx.lease = a.leaseId;
      return check(a.status === 200 && a.leaseId, `lease ${a.leaseId} HELD`, { status: a.status, leaseId: a.leaseId });
    })
    .step('ATTACK — a captured/forged token replayed on the real lease → 401 (folds the wrong lease_id)', async (ctx) => {
      const r = await req('POST', `/v1/leases/${ctx.lease}/envelope/ingest`, { pat: 'captured-ingest-token-from-another-lease', body: { kind: 'tool_call', tool: 'x' } });
      ctx.realStatus = r.status;
      return check(r.status === 401, `forged token on real lease → ${r.status} (HMAC folds lease_id; cross-lease replay fails)`, { status: r.status });
    })
    .step('ATTACK — the same forged token on an UNKNOWN lease → the same 401 (auth precedes lookup: no oracle)', async (ctx) => {
      const r = await req('POST', `/v1/leases/lease-ffffffff-ffff-ffff-ffff-ffffffffffff/envelope/ingest`, { pat: 'captured-ingest-token-from-another-lease', body: { kind: 'tool_call', tool: 'x' } });
      const identical = r.status === ctx.realStatus;
      return check(r.status === 401 && identical, `unknown lease → ${r.status}; identical to real-lease 401 → ${identical} (constant-time verify before registry lookup)`, {
        status: r.status,
        identical,
        gap: 'Replay on the SAME lease with its OWN valid token is allowed-but-bounded (appends to that one lease\'s bounded, non-persisted feed) — verifying that requires the lease\'s genuine ingest token.',
        reachableFrom: 'in-box only — the in-box agent holding that lease\'s real ingest token.',
      });
    })
    .onCleanup(async (ctx) => { if (ctx.lease) await closeLease(pat, ctx.lease); })
    .run();
});

// ─────────────────────────────────────────────────────────────────────────────────────────────
// S7.15 — Queue-trigger dedup-cap exhaustion (built-not-proven). The 4096-entry bounded dedup map
// can only be exercised by a real trigger flood driving real leases. Observable: the trigger is
// authed/tenant-scoped (no PAT → 401) and capping is at ACQUIRE (usage.cap), never at trigger —
// so exhausting the dedup map cannot bypass the concurrency cap.
// ─────────────────────────────────────────────────────────────────────────────────────────────
test('JOURNEY · the queue trigger is authed & tenant-scoped; the cap is at acquire, so dedup exhaustion cannot bypass it', { skip }, async () => {
  const pat = P.pro;
  await new Journey('Queue-dedup exhaustion — cap-at-acquire not-at-trigger; the 4096-map exhaustion is the flood gap', { sid: ['S7.15'], persona: 'P7 attacker', atoms: ['F-4.9', 'F-5.2'] })
    .step('POSITIVE CONTROL — the concurrency cap (enforced at ACQUIRE) is a finite bound', async (ctx) => {
      const u = await usage(pat);
      ctx.cap = u.cap;
      return check(u.status === 200 && typeof u.cap === 'number' && u.cap > 0, `cap = ${u.cap} enforced at acquire (a trigger operates only on an already-admitted lease)`, { cap: u.cap });
    })
    .step('ATTACK — the trigger endpoint is not an anon surface: no PAT → 401 (tenant-scoped)', async () => {
      const r = await req('POST', '/v1/queue/trigger', { body: { item_id: 'x', tree_hash: 'deadbeef' } });
      return check(r.status === 401, `anon trigger → ${r.status} (authed, tenant-scoped — the dedup key folds tenant)`, {
        status: r.status,
        gap: 'The TRIGGER_DEDUP_CAP=4096 insertion-capped map (at-cap: serve-but-stop-memoizing → a later duplicate re-executes as a real, separately-metered, ceiling-bounded job) can only be exercised by a large flood of distinct (item_id, tree_hash) triggers on real leases.',
        reachableFrom: 'a hugit-driven trigger flood on admitted leases — not a single API probe; and every re-exec is itself capped+metered, so waste is ceiling-bounded, never unbounded.',
      });
    })
    .run();
});
