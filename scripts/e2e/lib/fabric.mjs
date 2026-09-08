// Live-fabric client for the e2e suite — the surfaces a REAL USER can reach.
//
// Real-user fidelity: this hits the public HTTP surface exactly as a user would (a PAT in
// the Authorization header, the /v1 routes). It never fabricates privileged /internal calls
// as a *stimulus*. The PAT is loaded from env or the OOB secret file and is NEVER logged.
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const BASE = process.env.FABRIC_BASE_URL || 'https://corelink-fabricd.gmhelmold.workers.dev';

// The standing f0005 tenant PAT (proven live at review). Env wins; else the OOB file.
export function loadPat() {
  if (process.env.FABRIC_F0005_PAT) return process.env.FABRIC_F0005_PAT.trim();
  try {
    const patFile = process.env.CORELINK_E2E_PAT_FILE || join(homedir(), '.corelink/secrets/f0005-runners-item4-acquiring-pat.txt');
    return readFileSync(patFile, 'utf8').trim();
  } catch {
    return null;
  }
}

// Live cells only run when explicitly asked (E2E_LIVE=1) — never in the pure CI gate.
export const LIVE = process.env.E2E_LIVE === '1';

// The CoreLink multi-tenant e2e PATs (from e2e-prod-env.sh, sourced by run.sh). Each is a
// real tenant/tier PAT that introspects valid → authenticates against fabricd. Values never
// logged. Enables D2 (multi-tenant) + entitlement proofs that were previously marked X4.
export function tenantPats() {
  const e = process.env;
  return {
    free: e.CORELINK_E2E_PAT_FREE || null,
    solo: e.CORELINK_E2E_PAT_SOLO || null,
    pro: e.CORELINK_E2E_PAT_PRO || null,
    enterprise: e.CORELINK_E2E_PAT_ENTERPRISE || null,
    ro: e.CORELINK_E2E_PAT_RO || null,
    rw: e.CORELINK_E2E_PAT_RW || null,
    admin: e.CORELINK_E2E_PAT_ADMIN || null,
    tenantB: e.CORELINK_E2E_PAT_TENANT_B || null,
    tenantBAdmin: e.CORELINK_E2E_PAT_TENANT_B_ADMIN || null,
  };
}

// A single request. Returns {status, text, json, headers} — the caller asserts BEHAVIOR.
export async function req(method, path, { pat, body, headers = {}, timeoutMs = 15000 } = {}) {
  const h = { ...headers };
  if (pat) h.authorization = `Bearer ${pat}`;
  if (body !== undefined) h['content-type'] = 'application/json';
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: h,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(timeoutMs),
  });
  const text = await res.text();
  let json;
  try { json = JSON.parse(text); } catch { /* not json */ }
  const outHeaders = {};
  for (const [k, v] of res.headers) outHeaders[k] = v;
  return { status: res.status, text, json, headers: outHeaders };
}

// A deliberately-invalid image so an authed acquire stops at server-side image validation
// (400 = PAST auth) and never spawns a real box. The no-side-effect probe.
export const DUMMY_ACQUIRE = {
  image_digest: 'sha256:0000000000000000000000000000000000000000000000000000000000000000',
  net_policy: 'deny-all',
  tmp_root: '/tmp/e2e-probe',
  expiry_ms: 60000,
};

// A REAL content-pinned image (repo@sha256:<64hex>) that passes the supply-chain floor, so an
// acquire actually creates a HELD lease — the real-user path. Journeys close what they open.
export const VALID_IMAGE = 'alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc';

// ── Real-user lease actions (the verbs a user performs) ────────────────────────────────────
export async function acquire(pat, { image = VALID_IMAGE, netPolicy = 'deny-all', tmpRoot = '/tmp/e2e-journey', expiryMs = 60000 } = {}) {
  const r = await req('POST', '/v1/leases', { pat, body: { image_digest: image, net_policy: netPolicy, tmp_root: tmpRoot, expiry_ms: expiryMs } });
  return { status: r.status, leaseId: r.json?.lease?.lease_id ?? null, state: r.json?.lease?.state ?? null, json: r.json, text: r.text };
}
export async function getLease(pat, id) {
  const r = await req('GET', `/v1/leases/${id}`, { pat });
  return { status: r.status, state: r.json?.state ?? r.json?.lease?.state ?? null, json: r.json, text: r.text };
}
export async function listLeases(pat) {
  const r = await req('GET', '/v1/leases', { pat });
  return { status: r.status, tenant: r.json?.tenant ?? null, leases: r.json?.leases ?? [], json: r.json };
}
export async function closeLease(pat, id, status = 'succeeded') {
  // The close route expects CloseRequest { status, check_result?, cost_usd_micros? } — a real
  // client reports the job outcome. A bare POST is 415; a body missing `status` is 422.
  // Closing a HELD lease runs real teardown (box reclaim) → allow a longer timeout.
  const r = await req('POST', `/v1/leases/${id}/close`, { pat, body: { status }, timeoutMs: 90000 });
  return { status: r.status, json: r.json, text: r.text };
}
export async function usage(pat) {
  const r = await req('GET', '/v1/usage', { pat });
  return { status: r.status, tenant: r.json?.tenant ?? null, cap: r.json?.plan_cap ?? null, activeNow: r.json?.active_now ?? null, json: r.json };
}

// Assert a captured body carries NO secret-shaped material (G2 non-leak invariant).
// The `corelink_` alternative is load-bearing: this fabric's real PATs are `corelink_<...>`
// (96 chars) — WITHOUT it the sweep would miss a leaked PAT entirely (audit finding F8).
// Cred-tickets and CAS-PATs share the family; the generic long-token arm is a backstop.
const SECRET_RE = /(corelink_[a-z0-9]{16,})|(bearer\s+[a-z0-9._-]{16,})|(pat_[a-z0-9]{12,})|(cas[_-]?pat[_-][a-z0-9]{12,})|(-----BEGIN)|(sk-[a-z0-9]{16,})/i;
export function bodyLeaksSecret(text) {
  return SECRET_RE.test(text || '');
}
